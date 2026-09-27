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

/// Fallback when `models.toml` carries no `[whisper] model_id`.
///
/// Measured on 11.85s of French audio, warm, Apple M2 (see models.toml):
/// tiny 0.93s / 350 MB, base 1.52s / 631 MB, small 5.07s / 1980 MB. `base` is
/// 3x faster and 3x lighter than `small` for one fewer mistake in 30 words.
pub const DEFAULT_MODEL: &str = "openai/whisper-base";

/// Higher-quality option for machines with a GPU and plenty of memory.
/// Select it with `vox config set stt_model`, `VOX_STT_MODEL` or `--model`.
pub const QUALITY_MODEL: &str = "openai/whisper-large-v3-turbo";

static MODEL: Mutex<Option<WhisperStt>> = Mutex::new(None);

/// Which Whisper repo to load, most specific source first: the `--model`
/// flag, `VOX_STT_MODEL`, the stored `stt_model` preference, the
/// `[whisper] model_id` in models.toml, then [`DEFAULT_MODEL`].
///
/// The env var sits above the stored preference on purpose: otherwise anyone
/// who ever ran `vox config set stt_model` could no longer override it for a
/// single shell.
pub fn model_id(override_id: Option<&str>) -> String {
    if let Some(id) = override_id.map(str::trim).filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    if let Some(id) = std::env::var("VOX_STT_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return id;
    }
    if let Some(id) = configured_model() {
        return id;
    }
    crate::config::model_config_str("whisper", "model_id")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// The `stt_model` preference, if the user set one.
fn configured_model() -> Option<String> {
    crate::db::open()
        .ok()
        .and_then(|conn| crate::db::get_preferences(&conn).ok())
        .and_then(|p| p.stt_model)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
        // Blank overrides are ignored, not passed through as a repo name.
        assert_eq!(model_id(Some("  ")), model_id(None));
        assert_eq!(
            model_id(Some(" openai/whisper-tiny ")),
            "openai/whisper-tiny"
        );
    }

    #[test]
    fn resolved_model_is_never_empty() {
        // Whichever layer answers — preference, env, models.toml or the
        // built-in — the result has to be a usable repo id.
        assert!(!model_id(None).trim().is_empty());
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
