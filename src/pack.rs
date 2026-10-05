//! Sound packs — themed audio clips (peon-ping compatible).
//!
//! Packs are found through the peon-ping registry, downloaded from the GitHub
//! repository it names, and stored in `~/.config/vox/packs/`.
//! Each pack has a manifest.json with categories (greeting, complete, error, etc.).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::audio;
use crate::config;

/// The index of every published pack. peon-ping packs no longer live in one
/// repository: each registry entry names the GitHub repository, the tag and the
/// directory that hold the pack's `openpeon.json` and its `sounds/`.
const REGISTRY_URL: &str = "https://peonping.github.io/registry/index.json";
const RAW_GITHUB: &str = "https://raw.githubusercontent.com";
const PACKS_SITE: &str = "https://openpeon.com/packs";

/// The registry index is half a megabyte: on a slow link it does not arrive
/// within the 30 seconds reqwest allows a request by default.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// The names vox has always used for the categories, against the names of the
/// CESP specification that `openpeon.json` uses (CESP v1.0, appendix B).
/// Installed manifests keep the vox names, so that `vox pack play greeting`
/// works the same on a pack installed by an older version and on a new one.
const CATEGORY_NAMES: &[(&str, &str)] = &[
    ("session.start", "greeting"),
    ("task.acknowledge", "acknowledge"),
    ("task.complete", "complete"),
    ("task.error", "error"),
    ("input.required", "permission"),
    ("resource.limit", "resource_limit"),
    ("user.spam", "annoyed"),
];

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
/// so an unvalidated name escapes the packs directory completely, and `remove`
/// calls `fs::remove_dir_all` — an arbitrary recursive delete.
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

/// The registry index, reduced to what resolving a name needs.
#[derive(Deserialize)]
struct Registry {
    packs: Vec<RegistryEntry>,
}

#[derive(Deserialize)]
struct RegistryEntry {
    name: String,
    #[serde(default)]
    source_repo: String,
    #[serde(default)]
    source_ref: String,
    #[serde(default)]
    source_path: String,
}

/// `openpeon.json`, reduced to what vox keeps of it.
#[derive(Deserialize)]
struct OpenPeonManifest {
    #[serde(default)]
    display_name: String,
    categories: HashMap<String, OpenPeonCategory>,
}

#[derive(Deserialize)]
struct OpenPeonCategory {
    #[serde(default)]
    sounds: Vec<OpenPeonSound>,
}

#[derive(Deserialize)]
struct OpenPeonSound {
    file: String,
    #[serde(default)]
    label: String,
}

