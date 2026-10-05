//! Pocket TTS backend — pure Rust CPU inference via candle (pocket-tts crate).
//!
//! Kyutai's 100M-parameter FlowLM + Mimi codec model, designed for CPU execution.
//! Model is loaded once and kept warm in a global Mutex for the process lifetime.
//!
//! Two weight sets exist on HuggingFace:
//! - `kyutai/pocket-tts` (gated, needs HF_TOKEN + accepted license): full voice
//!   cloning from a reference WAV.
//! - `kyutai/pocket-tts-without-voice-cloning` (public): predefined voice
//!   embeddings only.
//!
//! When HF_TOKEN is not set we transparently fall back to the public weights,
//! so predefined voices work with zero setup (piper-style UX).

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use hf_hub::{Cache, Repo, RepoType};
use pocket_tts::TTSModel;
use pocket_tts::voice_state::ModelState;

use super::{SpeakOptions, TtsBackend};
use crate::audio;

/// Model variant published by Kyutai (config vendored, weights on HF).
const MODEL_VARIANT: &str = "b6369a24";

/// Config template vendored from the pocket-tts crate. The crate resolves
/// configs relative to its compile-time CARGO_MANIFEST_DIR, which does not
/// exist on end-user machines — so we stage our own copy under the vox config
/// dir and pass its absolute path as the "variant".
const CONFIG_TEMPLATE: &str = include_str!("pocket_config_b6369a24.yaml");

/// Gated weights with voice cloning support (requires HF_TOKEN).
const WEIGHTS_CLONING: &str =
    "hf://kyutai/pocket-tts/tts_b6369a24.safetensors@427e3d61b276ed69fdd03de0d185fa8a8d97fc5b";

/// Public weights without voice cloning.
const WEIGHTS_PUBLIC: &str = "hf://kyutai/pocket-tts-without-voice-cloning/tts_b6369a24.safetensors@d4fdd22ae8c8e1cb3634e150ebeff1dab2d16df3";

const DEFAULT_VOICE: &str = "alba";

/// Predefined voice embeddings published by Kyutai on HuggingFace (public repo).
const PREDEFINED_VOICES: &[&str] = &[
    "alba", "marius", "javert", "jean", "fantine", "cosette", "eponine", "azelma",
];

/// HuggingFace repo hosting the precomputed voice embeddings.
const VOICES_REPO: &str = "kyutai/pocket-tts-without-voice-cloning";

pub struct PocketBackend;

/// Global model instance — loaded once, stays warm for the process lifetime.
/// Uses Mutex because TTSModel contains candle state that is not Sync.
static MODEL: Mutex<Option<TTSModel>> = Mutex::new(None);

fn has_hf_token() -> bool {
    std::env::var("HF_TOKEN").is_ok_and(|t| !t.is_empty())
}

/// The weights `TTSModel::load` fetches: the gated set when a token allows it.
fn weights_path(voice_cloning: bool) -> &'static str {
    if voice_cloning {
        WEIGHTS_CLONING
    } else {
        WEIGHTS_PUBLIC
    }
}

/// Stage the model config under the vox config dir and return the absolute
/// path (without extension) to pass to `TTSModel::load` as the variant.
///
/// `find_config_path` in pocket-tts joins the variant onto candidate base
/// dirs; an absolute path replaces the base entirely, which lets us point it
/// at our staged config.
fn ensure_config(voice_cloning: bool) -> Result<PathBuf> {
    let dir = crate::config::config_dir().join("pocket");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create config dir: {}", dir.display()))?;
    let weights = weights_path(voice_cloning);
    let name = if voice_cloning {
        format!("{MODEL_VARIANT}-cloning")
    } else {
        format!("{MODEL_VARIANT}-public")
    };
    let yaml_path = dir.join(format!("{name}.yaml"));
    let content = CONFIG_TEMPLATE.replace("__WEIGHTS_PATH__", weights);
    std::fs::write(&yaml_path, content)
        .with_context(|| format!("failed to write model config: {}", yaml_path.display()))?;
    Ok(dir.join(name))
}

/// Whether a model is currently resident in this process. Used by
/// `vox daemon status` to report what is actually warm.
pub fn is_loaded() -> bool {
    MODEL.try_lock().map(|g| g.is_some()).unwrap_or(true)
}

/// The hf-hub cache pocket-tts downloads into, `None` without a home directory.
///
/// pocket-tts builds its client with `ApiBuilder::new()`, whose cache is
/// `~/.cache/huggingface/hub` on every platform: that constructor does not
/// read HF_HOME, and the platform cache directory (`~/Library/Caches` on
/// macOS) is never used.
fn hub_cache() -> Option<Cache> {
    dirs::home_dir().map(|home| Cache::new(home.join(".cache").join("huggingface").join("hub")))
}

