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
    let weights = if voice_cloning {
        WEIGHTS_CLONING
    } else {
        WEIGHTS_PUBLIC
    };
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
        eprintln!("Loading pocket-tts model {MODEL_VARIANT} (downloading if needed)...");
        let variant = ensure_config(cloning)?;
        let variant = variant.to_str().context("config path is not valid UTF-8")?;
        let model = TTSModel::load(variant).context("failed to load pocket-tts model")?;
        *guard = Some(model);
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

        let tmp = tempfile::NamedTempFile::new().context("failed to create temp file")?;
        let wav_path = tmp.path().with_extension("wav");

        with_model(|model| {
            let voice_state = resolve_voice_state(model, &voice)?;
            let audio_tensor = model
                .generate(text, &voice_state)
                .context("pocket-tts generation failed")?;
            pocket_tts::audio::write_wav(&wav_path, &audio_tensor, model.sample_rate as u32)
                .context("failed to save generated audio")
        })?;

        audio::apply_wav_gain(&wav_path, opts.volume)?;
        audio::play_wav_blocking(&wav_path)?;

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
