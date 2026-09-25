//! Piper TTS backend — full Rust via piper-rs (ONNX Runtime + espeak-rs).
//!
//! Multilingual neural TTS, <1s inference on CPU, 30+ languages.
//! Zero Python dependency. Model files auto-downloaded from HuggingFace.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use piper_rs::Piper;

use super::{SpeakOptions, TtsBackend};
use crate::config;

pub struct PiperBackend;

/// Cached model — (language, Piper instance).
/// Reloads when language changes (different ONNX model per language).
static MODEL: Mutex<Option<(String, Piper)>> = Mutex::new(None);

/// espeak-ng-data embedded at build time (staged by build.rs into OUT_DIR).
/// Needed because the espeak-ng library statically linked into vox has a
/// hard-coded data path from the CI builder that does not exist on user
/// machines. We extract this once and point espeak-rs at the result.
static ESPEAK_DATA: Dir<'_> = include_dir!("$OUT_DIR/espeak-ng-data");

static ESPEAK_DATA_INIT: OnceLock<Result<PathBuf, String>> = OnceLock::new();

fn models_dir() -> PathBuf {
    config::config_dir().join("piper")
}

/// Extract embedded espeak-ng-data to the user's config dir (once) and set the
/// `PIPER_ESPEAKNG_DATA_DIRECTORY` env var so espeak-rs can locate it.
fn ensure_espeak_data() -> Result<()> {
    let result = ESPEAK_DATA_INIT.get_or_init(|| {
        let parent = config::config_dir().join("piper");
        let data_dir = parent.join("espeak-ng-data");
        let sentinel = data_dir.join(".vox-extracted");

        if !sentinel.exists() {
            if data_dir.exists() {
                std::fs::remove_dir_all(&data_dir).map_err(|e| e.to_string())?;
            }
            std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
            ESPEAK_DATA
                .extract(&data_dir)
                .map_err(|e| format!("failed to extract espeak-ng-data: {e}"))?;
            std::fs::File::create(&sentinel).map_err(|e| e.to_string())?;
        }

        Ok(parent)
    });

    let parent = result.as_ref().map_err(|e| anyhow::anyhow!("{e}"))?;

    // SAFETY: espeak-rs reads this env var inside a OnceLock-guarded init that
    // runs on the first phonemizer call. We set it before any piper call, so
    // there is no race with other threads reading PIPER_ESPEAKNG_DATA_DIRECTORY.
    unsafe {
        std::env::set_var("PIPER_ESPEAKNG_DATA_DIRECTORY", parent);
    }
    Ok(())
}

/// Map lang code to (model_name, download_base_url).
fn model_for_lang(lang: &str) -> (&'static str, &'static str) {
    match lang {
        "fr" => (
            "fr_FR-tom-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/fr/fr_FR/tom/medium",
        ),
        "es" => (
            "es_ES-davefx-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/es/es_ES/davefx/medium",
        ),
        "de" => (
            "de_DE-thorsten-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/de/de_DE/thorsten/medium",
        ),
        "it" => (
            "it_IT-riccardo-x_low",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/it/it_IT/riccardo/x_low",
        ),
        "pt" => (
            "pt_BR-faber-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/pt/pt_BR/faber/medium",
        ),
        "zh" => (
            "zh_CN-huayan-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/zh/zh_CN/huayan/medium",
        ),
        "ja" => (
            "ja_JP-kokoro-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/ja/ja_JP/kokoro/medium",
        ),
        "ko" => (
            "ko_KR-kss-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/ko/ko_KR/kss/medium",
        ),
        "ru" => (
            "ru_RU-irina-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/ru/ru_RU/irina/medium",
        ),
        "ar" => (
            "ar_JO-kareem-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/ar/ar_JO/kareem/medium",
        ),
        "nl" => (
            "nl_NL-mls-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/nl/nl_NL/mls/medium",
        ),
        _ => (
            "en_US-lessac-medium",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium",
        ),
    }
}

/// Timeout for a single model download. Without one a dead connection hangs
/// the CLI forever with no output, which is what users actually reported.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Download a model file, refusing error responses.
///
/// Without `error_for_status` an HTTP 404 or 429 HTML body was written straight
/// into the `.onnx` file; the `if !path.exists()` guard then made that
/// corruption permanent, and every later run failed with an opaque ONNX error.
fn download(url: &str, what: &str) -> Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .context("failed to build HTTP client")?;
    let response = client.get(url).send().with_context(|| {
        format!("failed to download {what} (no network? try a different backend: vox -b say)")
    })?;
    let response = response
        .error_for_status()
        .with_context(|| format!("server refused to send {what}"))?;
    let bytes = response
        .bytes()
        .with_context(|| format!("failed to read {what}"))?;
    if bytes.is_empty() {
        anyhow::bail!("{what} came back empty");
    }
    Ok(bytes.to_vec())
}

/// Write to a temporary file and rename, so an interrupted download never
/// leaves a half-written model behind for the `exists()` guard to trust.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("failed to finalise {}", path.display()))?;
    Ok(())
}

