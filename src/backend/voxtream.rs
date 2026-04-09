//! VoXtream2 TTS backend — zero-shot streaming TTS with dynamic speaking rate control.
//!
//! 0.5B param model, 74ms first-packet latency, 4x real-time on consumer GPU.
//! Supports zero-shot voice cloning via audio prompt (3-10s).
//! Requires: `pip install "voxtream>=0.2"` and `espeak-ng`.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use super::{SpeakOptions, TtsBackend};
use crate::audio;
use crate::config;

/// Default prompt audio for voxtream when no voice clone is provided.
/// Generated on first use via macOS `say` or a bundled fallback.
/// Stored in /tmp to avoid paths with spaces (torchaudio PosixPath bug).
pub fn default_prompt_audio() -> Result<PathBuf> {
    let path = PathBuf::from("/tmp/vox_voxtream_default_prompt.wav");
    if path.exists() {
        return Ok(path);
    }

    // Try generating with macOS say
    #[cfg(target_os = "macos")]
    {
        std::fs::create_dir_all(config::config_dir()).ok();
        let status = Command::new("/usr/bin/say")
            .arg("-v")
            .arg("Samantha")
            .arg("-o")
            .arg(&*path.to_string_lossy())
            .arg("--data-format=LEI16@16000")
            .arg("Hello, my name is Samantha. I am testing voice synthesis today.")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Ok(s) = status {
            if s.success() && path.exists() {
                return Ok(path);
            }
        }
    }

    anyhow::bail!(
        "VoXtream2 requires a prompt audio file (3-10s). Provide one via voice clone:\n\
         vox clone add myvoice --audio ~/voice.wav\n\
         vox -b voxtream -v myvoice \"text\""
    )
}

pub struct VoxtreamBackend;

/// Find the voxtream binary — check PATH first, then common venv locations.
pub fn find_voxtream() -> Option<PathBuf> {
    // Check PATH
    if let Ok(status) = Command::new("voxtream")
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        if status.success() {
            return Some(PathBuf::from("voxtream"));
        }
    }

    // Check common venv locations
    let candidates = [
        dirs::home_dir().map(|h| h.join(".local/venvs/voxtream/bin/voxtream")),
        dirs::home_dir().map(|h| h.join(".venvs/voxtream/bin/voxtream")),
        dirs::home_dir().map(|h| h.join("venvs/voxtream/bin/voxtream")),
    ];

    candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.exists())
}

impl TtsBackend for VoxtreamBackend {
    fn name(&self) -> &str {
        "voxtream"
    }

    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()> {
        let bin = find_voxtream().context(
            "voxtream not found. Install it:\n\
             python3.11 -m venv ~/.local/venvs/voxtream\n\
             ~/.local/venvs/voxtream/bin/pip install \"voxtream>=0.2\"\n\
             brew install espeak-ng",
        )?;

        let tmp = tempfile::NamedTempFile::new().context("failed to create temp file")?;
        let wav_path = tmp.path().with_extension("wav");
        let wav_str = wav_path.to_string_lossy().to_string();

        // Config files are stored in ~/.config/vox/voxtream/
        let config_dir = config::config_dir().join("voxtream");
        let generator_config = config_dir.join("generator.json");
        let rate_config = config_dir.join("speaking_rate.json");
        if !generator_config.exists() {
            anyhow::bail!(
                "VoXtream2 config not found at {}. Clone the repo configs:\n\
                 git clone --depth 1 https://github.com/herimor/voxtream.git /tmp/voxtream-repo\n\
                 mkdir -p ~/.config/vox/voxtream\n\
                 cp /tmp/voxtream-repo/configs/*.json ~/.config/vox/voxtream/",
                generator_config.display()
            );
        }

        eprintln!("Loading VoXtream2 model (first run may download ~500MB)...");

        let mut cmd = Command::new(&bin);
        cmd.arg("-t").arg(text);
        cmd.arg("-o").arg(&wav_str);
        cmd.arg("-c").arg(&generator_config);

        // Prompt audio is required — use ref_audio (clone) or generate a default
        let prompt_path = match opts.ref_audio {
            Some(ref path) => PathBuf::from(path),
            None => default_prompt_audio()?,
        };
        cmd.arg("-pa").arg(&prompt_path);

        // Always pass speaking rate config (voxtream requires it)
        cmd.arg("--spk-rate-config").arg(&rate_config);

        // Speaking rate (syllables per second)
        if let Some(rate) = opts.rate {
            cmd.arg("-fs");
            cmd.arg("--spk-rate").arg(format!("{}.0", rate));
        }

        let output = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .context("failed to execute voxtream")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("VoXtream2 TTS failed: {stderr}");
        }

        audio::apply_wav_gain(&wav_path, opts.volume)?;
        audio::play_wav_blocking(&wav_path)?;
        let _ = std::fs::remove_file(&wav_path);
        Ok(())
    }

    fn list_voices(&self) -> Result<Vec<String>> {
        // VoXtream2 is zero-shot — any audio prompt works as a "voice"
        Ok(vec![
            "(zero-shot: use --voice with a clone name, or provide any audio prompt)".into(),
        ])
    }

    fn is_available(&self) -> bool {
        find_voxtream().is_some()
    }
}
