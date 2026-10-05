//! Piper TTS backend — full Rust via piper-rs (ONNX Runtime + espeak-rs).
//!
//! Neural TTS, <1s inference on CPU. One default voice for each language in
//! `VOICES`; any other espeak-phonemized voice of rhasspy/piper-voices can be
//! named with `-v`. Zero Python dependency. Model files auto-downloaded from
//! HuggingFace.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use piper_rs::Piper;

use super::{SpeakOptions, TtsBackend};
use crate::config;

pub struct PiperBackend;

/// Cached model — (voice name, Piper instance).
/// Reloads when the voice changes (one ONNX model per voice).
static MODEL: Mutex<Option<(String, Piper)>> = Mutex::new(None);

/// Whether a voice is currently resident in this process. Used by
/// `vox daemon status` to report what is actually warm.
pub fn is_loaded() -> bool {
    MODEL.try_lock().map(|g| g.is_some()).unwrap_or(true)
}

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

/// The default voice of each language. A voice name alone says where the voice
/// is published (see `voice_url`), so this one table is what gets downloaded
/// and what `--list-voices` prints. English comes first: it is the fallback.
///
/// Japanese is absent on purpose, see `no_japanese`.
const VOICES: &[(&str, &str)] = &[
    ("en", "en_US-lessac-medium"),
    ("fr", "fr_FR-tom-medium"),
    ("es", "es_ES-davefx-medium"),
    ("de", "de_DE-thorsten-medium"),
    ("it", "it_IT-riccardo-x_low"),
    ("pt", "pt_BR-faber-medium"),
    ("zh", "zh_CN-huayan-medium"),
    ("ko", "ko_KR-kss-medium"),
    ("ru", "ru_RU-irina-medium"),
    ("ar", "ar_JO-kareem-medium"),
    ("nl", "nl_NL-mls-medium"),
];

const VOICES_REPO: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main";
const QUALITIES: &[&str] = &["x_low", "low", "medium", "high"];

/// The only Japanese voice of rhasspy/piper-voices, ja_JP-hi_fi_captain-medium,
/// was trained on its own Japanese phonemes (`"phoneme_type": "japanese"`), not
/// on the espeak ones piper-rs produces: fed those, it does not say the
/// sentence. Refuse before downloading 77 MB of it, and name a way out.
fn no_japanese() -> anyhow::Error {
    let say = if cfg!(target_os = "macos") {
        ", or `-b say -v Kyoko`"
    } else {
        ""
    };
    anyhow::anyhow!(
        "piper cannot speak Japanese: its only Japanese voice needs a phonemizer vox does not have. \
         Use `-b qwen-native`{say} (to keep it: `vox config set backend qwen-native`)"
    )
}

/// The default voice of a language. A code piper has no voice for gets the
/// English one, as `-l` is not validated on every path that reaches here.
fn default_voice(lang: &str) -> Result<&'static str> {
    if lang == "ja" {
        return Err(no_japanese());
    }
    Ok(VOICES
        .iter()
        .find(|(code, _)| *code == lang)
        .map_or(VOICES[0].1, |(_, voice)| voice))
}

/// Directory a voice's files are published in.
///
/// A piper voice name is `<lang>_<REGION>-<name>-<quality>` and the repository
/// is laid out by those same parts, so the name is enough to find the files.
/// It also ends up in a file name, hence the strict character check.
fn voice_url(voice: &str) -> Result<String> {
    let mut parts = voice.split('-');
    let parsed = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(locale), Some(name), Some(quality), None) => locale
            .split_once('_')
            .map(|(lang, region)| (lang, region, locale, name, quality)),
        _ => None,
    };
    let well_formed = parsed.filter(|(lang, region, _, name, quality)| {
        (2..=3).contains(&lang.len())
            && lang.chars().all(|c| c.is_ascii_lowercase())
            && region.len() == 2
            && region.chars().all(|c| c.is_ascii_uppercase())
            && !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && QUALITIES.contains(quality)
    });
    let Some((lang, _, locale, name, quality)) = well_formed else {
        anyhow::bail!(
            "'{voice}' is not a piper voice name \
             (expected <lang>_<REGION>-<name>-<quality>, like fr_FR-siwis-medium)"
        );
    };
    Ok(format!("{VOICES_REPO}/{lang}/{locale}/{name}/{quality}"))
}