/// Whether `weights`, an `hf://owner/repo/file[@revision]` path, is already in
/// `cache`. This is the lookup hf-hub does before it decides to download.
fn is_cached(cache: &Cache, weights: &str) -> bool {
    let mut parts = weights.trim_start_matches("hf://").splitn(3, '/');
    let (Some(owner), Some(name), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let (file, revision) = file.rsplit_once('@').unwrap_or((file, "main"));
    let repo = Repo::with_revision(
        format!("{owner}/{name}"),
        RepoType::Model,
        revision.to_string(),
    );
    cache.repo(repo).get(file).is_some()
}

/// Tell the user about the one-off model download before it starts.
///
/// hf-hub's progress bar hides itself when stderr is not a terminal, which is
/// exactly the case under the MCP server and Claude Code hooks — the first run
/// then looked like a multi-minute freeze with no output at all.
fn announce_first_run(weights: &str) {
    let cache = hub_cache();
    if cache
        .as_ref()
        .is_some_and(|cache| is_cached(cache, weights))
    {
        return;
    }
    eprintln!(
        "First run: downloading the pocket-tts model (~226 MB, once). \
         This can take a few minutes on a slow link. \
         Cache: {}",
        cache
            .map(|cache| cache.path().display().to_string())
            .unwrap_or_else(|| "~/.cache/huggingface/hub".to_string())
    );
}

pub fn with_model<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&TTSModel) -> Result<T>,
{
    let mut guard = MODEL
        .lock()
        .map_err(|e| anyhow::anyhow!("model lock poisoned: {e}"))?;
    if guard.is_none() {
        let cloning = has_hf_token();
        if !cloning {
            eprintln!(
                "HF_TOKEN not set — using public pocket-tts weights (predefined voices only)."
            );
        }
        announce_first_run(weights_path(cloning));
        eprintln!("Loading pocket-tts model {MODEL_VARIANT}...");
        let variant = ensure_config(cloning)?;
        let variant = variant.to_str().context("config path is not valid UTF-8")?;
        crate::timing::mark("pocket: config ready");
        let model = TTSModel::load(variant).context("failed to load pocket-tts model")?;
        *guard = Some(model);
        crate::timing::mark("pocket: model loaded");
    }
    f(guard.as_ref().unwrap())
}

/// Pre-load the model so subsequent calls are instant.
pub fn preload_model() -> Result<()> {
    with_model(|_| Ok(()))
}

/// Resolve the voice option to a pocket-tts voice state.
///
/// Accepts a predefined voice name (downloaded as a precomputed embedding from
/// HuggingFace), a path to a reference WAV (voice cloning), or a path to a
/// local `.safetensors` embedding.
fn resolve_voice_state(model: &TTSModel, voice: &str) -> Result<ModelState> {
    if PREDEFINED_VOICES.contains(&voice) {
        let hf_path = format!("hf://{VOICES_REPO}/embeddings/{voice}.safetensors");
        let local = pocket_tts::weights::download_if_necessary(&hf_path)
            .with_context(|| format!("failed to download voice embedding: {voice}"))?;
        return model
            .get_voice_state_from_prompt_file(&local)
            .with_context(|| format!("failed to load voice embedding: {voice}"));
    }
    if voice.ends_with(".safetensors") {
        return model
            .get_voice_state_from_prompt_file(voice)
            .with_context(|| format!("failed to load voice embedding file: {voice}"));
    }
    if voice.ends_with(".wav") {
        if !has_hf_token() {
            anyhow::bail!(
                "Voice cloning from a WAV needs the gated weights: set HF_TOKEN and accept \
                 the license at https://huggingface.co/kyutai/pocket-tts"
            );
        }
        return model
            .get_voice_state(voice)
            .with_context(|| format!("failed to encode reference audio: {voice}"));
    }
    anyhow::bail!(
        "Unknown pocket voice: {voice}. Use one of {}, a .wav file, or a .safetensors embedding",
        PREDEFINED_VOICES.join(", ")
    )
}

/// One generated frame, shaped `[batch, channels, samples]`, as mono samples.
fn mono_samples(frame: &candle_core::Tensor) -> Result<Vec<f32>> {
    let channels = frame
        .squeeze(0)
        .and_then(|frame| frame.to_vec2::<f32>())
        .context("unexpected audio frame shape")?;
    match channels.as_slice() {
        [] => Ok(Vec::new()),
        [only] => Ok(only.clone()),
        many => Ok((0..many[0].len())
            .map(|i| many.iter().map(|channel| channel[i]).sum::<f32>() / many.len() as f32)
            .collect()),
    }
}