/// Ensure model files exist, downloading if needed. Returns (onnx_path, json_path).
fn ensure_model(lang: &str) -> Result<(PathBuf, PathBuf)> {
    let (model_name, base_url) = model_for_lang(lang);
    let dir = models_dir();
    std::fs::create_dir_all(&dir).ok();

    let onnx_path = dir.join(format!("{model_name}.onnx"));
    let json_path = dir.join(format!("{model_name}.onnx.json"));

    if !onnx_path.exists() {
        eprintln!(
            "Downloading piper voice '{model_name}' (~60 MB, once) to {}...",
            dir.display()
        );
        let url = format!("{base_url}/{model_name}.onnx?download=true");
        let bytes = download(&url, &format!("{model_name}.onnx"))?;
        write_atomically(&onnx_path, &bytes)?;
        eprintln!(
            "Downloaded {} ({:.1} MB)",
            model_name,
            bytes.len() as f64 / 1_000_000.0
        );
    }

    if !json_path.exists() {
        let url = format!("{base_url}/{model_name}.onnx.json?download=true");
        let bytes = download(&url, &format!("{model_name}.onnx.json"))?;
        write_atomically(&json_path, &bytes)?;
    }

    Ok((onnx_path, json_path))
}

fn get_or_load_model(
    lang: &str,
) -> Result<std::sync::MutexGuard<'static, Option<(String, Piper)>>> {
    ensure_espeak_data()?;

    let mut guard = MODEL
        .lock()
        .map_err(|e| anyhow::anyhow!("model lock poisoned: {e}"))?;

    // Reload if language changed
    let need_reload = match &*guard {
        Some((cached_lang, _)) => cached_lang != lang,
        None => true,
    };

    if need_reload {
        let (onnx_path, json_path) = ensure_model(lang)?;
        let model = Piper::new(&onnx_path, &json_path)
            .map_err(|e| anyhow::anyhow!("failed to load piper model: {e}"))?;
        *guard = Some((lang.to_string(), model));
    }

    Ok(guard)
}

impl TtsBackend for PiperBackend {
    fn name(&self) -> &str {
        "piper"
    }

    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()> {
        let lang = opts.lang.as_deref().unwrap_or("en");

        let mut guard = get_or_load_model(lang)?;
        let (_, model) = guard.as_mut().context("model not loaded")?;

        let (audio_data, sample_rate) = model
            .create(text, false, None, None, None, None)
            .map_err(|e| anyhow::anyhow!("Piper TTS failed: {e}"))?;

        if audio_data.is_empty() {
            return Ok(());
        }

        // Write to temp WAV
        let tmp = tempfile::NamedTempFile::new().context("failed to create temp file")?;
        let wav_path = tmp.path().with_extension("wav");

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&wav_path, spec)?;
        for sample in &audio_data {
            let s = (*sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(s)?;
        }
        writer.finalize()?;

        // Play audio
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("afplay")
                .arg(&wav_path)
                .status()
                .context("failed to play audio")?;
        }
        #[cfg(not(target_os = "macos"))]
        {
            crate::audio::apply_wav_gain(&wav_path, opts.volume)?;
            crate::audio::play_wav_blocking(&wav_path)?;
        }

        let _ = std::fs::remove_file(&wav_path);
        Ok(())
    }

    fn list_voices(&self) -> Result<Vec<String>> {
        Ok(vec![
            "en_US-lessac-medium".into(),
            "fr_FR-siwis-medium".into(),
            "es_ES-davefx-medium".into(),
            "de_DE-thorsten-medium".into(),
            "it_IT-riccardo-x_low".into(),
            "pt_BR-faber-medium".into(),
            "zh_CN-huayan-medium".into(),
            "ja_JP-kokoro-medium".into(),
            "ko_KR-kss-medium".into(),
            "ru_RU-irina-medium".into(),
            "ar_JO-kareem-medium".into(),
            "nl_NL-mls-medium".into(),
        ])
    }

    fn is_available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomically_leaves_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("voice.onnx");
        write_atomically(&target, b"model bytes").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"model bytes");
        // The staging file must be gone, so the `exists()` guard cannot trust it.
        assert!(!target.with_extension("part").exists());
    }

    #[test]
    fn write_atomically_replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("voice.onnx");
        std::fs::write(&target, b"old").unwrap();
        write_atomically(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn download_refuses_an_error_response() {
        // A 404 used to be written into the model file verbatim.
        let err = download(
            "https://huggingface.co/rtk-ai/does-not-exist-404",
            "test.onnx",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("refused to send") || err.contains("failed to download"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn every_supported_language_maps_to_a_voice() {
        for lang in crate::config::SUPPORTED_LANGS {
            let (model, url) = model_for_lang(lang);
            assert!(!model.is_empty(), "{lang} has no model");
            assert!(url.starts_with("https://"), "{lang} has a bad url");
        }
    }
}
