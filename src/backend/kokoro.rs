//! Kokoro TTS backend — pure Rust inference via ONNX Runtime (kokoro-tts crate).
//!
//! 82M param model, cross-platform (CPU + CUDA). No Python dependency.
//! Model files (~80MB) downloaded separately from GitHub releases.
//! Sample rate: 24kHz.

use std::sync::OnceLock;

use anyhow::{Context, Result};
use kokoro_tts::{KokoroTts, Voice};
use tokio::sync::Mutex;

use super::{SpeakOptions, TtsBackend};
use crate::config;

const SAMPLE_RATE: u32 = 24_000;

pub struct KokoroBackend;

/// Global model instance — loaded once, stays warm.
static MODEL: OnceLock<Mutex<KokoroTts>> = OnceLock::new();

fn kokoro_dir() -> std::path::PathBuf {
    let subdir = config::model_config_str("kokoro", "subdir").unwrap_or_else(|| "kokoro".into());
    config::config_dir().join(subdir)
}

fn model_file() -> String {
    config::model_config_str("kokoro", "model_file").unwrap_or_else(|| "kokoro-v1.0.onnx".into())
}

fn voices_file() -> String {
    config::model_config_str("kokoro", "voices_file").unwrap_or_else(|| "voices.bin".into())
}

fn model_path() -> std::path::PathBuf {
    kokoro_dir().join(model_file())
}

fn voices_path() -> std::path::PathBuf {
    kokoro_dir().join(voices_file())
}

/// Run an async future on the current tokio runtime, or create one if needed.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(f))
    } else {
        tokio::runtime::Runtime::new()
            .expect("failed to create tokio runtime")
            .block_on(f)
    }
}

async fn get_or_init_model() -> Result<&'static Mutex<KokoroTts>> {
    if let Some(m) = MODEL.get() {
        return Ok(m);
    }
    let mp = model_path();
    let vp = voices_path();
    eprintln!("Loading Kokoro model from {}...", kokoro_dir().display());
    let tts = KokoroTts::new(&mp, &vp)
        .await
        .map_err(|e| anyhow::anyhow!("failed to load Kokoro model: {e}"))?;
    Ok(MODEL.get_or_init(|| Mutex::new(tts)))
}

/// Map a voice name string to the typed Voice enum.
fn map_voice(name: &str, speed: f32) -> Voice {
    match name {
        // English
        "af_heart" => Voice::AfHeart(speed),
        "af_bella" => Voice::AfBella(speed),
        "af_nova" => Voice::AfNova(speed),
        "af_sky" => Voice::AfSky(speed),
        "af_sarah" => Voice::AfSarah(speed),
        "af_nicole" => Voice::AfNicole(speed),
        "af_jessica" => Voice::AfJessica(speed),
        "af_river" => Voice::AfRiver(speed),
        "af_aoede" => Voice::AfAoede(speed),
        "af_kore" => Voice::AfKore(speed),
        "af_alloy" => Voice::AfAlloy(speed),
        "am_adam" => Voice::AmAdam(speed),
        "am_michael" => Voice::AmMichael(speed),
        "am_echo" => Voice::AmEcho(speed),
        "am_eric" => Voice::AmEric(speed),
        "am_liam" => Voice::AmLiam(speed),
        "am_onyx" => Voice::AmOnyx(speed),
        "am_puck" => Voice::AmPuck(speed),
        "am_fenrir" => Voice::AmFenrir(speed),
        "bf_emma" => Voice::BfEmma(speed),
        "bf_alice" => Voice::BfAlice(speed),
        "bf_isabella" => Voice::BfIsabella(speed),
        "bf_lily" => Voice::BfLily(speed),
        "bm_george" => Voice::BmGeorge(speed),
        "bm_daniel" => Voice::BmDaniel(speed),
        "bm_fable" => Voice::BmFable(speed),
        "bm_lewis" => Voice::BmLewis(speed),
        // French
        "ff_siwis" => Voice::FfSiwis(speed),
        // Japanese
        "jf_alpha" => Voice::JfAlpha(speed),
        "jf_nezumi" => Voice::JfNezumi(speed),
        "jf_tebukuro" => Voice::JfTebukuro(speed),
        "jf_gongitsune" => Voice::JfGongitsune(speed),
        "jm_kumo" => Voice::JmKumo(speed),
        // Chinese
        "zf_xiaoxiao" => Voice::ZfXiaoxiao(speed),
        "zf_xiaoni" => Voice::ZfXiaoni(speed),
        "zf_xiaobei" => Voice::ZfXiaobei(speed),
        "zf_xiaoyi" => Voice::ZfXiaoyi(speed),
        "zm_yunxi" => Voice::ZmYunxi(speed),
        "zm_yunyang" => Voice::ZmYunyang(speed),
        "zm_yunxia" => Voice::ZmYunxia(speed),
        "zm_yunjian" => Voice::ZmYunjian(speed),
        // Hindi
        "hf_alpha" => Voice::HfAlpha(speed),
        "hf_beta" => Voice::HfBeta(speed),
        "hm_psi" => Voice::HmPsi(speed),
        "hm_omega" => Voice::HmOmega(speed),
        // Italian
        "if_sara" => Voice::IfSara(speed),
        "im_nicola" => Voice::ImNicola(speed),
        // Portuguese
        "pf_dora" => Voice::PfDora(speed),
        "pm_alex" => Voice::PmAlex(speed),
        // Spanish
        "ef_dora" => Voice::EfDora(speed),
        "em_alex" => Voice::EmAlex(speed),
        _ => Voice::AfHeart(speed),
    }
}