/// The voice to load: one named with `-v` (or stored as the voice preference)
/// wins over the language's default.
fn resolve_voice(lang: &str, requested: Option<&str>) -> Result<String> {
    if let Some(name) = requested {
        match voice_url(name) {
            Ok(_) if name.starts_with("ja_") => return Err(no_japanese()),
            Ok(_) => return Ok(name.to_string()),
            // Not fatal: the voice preference is shared by every backend, so a
            // pocket or say voice legitimately arrives here. Say it is unused.
            Err(e) => eprintln!("piper: {e}; using the default voice for '{lang}'"),
        }
    }
    default_voice(lang).map(str::to_string)
}

/// Timeout for a single model download. Without one a dead connection hangs
/// the CLI forever with no output, which is what users actually reported.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// What to suggest when the download cannot even start. `say` is the one
/// backend that never downloads anything, and it exists on macOS only.
fn offline_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "no network? `vox -b say` needs no download"
    } else {
        "no network? a piper voice is downloaded once, then works offline"
    }
}

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
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to download {what} ({})", offline_hint()))?;
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

/// Turn a published voice config into one piper-rs loads, or refuse the voice.
/// Returns `None` when the config is fine as it is.
///
/// piper-rs only produces espeak phonemes. A voice trained on another
/// phonemizer (`"phoneme_type"`: pinyin, japanese, text...) would load, then
/// fail inside espeak or say something else than the sentence. Older configs
/// have no `phoneme_type`; they are espeak ones too.
///
/// piper-rs also rejects two things that espeak voices do publish:
/// - keys of `phoneme_id_map` longer than one character. Voices trained with
///   a recent piper (ko_KR-kss-medium) carry a few diphthong keys such as
///   "aɪ"; piper-rs looks phonemes up one character at a time, so those keys
///   could never match and dropping them loses nothing.
/// - a missing `speaker_id_map`, which some single-speaker voices
///   (nl_NL-pim-medium) leave out. An empty one says the same.
fn repair_config(config: &[u8]) -> Result<Option<Vec<u8>>> {
    let mut json: serde_json::Value =
        serde_json::from_slice(config).context("its config is not valid JSON")?;
    let phonemizer = json.get("phoneme_type").and_then(|t| t.as_str());
    if let Some(other) = phonemizer.filter(|t| *t != "espeak") {
        anyhow::bail!(
            "it is phonemized by '{other}', and vox only has espeak: \
             pick a voice whose config says \"phoneme_type\": \"espeak\""
        );
    }
    let Some(root) = json.as_object_mut() else {
        return Ok(None);
    };
    let mut changed = false;
    if let Some(map) = root
        .get_mut("phoneme_id_map")
        .and_then(|m| m.as_object_mut())
    {
        let before = map.len();
        map.retain(|key, _| key.chars().count() == 1);
        changed |= map.len() != before;
    }
    if root.get("speaker_id_map").is_none_or(|m| m.is_null()) {
        root.insert("speaker_id_map".into(), serde_json::json!({}));
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(serde_json::to_vec(&json)?))
}

/// Ensure a voice's files exist, downloading if needed. Returns (onnx_path, json_path).
fn ensure_model(voice: &str) -> Result<(PathBuf, PathBuf)> {
    let base_url = voice_url(voice)?;
    let dir = models_dir();
    std::fs::create_dir_all(&dir).ok();

    let onnx_path = dir.join(format!("{voice}.onnx"));
    let json_path = dir.join(format!("{voice}.onnx.json"));
    let fetch = |file: &str| {
        download(&format!("{base_url}/{file}?download=true"), file).with_context(|| {
            format!("could not get piper voice '{voice}' (the voices that exist: https://huggingface.co/rhasspy/piper-voices)")
        })
    };

    // The config first: it is a few kB and says whether piper-rs can use the
    // voice at all, before 60 MB of model are fetched for nothing.
    if !json_path.exists() {
        let bytes = fetch(&format!("{voice}.onnx.json"))?;
        write_atomically(&json_path, &bytes)?;
    }

    // Done on every load, not at download: a config fetched by an older vox is
    // already on disk, and the `exists()` guard would keep it broken forever.
    // One flat message: MCP shows only the outermost one.
    let config = std::fs::read(&json_path)
        .with_context(|| format!("failed to read {}", json_path.display()))?;
    let repaired = repair_config(&config).map_err(|e| {
        anyhow::anyhow!(
            "piper voice '{voice}' cannot be used: {e:#} ({})",
            json_path.display()
        )
    })?;
    if let Some(repaired) = repaired {
        write_atomically(&json_path, &repaired)?;
    }

    if !onnx_path.exists() {
        eprintln!(
            "Downloading piper voice '{voice}' (~60 MB, once) to {}...",
            dir.display()
        );
        let bytes = fetch(&format!("{voice}.onnx"))?;
        write_atomically(&onnx_path, &bytes)?;
        eprintln!(
            "Downloaded {} ({:.1} MB)",
            voice,
            bytes.len() as f64 / 1_000_000.0
        );
    }

    Ok((onnx_path, json_path))
}

