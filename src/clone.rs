//! Voice cloning — reference import, microphone recording, clone resolution,
//! and the rule that picks the backend a clone is spoken with.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use crate::config;
use crate::db;

/// The formats the bundled rodio decoders read. `.m4a` is not one of them:
/// AAC needs symphonia's `aac` and `isomp4` features, which this build leaves
/// out, so accepting the name would only move the failure to synthesis.
const VALID_AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg"];

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

/// Where a clone's reference is kept, as an absolute path.
///
/// The name becomes a file name, and over MCP an agent chooses it, so it has
/// to be a single path component: anything else could write outside the
/// clones directory.
fn reference_path(name: &str) -> Result<PathBuf> {
    let mut parts = Path::new(name).components();
    let is_file_name = matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(part)), None) if part == std::ffi::OsStr::new(name)
    );
    if !is_file_name || name.contains('\0') {
        bail!("Invalid clone name: {name:?}. Use a plain name, not a path.");
    }
    std::path::absolute(config::clones_dir().join(format!("{name}.wav")))
        .context("Failed to resolve the clones directory")
}

/// Where the reference of a new clone will be kept, once its name is known
/// to be free.
///
/// Checked before anything is written or recorded: the reference is stored
/// under the name, so writing first would replace the reference of the clone
/// that already owns it. Names are compared without case, because `Me.wav`
/// and `me.wav` are one file on a case-insensitive file system, the default
/// on macOS and Windows.
pub fn new_reference_path(conn: &Connection, name: &str) -> Result<PathBuf> {
    let dest = reference_path(name)?;
    let folded = name.to_lowercase();
    if let Some(taken) = db::list_clones(conn)?
        .into_iter()
        .find(|existing| existing.name.to_lowercase() == folded)
    {
        bail!(
            "Voice clone '{0}' already exists. Remove it first: vox clone remove {0}",
            taken.name
        );
    }
    Ok(dest)
}

/// Register a voice clone from an audio file, and return where its reference
/// was stored.
///
/// qwen-native and pocket both read the reference with a WAV reader, and
/// pocket only recognises a clone by its `.wav` suffix. So whatever format was
/// given is decoded once, here, and kept as a mono WAV in the clones
/// directory: the clone then depends neither on the original file nor on the
/// directory the command was run from.
/// Removes a clone, and the reference vox keeps for it.
///
/// `clone add` and `clone record` put that reference in the clones directory;
/// a row deleted alone left the recording of a voice on disk for good. A
/// reference stored anywhere else belongs to the user (clones added by older
/// versions point at the original file) and is left where it is. Returns
/// false when no clone has that name.
pub fn remove_clone(conn: &Connection, name: &str) -> Result<bool> {
    let Some(clone) = db::get_clone(conn, name)? else {
        return Ok(false);
    };
    if !db::remove_clone(conn, name)? {
        return Ok(false);
    }
    let reference = Path::new(&clone.ref_audio);
    if reference.parent() == Some(crate::config::clones_dir().as_path()) {
        // Already gone is fine; anything else is worth saying, not failing on:
        // the clone itself is removed.
        if let Err(e) = std::fs::remove_file(reference)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("Could not delete {}: {e}", reference.display());
        }
    }
    Ok(true)
}

pub fn add_clone_from_file(
    conn: &Connection,
    name: &str,
    audio: &str,
    ref_text: Option<&str>,
) -> Result<String> {
    validate_audio(audio)?;
    let dest = new_reference_path(conn, name)?;

    let (samples, rate) = crate::levels::decode_mono(Path::new(audio))
        .filter(|(samples, rate)| !samples.is_empty() && *rate > 0)
        .with_context(|| format!("Could not decode {audio}: no audio could be read from it"))?;

    let dir = dest.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).context("Failed to create clones directory")?;
    // Written beside the destination and renamed over it, so a failed write
    // leaves nothing behind, and a source that already is the destination
    // (re-adding a recording after `clone remove`) is not truncated mid-read.
    let staged = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile_in(dir)
        .context("Failed to create the reference file")?
        .into_temp_path();
    crate::mic::write_wav(&staged, &samples, rate)?;
    staged
        .persist(&dest)
        .with_context(|| format!("Failed to write {}", dest.display()))?;

    let stored = dest.to_string_lossy().to_string();
    db::add_clone(conn, name, &stored, ref_text)?;
    Ok(stored)
}

pub fn resolve_voice(conn: &Connection, voice_name: &str) -> Result<Option<db::VoiceClone>> {
    db::get_clone(conn, voice_name)
}

pub fn record_clone(name: &str, duration: u32) -> Result<String> {
    let dir = config::clones_dir();
    std::fs::create_dir_all(&dir).context("Failed to create clones directory")?;
    let output_path = dir.join(format!("{name}.wav"));
    let output_str = output_path.to_string_lossy().to_string();

    eprintln!("Recording {duration}s of audio.");
    eprintln!("A beep means start speaking; a second, lower beep means stop.");
    let (samples, rate) =
        crate::mic::record_native(&crate::mic::RecordOptions::for_duration(duration as f64))?;
    if samples.is_empty() {
        bail!("Recording failed: no audio captured");
    }
    crate::mic::write_wav(&output_path, &samples, rate)?;
    eprintln!("Recording saved to {output_str}");
    Ok(output_str)
}

/// Whether `backend` can speak with a cloned voice at all.
pub fn can_clone(backend: &str) -> bool {
    matches!(backend, "qwen-native" | "pocket")
}

/// Whether pocket can clone in this process: only its gated weights encode a
/// reference, and pocket loads them only when HF_TOKEN is set.
pub fn pocket_can_clone() -> bool {
    std::env::var("HF_TOKEN").is_ok_and(|t| !t.is_empty())
}

/// The backend to speak with: the one rule the command line and the MCP
/// server share.
///
/// A backend named for this call (`-b`, the MCP `backend` argument) is taken
/// as given, even when it names the default and even when it cannot clone.
/// Otherwise the stored preference applies, then the default for the
/// language; and a voice clone moves that choice to a backend that can use
/// it: pocket stays when it can clone (`pocket_can_clone`), anything else
/// becomes qwen-native, which clones with public weights.
pub fn speak_backend(
    explicit: Option<&str>,
    stored: Option<&str>,
    lang: Option<&str>,
    is_clone: bool,
    pocket_can_clone: bool,
) -> String {
    if let Some(backend) = explicit {
        return backend.to_string();
    }
    let chosen = stored.unwrap_or_else(|| config::default_backend_for_lang(lang));
    let keeps_clone = chosen == "qwen-native" || (chosen == "pocket" && pocket_can_clone);
    if is_clone && !keeps_clone {
        return "qwen-native".to_string();
    }
    chosen.to_string()
}
