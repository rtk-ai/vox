//! Qwen-native TTS backend — pure Rust inference via candle (qwen3-tts-rs).
//!
//! Cross-platform with optional Metal (macOS) or CUDA (Linux) GPU acceleration.
//! Model is loaded once and kept warm in a global Mutex for the process lifetime.
//! Supports voice cloning via reference audio + text prompt.

use std::sync::Mutex;

use anyhow::{Context, Result};
use qwen3_tts::{AudioBuffer, Language, ModelPaths, Qwen3TTS};

use super::{SpeakOptions, TtsBackend};
use crate::audio;

/// Fallback when `models.toml` carries no `[qwen-native] model_id`.
const DEFAULT_MODEL: &str = "Qwen/Qwen3-TTS-12Hz-0.6B-Base";

/// Which Qwen3-TTS repo to load: the `--model` flag or `model` preference
/// first, then the `[qwen-native] model_id` in models.toml, then the default.
pub fn model_id(override_id: Option<&str>) -> String {
    if let Some(id) = override_id.map(str::trim).filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    crate::config::model_config_str("qwen-native", "model_id")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

pub struct QwenNativeBackend;

/// Whether a model is currently resident in this process. Used by
/// `vox daemon status` to report what is actually warm.
pub fn is_loaded() -> bool {
    MODEL.try_lock().map(|g| g.is_some()).unwrap_or(true)
}

/// Global model instance, paired with the repo id it was loaded from — kept
/// warm for the process lifetime. Uses Mutex because Qwen3TTS holds a RefCell
/// (not Sync). The id is stored so that asking for a *different* model reloads
/// instead of silently reusing the resident one.
static MODEL: Mutex<Option<(String, Qwen3TTS)>> = Mutex::new(None);

pub fn with_model<F, T>(model_id: Option<&str>, f: F) -> Result<T>
where
    F: FnOnce(&Qwen3TTS) -> Result<T>,
{
    let mut guard = MODEL
        .lock()
        .map_err(|e| anyhow::anyhow!("model lock poisoned: {e}"))?;
    load_if_needed(&mut guard, model_id)?;
    f(&guard.as_ref().unwrap().1)
}

/// Load the requested model unless that exact model is already resident.
fn load_if_needed(
    guard: &mut std::sync::MutexGuard<'_, Option<(String, Qwen3TTS)>>,
    requested: Option<&str>,
) -> Result<()> {
    let id = model_id(requested);
    if guard.as_ref().is_some_and(|(loaded, _)| *loaded == id) {
        return Ok(());
    }
    eprintln!("Loading model {id} (downloading if needed)...");
    let paths =
        ModelPaths::download(Some(&id)).context("failed to download model from HuggingFace Hub")?;
    let device = qwen3_tts::auto_device().context("failed to detect compute device")?;
    eprintln!("Using device: {device:?}");
    let model = Qwen3TTS::from_paths(&paths, device).context("failed to load Qwen3-TTS model")?;
    **guard = Some((id, model));
    Ok(())
}

/// Pre-load the model so subsequent calls are instant.
pub fn preload_model(model_id: Option<&str>) -> Result<()> {
    let mut guard = MODEL
        .lock()
        .map_err(|e| anyhow::anyhow!("model lock poisoned: {e}"))?;
    load_if_needed(&mut guard, model_id)?;
    Ok(())
}

/// Map our short language codes to qwen3_tts::Language.
pub fn parse_language(code: &str) -> Result<Language> {
    match code {
        "en" => Ok(Language::English),
        "fr" => Ok(Language::French),
        "es" => Ok(Language::Spanish),
        "de" => Ok(Language::German),
        "it" => Ok(Language::Italian),
        "pt" => Ok(Language::Portuguese),
        "zh" => Ok(Language::Chinese),
        "ja" => Ok(Language::Japanese),
        "ko" => Ok(Language::Korean),
        "ru" => Ok(Language::Russian),
        _ => anyhow::bail!(
            "Unsupported language for qwen-native: {code}. \
             Supported: en, fr, es, de, it, pt, zh, ja, ko, ru"
        ),
    }
}

impl TtsBackend for QwenNativeBackend {
    fn name(&self) -> &str {
        "qwen-native"
    }

    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()> {
        let lang = parse_language(opts.lang.as_deref().unwrap_or("en"))?;
        let ref_audio_path = opts.ref_audio.clone();
        let ref_text = opts.ref_text.clone();

        // Warn before loading the model (a ~1 GB download on first use), not
        // after: the Base checkpoint has no preset speakers and `synthesize`
        // takes no language, so the language only reaches the model through
        // the voice-clone path. Say so instead of ignoring the flag silently.
        //
        // Not for Japanese: it comes here by default, because piper cannot
        // speak it, and the advice to use piper would send the user back to
        // the backend that refused them.
        if let Some(code) = opts.lang.as_deref()
            && code != "ja"
            && ref_audio_path.is_none()
        {
            eprintln!(
                "Warning: -l/--lang has no effect on qwen-native without a voice clone \
                 (the Base model infers the language from the text). \
                 Use -v <clone> for language control, or -b piper for per-language voices."
            );
        }

        let mut audio_buf = with_model(opts.model.as_deref(), |model| {
            if let Some(ref path) = ref_audio_path {
                let ref_audio = AudioBuffer::load(path)
                    .with_context(|| format!("failed to load reference audio: {path}"))?;
                let prompt = model.create_voice_clone_prompt(&ref_audio, ref_text.as_deref())?;
                Ok(model.synthesize_voice_clone(text, &prompt, lang, None)?)
            } else {
                Ok(model.synthesize(text, None)?)
            }
        })?;

        // Apply volume gain to audio buffer
        if (opts.volume - 1.0).abs() > f32::EPSILON {
            for sample in &mut audio_buf.samples {
                *sample = (*sample * opts.volume).clamp(-1.0, 1.0);
            }
        }

        // Save to temp file and play with rodio
        let tmp = tempfile::NamedTempFile::new().context("failed to create temp file")?;
        let wav_path = tmp.path().with_extension("wav");
        audio_buf
            .save(&wav_path)
            .context("failed to save generated audio")?;

        audio::deliver(&wav_path, opts.output.as_deref())?;

        let _ = std::fs::remove_file(&wav_path);

        Ok(())
    }

    fn list_voices(&self) -> Result<Vec<String>> {
        // Base model doesn't have preset voices — voice cloning is the way
        Ok(vec!["(use voice clones with --voice)".into()])
    }

    fn is_available(&self) -> bool {
        // Always available since it's compiled in
        true
    }
}