fn get_or_load_model(
    voice: &str,
) -> Result<std::sync::MutexGuard<'static, Option<(String, Piper)>>> {
    ensure_espeak_data()?;

    let mut guard = MODEL
        .lock()
        .map_err(|e| anyhow::anyhow!("model lock poisoned: {e}"))?;

    // Reload if the voice changed
    let need_reload = match &*guard {
        Some((cached_voice, _)) => cached_voice != voice,
        None => true,
    };

    crate::timing::mark("piper: espeak data ready");
    if need_reload {
        let (onnx_path, json_path) = ensure_model(voice)?;
        let model = Piper::new(&onnx_path, &json_path)
            .map_err(|e| anyhow::anyhow!("failed to load piper model: {e}"))?;
        *guard = Some((voice.to_string(), model));
        crate::timing::mark("piper: model loaded");
    }

    Ok(guard)
}

impl TtsBackend for PiperBackend {
    fn name(&self) -> &str {
        "piper"
    }

    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()> {
        let lang = opts.lang.as_deref().unwrap_or("en");
        // Before the device opens: a language piper cannot speak fails at once.
        let voice = resolve_voice(lang, opts.voice.as_deref())?;

        // Opened before the model loads, so the two waits overlap.
        let player = opts
            .output
            .is_none()
            .then(|| crate::audio::Player::start(opts.volume));

        let mut guard = get_or_load_model(&voice)?;
        let (_, model) = guard.as_mut().context("model not loaded")?;

        let (audio_data, sample_rate) = model
            .create(text, false, None, None, None, None)
            .map_err(|e| anyhow::anyhow!("Piper TTS failed: {e}"))?;

        crate::timing::mark("piper: audio synthesized");
        if audio_data.is_empty() {
            return Ok(());
        }

        // Speaking needs no file: the samples go straight to the device.
        if let Some(player) = player {
            player.push(sample_rate, audio_data);
            return player.finish();
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
        crate::timing::mark("piper: wav written");

        // Saving goes through the same delivery point as every other backend.
        crate::audio::apply_wav_gain(&wav_path, opts.volume)?;
        crate::audio::deliver(&wav_path, opts.output.as_deref())?;

        let _ = std::fs::remove_file(&wav_path);
        Ok(())
    }

    fn list_voices(&self) -> Result<Vec<String>> {
        Ok(VOICES.iter().map(|(_, voice)| voice.to_string()).collect())
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
    fn every_supported_language_has_a_voice_or_says_why_not() {
        for lang in crate::config::SUPPORTED_LANGS {
            match default_voice(lang) {
                Ok(voice) => {
                    let url = voice_url(voice).unwrap();
                    // The voice of a language lives under that language.
                    assert!(
                        url.starts_with(&format!("{VOICES_REPO}/{lang}/{lang}_")),
                        "{lang} maps to {url}"
                    );
                    if *lang != "en" {
                        assert_ne!(voice, VOICES[0].1, "{lang} fell back to English");
                    }
                }
                Err(e) => {
                    assert_eq!(*lang, "ja", "{lang} has no voice: {e}");
                    // `ja` used to map to ja_JP-kokoro-medium, a 404: the failure
                    // must come before any download and name a backend that works.
                    assert!(e.to_string().contains("-b qwen-native"), "{e}");
                }
            }
        }
    }

    #[test]
    fn list_voices_is_what_gets_loaded() {
        // The list used to be a second, hand-kept copy: it said fr_FR-siwis
        // while fr_FR-tom was loaded, and kept a Japanese voice that is a 404.
        let listed = PiperBackend.list_voices().unwrap();
        let loaded: Vec<String> = crate::config::SUPPORTED_LANGS
            .iter()
            .filter_map(|lang| default_voice(lang).ok())
            .map(str::to_string)
            .collect();
        assert_eq!(listed, loaded);
        assert!(listed.contains(&"fr_FR-tom-medium".to_string()));
        assert!(!listed.iter().any(|v| v.starts_with("ja_")));
    }

    #[test]
    fn voice_url_is_built_from_the_name() {
        assert_eq!(
            voice_url("fr_FR-siwis-medium").unwrap(),
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/fr/fr_FR/siwis/medium"
        );
        // Underscores belong to the name and to the quality, dashes separate.
        assert_eq!(
            voice_url("en_US-libritts_r-medium").unwrap(),
            format!("{VOICES_REPO}/en/en_US/libritts_r/medium")
        );
        assert_eq!(
            voice_url("it_IT-riccardo-x_low").unwrap(),
            format!("{VOICES_REPO}/it/it_IT/riccardo/x_low")
        );
    }

    #[test]
    fn voice_url_rejects_a_malformed_name() {
        for bad in [
            "",
            "siwis",
            "alba",
            "af_heart",
            "fr_FR-siwis",
            "fr_FR-siwis-medium-extra",
            "fr-siwis-medium",
            "FR_fr-siwis-medium",
            "fr_FR--medium",
            "fr_FR-siwis-best",
            "fr_FR-../../etc-medium",
            "fr_FR-si/wis-medium",
            "fr_FR-siwis.onnx-medium",
        ] {
            let err = voice_url(bad).unwrap_err().to_string();
            assert!(err.contains("not a piper voice name"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn a_named_voice_wins_over_the_language() {
        assert_eq!(
            resolve_voice("fr", Some("fr_FR-siwis-medium")).unwrap(),
            "fr_FR-siwis-medium"
        );
        assert_eq!(resolve_voice("fr", None).unwrap(), "fr_FR-tom-medium");
        // Another backend's voice (the preference is shared) is not an error.
        assert_eq!(
            resolve_voice("fr", Some("alba")).unwrap(),
            "fr_FR-tom-medium"
        );
        // A real Japanese voice name is refused like `-l ja`, not downloaded.
        for (lang, voice) in [
            ("ja", None),
            ("fr", Some("ja_JP-hi_fi_captain-medium")),
            ("ja", Some("alba")),
        ] {
            let err = resolve_voice(lang, voice).unwrap_err().to_string();
            assert!(err.contains("cannot speak Japanese"), "{err}");
        }
    }

    #[test]
    fn offline_hint_names_say_only_where_it_exists() {
        assert_eq!(
            offline_hint().contains("-b say"),
            cfg!(target_os = "macos"),
            "{}",
            offline_hint()
        );
        assert_eq!(
            no_japanese().to_string().contains("-b say"),
            cfg!(target_os = "macos")
        );
    }

    /// The shape of a published config, cut down to what piper-rs reads.
    fn config_with_phonemes(keys: &[&str]) -> Vec<u8> {
        let map: serde_json::Map<String, serde_json::Value> = keys
            .iter()
            .enumerate()
            .map(|(id, key)| (key.to_string(), serde_json::json!([id])))
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "audio": { "sample_rate": 22050 },
            "espeak": { "voice": "ko" },
            "inference": { "noise_scale": 0.667, "length_scale": 1.0, "noise_w": 0.8 },
            "num_speakers": 1,
            "speaker_id_map": {},
            "phoneme_id_map": map,
        }))
        .unwrap()
    }

    #[test]
    fn a_config_with_diphthong_keys_is_repaired_for_piper_rs() {
        // ko_KR-kss-medium ships these keys; piper-rs then refused the config
        // with `invalid value: string "aɪ", expected a character`.
        let published = config_with_phonemes(&["_", "^", "$", "a", "ɪ", "aɪ", "oʊ"]);
        let repaired = repair_config(&published)
            .unwrap()
            .expect("the diphthong keys must be dropped");
        let config: piper_rs::ModelConfig =
            serde_json::from_slice(&repaired).expect("piper-rs must accept the repaired config");
        let mut kept: Vec<char> = config.phoneme_id_map.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, ['$', '^', '_', 'a', 'ɪ']);
        assert_eq!(config.phoneme_id_map[&'a'], [3]);
    }

    #[test]
    fn a_config_without_diphthong_keys_is_left_alone() {
        let published = config_with_phonemes(&["_", "^", "$", "a", "ɪ"]);
        assert!(repair_config(&published).unwrap().is_none());
        assert!(repair_config(b"<html>429</html>").is_err());
    }

    #[test]
    fn a_config_without_a_speaker_map_is_repaired_for_piper_rs() {
        // nl_NL-pim-medium and ten other voices publish no `speaker_id_map`;
        // piper-rs then refused the config with `missing field`, after the
        // 60 MB download and on every run.
        let mut json: serde_json::Value =
            serde_json::from_slice(&config_with_phonemes(&["_", "^", "$", "a"])).unwrap();
        json.as_object_mut().unwrap().remove("speaker_id_map");
        let published = serde_json::to_vec(&json).unwrap();
        assert!(serde_json::from_slice::<piper_rs::ModelConfig>(&published).is_err());
        let repaired = repair_config(&published)
            .unwrap()
            .expect("the speaker map must be added");
        let config: piper_rs::ModelConfig =
            serde_json::from_slice(&repaired).expect("piper-rs must accept the repaired config");
        assert!(config.speaker_id_map.is_empty());
        assert_eq!(config.phoneme_id_map.len(), 4);
    }

    #[test]
    fn a_voice_with_another_phonemizer_is_refused() {
        // zh_CN-chaowen-medium is a pinyin voice: its multi-character keys are
        // its phonemes. It used to be downloaded, stripped of them like a
        // diphthong, then fail inside espeak.
        let mut json: serde_json::Value =
            serde_json::from_slice(&config_with_phonemes(&["_", "^", "$", "a", "zh", "ang"]))
                .unwrap();
        json["phoneme_type"] = serde_json::json!("pinyin");
        let err = repair_config(&serde_json::to_vec(&json).unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains("phonemized by 'pinyin'"), "{err}");

        // Said to be espeak, or not said at all (older configs): accepted.
        json["phoneme_type"] = serde_json::json!("espeak");
        assert!(
            repair_config(&serde_json::to_vec(&json).unwrap())
                .unwrap()
                .is_some()
        );
    }

    /// Asks Hugging Face about every voice in the table: both files must be
    /// served, and the config must be one piper-rs loads. A 200 alone is not
    /// enough: the Japanese voice that does exist answers 200 and cannot speak.
    #[test]
    #[ignore = "needs the network"]
    fn every_mapped_voice_is_published_and_loadable() {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        let mut problems = Vec::new();
        for (lang, voice) in VOICES {
            let seen = problems.len();
            let base = voice_url(voice).unwrap();
            for file in [format!("{voice}.onnx"), format!("{voice}.onnx.json")] {
                let url = format!("{base}/{file}");
                match client.head(&url).send() {
                    Ok(r) if r.status() == reqwest::StatusCode::OK => {}
                    Ok(r) => problems.push(format!("{lang}: HEAD {url} -> {}", r.status())),
                    Err(e) => problems.push(format!("{lang}: HEAD {url} -> {e}")),
                }
            }
            let config = client
                .get(format!("{base}/{voice}.onnx.json"))
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.bytes());
            let Ok(config) = config else {
                problems.push(format!("{lang}: cannot read the config of {voice}"));
                continue;
            };
            match repair_config(&config) {
                Err(e) => problems.push(format!("{lang}: {voice}: {e:#}")),
                Ok(repaired) => {
                    let loadable = repaired.as_deref().unwrap_or(&config);
                    if let Err(e) = serde_json::from_slice::<piper_rs::ModelConfig>(loadable) {
                        problems.push(format!(
                            "{lang}: piper-rs refuses the config of {voice}: {e}"
                        ));
                    }
                }
            }
            if problems.len() == seen {
                eprintln!("{lang}: {voice} ok");
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
