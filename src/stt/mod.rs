//! Speech-to-text — Whisper running locally on candle (pure Rust, all platforms).
//!
//! The model is downloaded from the Hugging Face hub on first use and kept
//! warm in a process-wide cache so that repeated `vox hear` calls inside the
//! daemon or MCP server do not reload weights.

pub mod whisper;

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};

use crate::mic;
pub use whisper::WhisperStt;

/// Default multilingual model: 99 languages, ~1 GB of RAM, fine on CPU.
pub const DEFAULT_MODEL: &str = "openai/whisper-small";

/// Higher-quality option for machines with a GPU and >= 16 GB of RAM
/// (~3.5 GB of RAM in f32). Select it with `VOX_STT_MODEL` or `--model`.
pub const QUALITY_MODEL: &str = "openai/whisper-large-v3-turbo";

static MODEL: Mutex<Option<WhisperStt>> = Mutex::new(None);

/// Model repo to use: `VOX_STT_MODEL` env var, else [`DEFAULT_MODEL`].
pub fn model_id(override_id: Option<&str>) -> String {
    if let Some(id) = override_id.filter(|s| !s.trim().is_empty()) {
        return id.to_string();
    }
    std::env::var("VOX_STT_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// Whisper language token name for a language code (`fr` -> `<|fr|>`).
pub fn language_token(lang: &str) -> String {
    format!("<|{}|>", lang.trim().to_lowercase())
}

/// Whether Whisper knows this language code.
pub fn is_supported_language(lang: &str) -> bool {
    whisper::LANGUAGES
        .iter()
        .any(|(code, _)| *code == lang.trim().to_lowercase())
}
/// Whether a model is currently resident in this process. Used by
/// `vox daemon status` to report what is actually warm.
pub fn is_loaded() -> bool {
    MODEL.try_lock().map(|g| g.is_some()).unwrap_or(true)
}

/// Run `f` against the cached model, loading it first if needed.
pub fn with_model<F, T>(override_id: Option<&str>, f: F) -> Result<T>
where
    F: FnOnce(&mut WhisperStt) -> Result<T>,
{
    let id = model_id(override_id);
    let mut guard = MODEL
        .lock()
        .map_err(|e| anyhow::anyhow!("STT model lock poisoned: {e}"))?;
    let reload = match guard.as_ref() {
        Some(m) => m.model_id() != id,
        None => true,
    };
    if reload {
        eprintln!("Loading STT model {id}...");
        let m = WhisperStt::load(&id).with_context(|| {
            format!("Failed to load STT model {id} (default: VOX_STT_MODEL={DEFAULT_MODEL})")
        })?;
        *guard = Some(m);
    }
    f(guard.as_mut().expect("model loaded"))
}

/// Load the model ahead of time (e.g. from the daemon or before a chat).
pub fn preload(override_id: Option<&str>) -> Result<()> {
    with_model(override_id, |_| Ok(()))
}

/// Transcribe 16 kHz mono samples. `lang = None` auto-detects the language.
pub fn transcribe_samples(samples: &[f32], lang: Option<&str>) -> Result<String> {
    transcribe_samples_with(samples, lang, None)
}

/// Like [`transcribe_samples`] with an explicit model repo override.
pub fn transcribe_samples_with(
    samples: &[f32],
    lang: Option<&str>,
    model: Option<&str>,
) -> Result<String> {
    if samples.len() < mic::TARGET_RATE as usize / 10 {
        return Ok(String::new());
    }
    if let Some(l) = lang
        && !is_supported_language(l)
    {
        anyhow::bail!("Language '{l}' is not supported by Whisper");
    }
    with_model(model, |m| m.transcribe(samples, lang))
}

/// Transcribe a WAV file (any rate/channels). `lang = None` auto-detects.
pub fn transcribe(audio_path: &str, lang: Option<&str>) -> Result<String> {
    let samples = mic::read_wav_16k(Path::new(audio_path))?;
    transcribe_samples(&samples, lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_id_prefers_override() {
        assert_eq!(
            model_id(Some("openai/whisper-small")),
            "openai/whisper-small"
        );
        assert_eq!(model_id(Some("  ")), model_id(None));
    }

    #[test]
    fn language_token_format() {
        assert_eq!(language_token("fr"), "<|fr|>");
        assert_eq!(language_token(" JA "), "<|ja|>");
    }

    #[test]
    fn supported_languages_cover_vox_langs() {
        for l in crate::config::SUPPORTED_LANGS {
            assert!(is_supported_language(l), "{l} missing");
        }
        assert!(!is_supported_language("xx"));
    }

    #[test]
    fn short_audio_is_empty_without_loading_model() {
        assert_eq!(transcribe_samples(&[0.0; 100], Some("en")).unwrap(), "");
    }

    #[test]
    fn unsupported_language_errors_before_loading_model() {
        let samples = vec![0.0f32; 16_000];
        let err = transcribe_samples(&samples, Some("klingon")).unwrap_err();
        assert!(err.to_string().contains("not supported"));
    }
}