impl TtsBackend for PocketBackend {
    fn name(&self) -> &str {
        "pocket"
    }

    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()> {
        // Reference audio (voice cloning) takes precedence over a named voice.
        let voice = opts
            .ref_audio
            .as_deref()
            .or(opts.voice.as_deref())
            .unwrap_or(DEFAULT_VOICE)
            .to_string();

        // Speaking: play each frame as the model produces it, so the first
        // sound comes after one frame of work instead of the whole utterance.
        if opts.output.is_none() {
            // Opened before the model loads, so the two waits overlap.
            let player = audio::Player::start(opts.volume);
            with_model(|model| {
                let voice_state = resolve_voice_state(model, &voice)?;
                crate::timing::mark("pocket: voice state ready");
                let sample_rate = model.sample_rate as u32;
                let mut frames = 0usize;
                let mut is_playing = true;
                for frame in model.generate_stream(text, &voice_state) {
                    let frame = frame.context("pocket-tts generation failed")?;
                    if !player.push(sample_rate, mono_samples(&frame)?) {
                        // The player is gone, so the device failed. `finish`
                        // below says why; generating more would be wasted.
                        is_playing = false;
                        break;
                    }
                    frames += 1;
                }
                crate::timing::mark("pocket: audio synthesized");
                if is_playing && frames == 0 {
                    anyhow::bail!("No audio generated");
                }
                Ok(())
            })?;
            return player.finish();
        }

        let tmp = tempfile::NamedTempFile::new().context("failed to create temp file")?;
        let wav_path = tmp.path().with_extension("wav");

        with_model(|model| {
            let voice_state = resolve_voice_state(model, &voice)?;
            crate::timing::mark("pocket: voice state ready");
            let audio_tensor = model
                .generate(text, &voice_state)
                .context("pocket-tts generation failed")?;
            crate::timing::mark("pocket: audio synthesized");
            // Buffered: handed a bare file, the writer issues one write per sample.
            let file = std::io::BufWriter::new(
                std::fs::File::create(&wav_path).context("failed to create audio file")?,
            );
            pocket_tts::audio::write_wav_to_writer(file, &audio_tensor, model.sample_rate as u32)
                .context("failed to save generated audio")
        })?;
        crate::timing::mark("pocket: wav written");

        audio::apply_wav_gain(&wav_path, opts.volume)?;
        audio::deliver(&wav_path, opts.output.as_deref())?;

        let _ = std::fs::remove_file(&wav_path);

        Ok(())
    }

    fn list_voices(&self) -> Result<Vec<String>> {
        Ok(PREDEFINED_VOICES.iter().map(|v| v.to_string()).collect())
    }

    fn is_available(&self) -> bool {
        // Always available since it's compiled in (pure Rust, CPU-only)
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lay a file out under `hub` the way hf-hub does after a download.
    fn store(hub: &std::path::Path, repo: &str, revision: &str, file: &str) {
        let repo = hub.join(format!("models--{}", repo.replace('/', "--")));
        std::fs::create_dir_all(repo.join("refs")).unwrap();
        std::fs::write(repo.join("refs").join(revision), revision).unwrap();
        let snapshot = repo.join("snapshots").join(revision);
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(snapshot.join(file), b"weights").unwrap();
    }

    #[test]
    fn hub_cache_is_the_one_hf_hub_downloads_into() {
        // `Cache::default()` is what `ApiBuilder::new()`, and so pocket-tts,
        // uses. Looking anywhere else announces a download on every load.
        assert_eq!(hub_cache().unwrap().path(), Cache::default().path());
    }

    #[test]
    fn weights_are_cached_only_once_hf_hub_has_stored_them() {
        let hub = tempfile::tempdir().unwrap();
        let cache = Cache::new(hub.path().to_path_buf());
        assert!(!is_cached(&cache, WEIGHTS_PUBLIC));

        store(
            hub.path(),
            "kyutai/pocket-tts-without-voice-cloning",
            "d4fdd22ae8c8e1cb3634e150ebeff1dab2d16df3",
            "tts_b6369a24.safetensors",
        );
        assert!(is_cached(&cache, WEIGHTS_PUBLIC));
        // The gated weights live in another repository and are still to fetch.
        assert!(!is_cached(&cache, WEIGHTS_CLONING));
    }

    #[test]
    fn a_repository_holding_only_voices_does_not_count_as_the_model() {
        let hub = tempfile::tempdir().unwrap();
        let cache = Cache::new(hub.path().to_path_buf());
        store(
            hub.path(),
            "kyutai/pocket-tts-without-voice-cloning",
            "main",
            "alba.safetensors",
        );
        assert!(!is_cached(&cache, WEIGHTS_PUBLIC));
    }

    #[test]
    fn a_path_without_a_revision_is_looked_up_on_main() {
        let hub = tempfile::tempdir().unwrap();
        let cache = Cache::new(hub.path().to_path_buf());
        store(hub.path(), "owner/repo", "main", "model.safetensors");
        assert!(is_cached(&cache, "hf://owner/repo/model.safetensors"));
        assert!(!is_cached(&cache, "hf://owner/repo"));
    }
}
