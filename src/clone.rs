//! Voice cloning — audio validation, microphone recording, clone resolution.

use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use crate::config;
use crate::db;

const VALID_AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "m4a"];

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

pub fn resolve_voice(conn: &Connection, voice_name: &str) -> Result<Option<db::VoiceClone>> {
    db::get_clone(conn, voice_name)
}

pub fn record_clone(name: &str, duration: u32) -> Result<String> {
    let dir = config::clones_dir();
    std::fs::create_dir_all(&dir).context("Failed to create clones directory")?;
    let output_path = dir.join(format!("{name}.wav"));
    let output_str = output_path.to_string_lossy().to_string();

    eprintln!("Recording {duration}s of audio... Speak now!");
    let (samples, rate) =
        crate::mic::record_native(&crate::mic::RecordOptions::for_duration(duration as f64))?;
    if samples.is_empty() {
        bail!("Recording failed: no audio captured");
    }
    crate::mic::write_wav(&output_path, &samples, rate)?;
    eprintln!("Recording saved to {output_str}");
    Ok(output_str)
}
