use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use crate::config;
use crate::db;

const VALID_AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "m4a"];

pub fn sox_install_hint() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Failed to run rec (sox). Is sox installed? (brew install sox)"
    }
    #[cfg(target_os = "linux")]
    {
        "Failed to run rec (sox). Is sox installed? (sudo apt install sox)"
    }
    #[cfg(target_os = "windows")]
    {
        "Failed to run rec (sox). Is sox installed? (choco install sox)"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "Failed to run rec (sox). Is sox installed?"
    }
}

pub fn validate_audio(path: &str) -> Result<()> {
    let p = Path::new(path);
    if !p.exists() {
        bail!("Audio file not found: {path}");
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !VALID_AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        bail!(
            "Unsupported audio format: .{ext}. Supported: {}",
            VALID_AUDIO_EXTENSIONS.join(", ")
        );
    }
    Ok(())
}

pub fn build_record_command(output_path: &str, duration: u32) -> Command {
    let mut cmd = Command::new("rec");
    cmd.arg(output_path);
    cmd.arg("trim").arg("0").arg(duration.to_string());
    cmd
}

pub fn resolve_voice(conn: &Connection, voice_name: &str) -> Result<Option<db::VoiceClone>> {
    db::get_clone(conn, voice_name)
}

pub fn record_clone(name: &str, duration: u32) -> Result<String> {
    let dir = config::clones_dir();
    std::fs::create_dir_all(&dir).context("Failed to create clones directory")?;
    let output_path = dir.join(format!("{name}.wav"));
    let output_str = output_path.to_string_lossy().to_string();

    eprintln!("Recording {duration}s of audio... Speak now!");
    let status = build_record_command(&output_str, duration)
        .status()
        .context(sox_install_hint())?;
    if !status.success() {
        bail!("Recording failed with status {status}");
    }
    eprintln!("Recording saved to {output_str}");
    Ok(output_str)
}
