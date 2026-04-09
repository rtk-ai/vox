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

const DEFAULT_MODEL: &str = "Qwen/Qwen3-TTS-12Hz-0.6B-Base";

pub struct QwenNativeBackend;

/// Global model instance — loaded once, stays warm for the process lifetime.
/// Uses Mutex because Qwen3TTS contains RefCell (not Sync).
static MODEL: Mutex<Option<Qwen3TTS>> = Mutex::new(None);

pub fn with_model<F, T>(model_id: Option<&str>, f: F) -> Result<T>
where
    F: FnOnce(&Qwen3TTS) -> Result<T>,
{
    let mut guard = MODEL
        .lock()
        .map_err(|e| anyhow::anyhow!("model lock poisoned: {e}"))?;
    if guard.is_none() {
        load_model_inner(&mut guard, model_id)?;
    }
    f(guard.as_ref().unwrap())
}

fn load_model_inner(
    guard: &mut std::sync::MutexGuard<'_, Option<Qwen3TTS>>,
    model_id: Option<&str>,
) -> Result<()> {
    let id = model_id.unwrap_or(DEFAULT_MODEL);
    eprintln!("Loading model {id} (downloading if needed)...");
    let paths =
        ModelPaths::download(Some(id)).context("failed to download model from HuggingFace Hub")?;
    let device = qwen3_tts::auto_device().context("failed to detect compute device")?;
    eprintln!("Using device: {device:?}");
    let model = Qwen3TTS::from_paths(&paths, device).context("failed to load Qwen3-TTS model")?;
    **guard = Some(model);
    Ok(())
}

/// Pre-load the model so subsequent calls are instant.
pub fn preload_model(model_id: Option<&str>) -> Result<()> {
    let mut guard = MODEL
        .lock()
        .map_err(|e| anyhow::anyhow!("model lock poisoned: {e}"))?;
    if guard.is_none() {
        load_model_inner(&mut guard, model_id)?;
    }
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

        audio::play_wav_blocking(&wav_path)?;

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
