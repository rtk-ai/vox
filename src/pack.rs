//! Sound packs — themed audio clips (peon-ping compatible).
//!
//! Packs are downloaded from the peon-ping GitHub repo and stored in `~/.config/vox/packs/`.
//! Each pack has a manifest.json with categories (greeting, complete, error, etc.).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::audio;
use crate::config;

const PACKS_REPO: &str = "https://raw.githubusercontent.com/tonyyont/peon-ping/main/packs";

const AVAILABLE_PACKS: &[&str] = &[
    "peon",
    "peon_fr",
    "peon_pl",
    "peasant",
    "peasant_fr",
    "sc_kerrigan",
    "sc_battlecruiser",
    "ra2_soviet_engineer",
];

/// Reject any name that is not a single, ordinary directory component.
///
/// Every pack operation builds a path as `packs_dir().join(name)`. `join` with
/// an absolute path discards the base entirely, and `..` is resolved by the OS,
/// so an unvalidated name escapes the packs directory completely. `install`
/// happened to be safe because it checks `AVAILABLE_PACKS` first; `remove`,
/// `load_manifest`, `play` and `pick_sound` did not, and `remove` calls
/// `fs::remove_dir_all` — an arbitrary recursive delete.
pub fn validate_pack_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Pack name cannot be empty");
    }
    if name == "." || name == ".." {
        anyhow::bail!("Invalid pack name: '{name}'");
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        anyhow::bail!("Invalid pack name '{name}': must not contain a path separator");
    }
    // Belt and braces: the component must survive a round trip through Path
    // as exactly one normal component. This also rejects Windows prefixes
    // such as `C:` and root components.
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(c)), None) if c == name => Ok(()),
        _ => anyhow::bail!("Invalid pack name '{name}': must be a single directory name"),
    }
}

/// Reject a manifest entry that is not a plain file name.
///
/// The sound file names come from a manifest fetched over the network, and are
/// joined onto the pack's sounds directory before being written.
fn validate_sound_file(file: &str) -> Result<()> {
    validate_pack_name(file).with_context(|| format!("Rejected sound file name '{file}'"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    pub name: String,
    pub display_name: String,
    pub categories: HashMap<String, Category>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub sounds: Vec<SoundEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundEntry {
    pub file: String,
    pub line: String,
}

/// List packs available for install from the remote repository.
pub fn list_available() -> &'static [&'static str] {
    AVAILABLE_PACKS
}

/// List installed packs by reading manifest.json from each pack directory.
pub fn list_installed() -> Result<Vec<PackManifest>> {
    let dir = config::packs_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut packs = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if manifest_path.exists() {
            let content = fs::read_to_string(&manifest_path)?;
            if let Ok(manifest) = serde_json::from_str::<PackManifest>(&content) {
                packs.push(manifest);
            }
        }
    }
    packs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packs)
}

/// Install a pack by downloading manifest + sound files from peon-ping repo.
pub fn install(name: &str) -> Result<()> {
    if !AVAILABLE_PACKS.contains(&name) {
        anyhow::bail!(
            "Unknown pack: {name}. Available: {}",
            AVAILABLE_PACKS.join(", ")
        );
    }

    let dest = config::packs_dir().join(name);
    if dest.exists() {
        anyhow::bail!("Pack '{name}' is already installed");
    }

    let manifest_url = format!("{PACKS_REPO}/{name}/manifest.json");
    let client = reqwest::blocking::Client::new();

    let manifest_resp = client
        .get(&manifest_url)
        .send()
        .context("Failed to download manifest")?;
    if !manifest_resp.status().is_success() {
        anyhow::bail!(
            "Failed to download manifest: HTTP {}",
            manifest_resp.status()
        );
    }
    let manifest_text = manifest_resp.text().context("Failed to read manifest")?;

    let manifest: PackManifest =
        serde_json::from_str(&manifest_text).context("Failed to parse manifest")?;

    // Collect unique sound files
    let mut files: HashSet<String> = HashSet::new();
    for cat in manifest.categories.values() {
        for sound in &cat.sounds {
            files.insert(sound.file.clone());
        }
    }

    // Create directories
    let sounds_dir = dest.join("sounds");
    fs::create_dir_all(&sounds_dir)?;

    // Save manifest
    fs::write(dest.join("manifest.json"), &manifest_text)?;

    // Download each sound file
    for file in &files {
        validate_sound_file(file)?;
        let url = format!("{PACKS_REPO}/{name}/sounds/{file}");
        let resp = client
            .get(&url)
            .send()
            .with_context(|| format!("Failed to download {file}"))?;
        if !resp.status().is_success() {
            // Clean up on failure
            let _ = fs::remove_dir_all(&dest);
            anyhow::bail!("Failed to download {file}: HTTP {}", resp.status());
        }
        let bytes = resp
            .bytes()
            .with_context(|| format!("Failed to read {file}"))?;
        fs::write(sounds_dir.join(file), &bytes)?;
        // (name validated above; `file` validated before the request)
    }

    Ok(())
}

/// Remove an installed pack.
pub fn remove(name: &str) -> Result<bool> {
    validate_pack_name(name)?;
    let dest = config::packs_dir().join(name);
    if !dest.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(&dest)?;
    Ok(true)
}

/// Load a pack's manifest.
pub fn load_manifest(name: &str) -> Result<PackManifest> {
    validate_pack_name(name)?;
    let manifest_path = config::packs_dir().join(name).join("manifest.json");
    if !manifest_path.exists() {
        anyhow::bail!("Pack '{name}' is not installed. Use: vox pack install {name}");
    }
    let content = fs::read_to_string(&manifest_path)?;
    let manifest: PackManifest = serde_json::from_str(&content)?;
    Ok(manifest)
}

/// Play a random sound from a category in the given pack.
/// Returns the voice line text of the sound played.
pub fn play(name: &str, category: Option<&str>) -> Result<String> {
    let manifest = load_manifest(name)?;

    let cat_name = category.unwrap_or("greeting");
    let cat = manifest
        .categories
        .get(cat_name)
        .with_context(|| format!("Category '{cat_name}' not found in pack '{name}'"))?;

    if cat.sounds.is_empty() {
        anyhow::bail!("No sounds in category '{cat_name}'");
    }

    // Pseudo-random selection using system time nanoseconds
    let idx = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize
        % cat.sounds.len();

    let sound = &cat.sounds[idx];
    let sound_path = config::packs_dir()
        .join(name)
        .join("sounds")
        .join(&sound.file);

    audio::play_audio_blocking(&sound_path)?;

    Ok(sound.line.clone())
}

/// Get the sound file path for a random sound (without playing it).
pub fn pick_sound(name: &str, category: Option<&str>) -> Result<(PathBuf, String)> {
    let manifest = load_manifest(name)?;

    let cat_name = category.unwrap_or("greeting");
    let cat = manifest
        .categories
        .get(cat_name)
        .with_context(|| format!("Category '{cat_name}' not found in pack '{name}'"))?;

    if cat.sounds.is_empty() {
        anyhow::bail!("No sounds in category '{cat_name}'");
    }

    let idx = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize
        % cat.sounds.len();

    let sound = &cat.sounds[idx];
    let sound_path = config::packs_dir()
        .join(name)
        .join("sounds")
        .join(&sound.file);

    Ok((sound_path, sound.line.clone()))
}
