//! macOS `say` backend — system TTS via /usr/bin/say.
//!
//! Near-zero latency, uses Apple's built-in voices. No voice cloning support.

use std::process::Command;

use anyhow::{Context, Result};

use super::{SpeakOptions, TtsBackend};

pub struct SayBackend;

impl SayBackend {
    pub fn build_command(text: &str, opts: &SpeakOptions) -> Command {
        let mut cmd = Command::new("/usr/bin/say");
        if let Some(ref voice) = opts.voice {
            cmd.arg("-v").arg(voice);
        }
        if let Some(rate) = opts.rate {
            cmd.arg("-r").arg(rate.to_string());
        }
        if let Some(ref out) = opts.output {
            // say writes AIFF by default; ask for 16-bit PCM so a .wav name
            // holds real WAVE data instead of an AIFF with the wrong suffix.
            cmd.arg("-o").arg(out);
            if out
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
            {
                cmd.arg("--file-format=WAVE")
                    .arg("--data-format=LEI16@22050");
            }
        }
        cmd.arg(text);
        cmd
    }
}

impl TtsBackend for SayBackend {
    fn name(&self) -> &str {
        "say"
    }

    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()> {
        let mut cmd = Self::build_command(text, opts);
        let status = cmd.status().context("Failed to run /usr/bin/say")?;
        if !status.success() {
            anyhow::bail!("say exited with status {status}");
        }
        if let Some(ref out) = opts.output {
            eprintln!("Saved audio to {}", out.display());
        }
        Ok(())
    }

    fn list_voices(&self) -> Result<Vec<String>> {
        let output = Command::new("/usr/bin/say")
            .arg("-v")
            .arg("?")
            .output()
            .context("Failed to list voices")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let voices: Vec<String> = stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(String::from)
            .collect();
        Ok(voices)
    }

    fn is_available(&self) -> bool {
        std::path::Path::new("/usr/bin/say").exists()
    }
}