/// True when a registry field can be put in a URL path as it is: the registry
/// is fetched over the network, and these fields choose what vox downloads.
fn is_url_path(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('/')
        && !s.contains("..")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

/// Find a pack in the registry index and return the URL of the directory that
/// holds its `openpeon.json`.
pub fn resolve_pack_url(registry_json: &str, name: &str) -> Result<String> {
    let registry: Registry =
        serde_json::from_str(registry_json).context("Failed to parse the pack registry")?;
    let Some(entry) = registry.packs.iter().find(|p| p.name == name) else {
        anyhow::bail!(
            "Unknown pack '{name}': the registry has {} packs and none has this name. \
             Browse them at {PACKS_SITE}",
            registry.packs.len()
        );
    };

    let (repo, git_ref) = (&entry.source_repo, &entry.source_ref);
    let is_repo = is_url_path(repo)
        && repo.split_once('/').is_some_and(|(owner, project)| {
            !owner.is_empty() && !project.is_empty() && !project.contains('/')
        });
    // A pack alone in its repository has "." or "" for a path.
    let path = match entry.source_path.trim_end_matches('/') {
        "." => "",
        path => path,
    };
    if !is_repo || !is_url_path(git_ref) || !(path.is_empty() || is_url_path(path)) {
        anyhow::bail!("The registry entry of pack '{name}' does not name a usable source");
    }

    let mut url = format!("{RAW_GITHUB}/{repo}/{git_ref}");
    if !path.is_empty() {
        url.push('/');
        url.push_str(path);
    }
    Ok(url)
}

/// Translate an `openpeon.json` manifest into the manifest vox stores.
///
/// Also returns the files to download: the name each one is stored under,
/// mapped to its path below the pack's `sounds/` directory in the repository.
pub fn from_openpeon(
    name: &str,
    openpeon_json: &str,
) -> Result<(PackManifest, HashMap<String, String>)> {
    // One published manifest (stellaris_evil_corp_advisor) starts with a byte
    // order mark, which serde_json does not take for JSON.
    let source: OpenPeonManifest =
        serde_json::from_str(openpeon_json.trim_start_matches('\u{feff}'))
            .with_context(|| format!("Failed to parse the manifest of pack '{name}'"))?;

    let mut categories: HashMap<String, Category> = HashMap::new();
    let mut files: HashMap<String, String> = HashMap::new();
    for (cesp_name, category) in source.categories {
        let vox_name = CATEGORY_NAMES
            .iter()
            .find(|(cesp, _)| *cesp == cesp_name)
            .map_or(cesp_name.as_str(), |(_, vox)| vox);
        let sounds = &mut categories
            .entry(vox_name.to_string())
            .or_insert_with(|| Category { sounds: Vec::new() })
            .sounds;

        for sound in category.sounds {
            // The specification wants "sounds/<file>", yet published packs
            // also give a bare file name, or a path with sub-directories.
            // The reference client reads all of them as a path below sounds/.
            let remote = match sound.file.strip_prefix("sounds/") {
                Some(below_sounds) => below_sounds,
                None => sound.file.rsplit('/').next().unwrap_or_default(),
            };
            if remote
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            {
                anyhow::bail!("Rejected sound file path '{}'", sound.file);
            }
            // vox keeps every sound of a pack in one directory.
            let file = remote.replace('/', "_");
            validate_sound_file(&file)?;
            if let Some(other) = files.insert(file.clone(), remote.to_string())
                && other != remote
            {
                anyhow::bail!(
                    "Sound files '{other}' and '{remote}' would both be stored as '{file}'"
                );
            }
            sounds.push(SoundEntry {
                file,
                line: sound.label,
            });
        }
    }
    if files.is_empty() {
        anyhow::bail!("Pack '{name}' has no sounds");
    }

    let manifest = PackManifest {
        // The directory is named after the registry entry, and some packs
        // call themselves something else in their own manifest.
        name: name.to_string(),
        display_name: if source.display_name.is_empty() {
            name.to_string()
        } else {
            source.display_name
        },
        categories,
    };
    Ok((manifest, files))
}

/// A few pack names to suggest for install. `install` accepts any pack of the
/// registry, which has several hundred.
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

fn download(client: &reqwest::blocking::Client, url: reqwest::Url) -> Result<Vec<u8>> {
    let resp = client.get(url).send()?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    Ok(resp.bytes()?.to_vec())
}

/// Install a pack: look it up in the registry, then download its manifest and
/// its sound files from the repository the registry names.
pub fn install(name: &str) -> Result<()> {
    // The MCP tool prints an error without its causes: the reason a download
    // failed ("HTTP 404 Not Found") has to be in the message itself.
    install_from_registry(name).map_err(|e| anyhow::anyhow!("{e:#}"))
}

fn install_from_registry(name: &str) -> Result<()> {
    validate_pack_name(name)?;

    let dest = config::packs_dir().join(name);
    clear_unfinished_install(name, &dest)?;

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .context("Failed to build the HTTP client")?;
    let registry = download(&client, reqwest::Url::parse(REGISTRY_URL)?)
        .context("Failed to download the pack registry")?;
    let base = resolve_pack_url(&String::from_utf8_lossy(&registry), name)?;

    let manifest_text = download(
        &client,
        reqwest::Url::parse(&format!("{base}/openpeon.json"))?,
    )
    .with_context(|| format!("Failed to download the manifest of pack '{name}'"))?;
    let (manifest, files) = from_openpeon(name, &String::from_utf8_lossy(&manifest_text))?;

    // A pack left half written is listed as installed and then fails to play.
    let result = write_pack(&client, &base, &dest, &manifest, &files);
    if result.is_err() {
        let _ = fs::remove_dir_all(&dest);
    }
    result
}

/// Refuse to install over a pack, and clear what an install that was
/// interrupted left behind.
///
/// The manifest is written last, so a directory without one is not a pack: it
/// is not listed and cannot be played. Taking it for an installed pack would
/// leave `install` answering "already installed" and `play` "not installed".
fn clear_unfinished_install(name: &str, dest: &Path) -> Result<()> {
    if dest.join("manifest.json").exists() {
        anyhow::bail!("Pack '{name}' is already installed");
    }
    if dest.exists() {
        fs::remove_dir_all(dest)
            .with_context(|| format!("Failed to clear the unfinished install of pack '{name}'"))?;
    }
    Ok(())
}

fn write_pack(
    client: &reqwest::blocking::Client,
    base: &str,
    dest: &Path,
    manifest: &PackManifest,
    files: &HashMap<String, String>,
) -> Result<()> {
    let sounds_dir = dest.join("sounds");
    fs::create_dir_all(&sounds_dir)?;

    for (file, remote) in files {
        // Built segment by segment: published file names contain spaces, `?`
        // and `!`, which a formatted URL would read as something else.
        let mut url = reqwest::Url::parse(base)?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid pack URL: {base}"))?
            .push("sounds")
            .extend(remote.split('/'));

        let bytes =
            download(client, url).with_context(|| format!("Failed to download {remote}"))?;
        if bytes.is_empty() {
            anyhow::bail!("Failed to download {remote}: the file is empty");
        }
        fs::write(sounds_dir.join(file), &bytes)?;
    }

    // Written last, and in the format vox has always stored: the manifest is
    // what makes a directory an installed pack.
    fs::write(
        dest.join("manifest.json"),
        serde_json::to_string_pretty(manifest)?,
    )?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An install killed while it downloads leaves `sounds/` and no manifest.
    #[test]
    fn an_interrupted_install_is_cleared_and_an_installed_pack_is_kept() {
        let tmp = tempfile::tempdir().unwrap();

        let unfinished = tmp.path().join("unfinished");
        fs::create_dir_all(unfinished.join("sounds")).unwrap();
        fs::write(unfinished.join("sounds/a.wav"), b"RIFF").unwrap();
        clear_unfinished_install("unfinished", &unfinished).unwrap();
        assert!(!unfinished.exists());

        let installed = tmp.path().join("installed");
        fs::create_dir_all(installed.join("sounds")).unwrap();
        fs::write(installed.join("manifest.json"), b"{}").unwrap();
        let err = clear_unfinished_install("installed", &installed).unwrap_err();
        assert_eq!(err.to_string(), "Pack 'installed' is already installed");
        assert!(installed.join("manifest.json").exists());

        clear_unfinished_install("absent", &tmp.path().join("absent")).unwrap();
    }
}