/// Check if a voice name matches the kokoro pattern (e.g. af_heart, jm_kumo).
fn is_kokoro_voice(name: &str) -> bool {
    name.len() >= 3 && name.as_bytes()[2] == b'_'
}

/// Pick default voice based on language.
fn default_voice_for_lang(lang: &str) -> &'static str {
    match lang {
        "fr" => "ff_siwis",
        "ja" => "jf_alpha",
        "zh" => "zf_xiaoxiao",
        "hi" => "hf_alpha",
        "it" => "if_sara",
        "pt" => "pf_dora",
        "es" => "ef_dora",
        "de" => "bf_emma", // British English voice works for German-adjacent
        _ => "af_heart",
    }
}

impl TtsBackend for KokoroBackend {
    fn name(&self) -> &str {
        "kokoro"
    }

    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()> {
        let mp = model_path();
        let vp = voices_path();
        if !mp.exists() || !vp.exists() {
            let model_url = config::model_config_str("kokoro", "model_url").unwrap_or_else(|| {
                "https://github.com/mzdk100/kokoro/releases/download/V1.0/kokoro-v1.0.onnx".into()
            });
            let voices_url =
                config::model_config_str("kokoro", "voices_url").unwrap_or_else(|| {
                    "https://github.com/mzdk100/kokoro/releases/download/V1.0/voices.bin".into()
                });
            anyhow::bail!(
                "Kokoro model not found. Download model files to {}:\n\
                 mkdir -p {}\n\
                 curl -L -o {} {}\n\
                 curl -L -o {} {}",
                kokoro_dir().display(),
                kokoro_dir().display(),
                mp.display(),
                model_url,
                vp.display(),
                voices_url,
            );
        }

        let lang = opts.lang.as_deref().unwrap_or("en");
        let voice_name = opts
            .voice
            .as_deref()
            .filter(|v| is_kokoro_voice(v))
            .unwrap_or_else(|| default_voice_for_lang(lang));
        let speed = 1.0_f32;
        let voice = map_voice(voice_name, speed);

        let (samples, _duration) = block_on(async {
            let model = get_or_init_model().await?;
            let tts = model.lock().await;
            tts.synth(text, voice)
                .await
                .map_err(|e| anyhow::anyhow!("Kokoro synthesis failed: {e}"))
        })?;

        // Write samples to WAV and play
        let tmp = tempfile::NamedTempFile::new().context("failed to create temp file")?;
        let wav_path = tmp.path().with_extension("wav");
        write_wav(&wav_path, &samples, SAMPLE_RATE)?;

        crate::audio::apply_wav_gain(&wav_path, opts.volume)?;
        crate::audio::play_wav_blocking(&wav_path)?;
        let _ = std::fs::remove_file(&wav_path);
        Ok(())
    }

    fn list_voices(&self) -> Result<Vec<String>> {
        Ok(vec![
            // English
            "af_heart".into(),
            "af_bella".into(),
            "af_nova".into(),
            "af_sky".into(),
            "af_sarah".into(),
            "af_nicole".into(),
            "af_jessica".into(),
            "af_river".into(),
            "af_aoede".into(),
            "af_kore".into(),
            "af_alloy".into(),
            "am_adam".into(),
            "am_michael".into(),
            "am_echo".into(),
            "am_eric".into(),
            "am_liam".into(),
            "am_onyx".into(),
            "am_puck".into(),
            "am_fenrir".into(),
            "bf_emma".into(),
            "bf_alice".into(),
            "bf_isabella".into(),
            "bf_lily".into(),
            "bm_george".into(),
            "bm_daniel".into(),
            "bm_fable".into(),
            "bm_lewis".into(),
            // French
            "ff_siwis".into(),
            // Japanese
            "jf_alpha".into(),
            "jf_nezumi".into(),
            "jf_tebukuro".into(),
            "jf_gongitsune".into(),
            "jm_kumo".into(),
            // Chinese
            "zf_xiaoxiao".into(),
            "zf_xiaoni".into(),
            "zf_xiaobei".into(),
            "zf_xiaoyi".into(),
            "zm_yunxi".into(),
            "zm_yunyang".into(),
            "zm_yunxia".into(),
            "zm_yunjian".into(),
            // Hindi
            "hf_alpha".into(),
            "hf_beta".into(),
            "hm_psi".into(),
            "hm_omega".into(),
            // Italian
            "if_sara".into(),
            "im_nicola".into(),
            // Portuguese
            "pf_dora".into(),
            "pm_alex".into(),
            // Spanish
            "ef_dora".into(),
            "em_alex".into(),
        ])
    }

    fn is_available(&self) -> bool {
        model_path().exists() && voices_path().exists()
    }
}

/// Write f32 PCM samples to a WAV file.
fn write_wav(path: &std::path::Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).context("failed to create WAV file")?;
    for &s in samples {
        writer.write_sample(s).context("failed to write sample")?;
    }
    writer.finalize().context("failed to finalize WAV file")?;
    Ok(())
}
