//! Configuration paths, platform defaults, and validation enums.

use std::path::PathBuf;

pub const DEFAULT_BACKEND: &str = "pocket";

/// Language-aware default backend, used when neither the CLI flag nor a
/// stored preference selects one. The pocket checkpoint is English-only, so
/// non-English languages fall back to piper (per-language voices).
pub fn default_backend_for_lang(lang: Option<&str>) -> &'static str {
    match lang {
        None | Some("en") => DEFAULT_BACKEND,
        Some(_) => "piper",
    }
}

pub const SUPPORTED_LANGS: &[&str] = &[
    "en", "fr", "es", "de", "it", "pt", "zh", "ja", "ko", "ru", "ar", "nl",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Gender {
    Feminine,
    Masculine,
}

impl Gender {
    pub fn as_str(&self) -> &str {
        match self {
            Gender::Feminine => "feminine",
            Gender::Masculine => "masculine",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "feminine" => Ok(Gender::Feminine),
            "masculine" => Ok(Gender::Masculine),
            _ => anyhow::bail!("Invalid gender: {s}. Must be 'feminine' or 'masculine'"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntonationStyle {
    Calm,
    Energetic,
    Warm,
    Authoritative,
    Cheerful,
    Serious,
}

impl IntonationStyle {
    pub fn as_str(&self) -> &str {
        match self {
            IntonationStyle::Calm => "calm",
            IntonationStyle::Energetic => "energetic",
            IntonationStyle::Warm => "warm",
            IntonationStyle::Authoritative => "authoritative",
            IntonationStyle::Cheerful => "cheerful",
            IntonationStyle::Serious => "serious",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "calm" => Ok(IntonationStyle::Calm),
            "energetic" => Ok(IntonationStyle::Energetic),
            "warm" => Ok(IntonationStyle::Warm),
            "authoritative" => Ok(IntonationStyle::Authoritative),
            "cheerful" => Ok(IntonationStyle::Cheerful),
            "serious" => Ok(IntonationStyle::Serious),
            _ => anyhow::bail!(
                "Invalid style: {s}. Must be one of: calm, energetic, warm, authoritative, cheerful, serious"
            ),
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Ok(p) = std::env::var("VOX_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vox")
}

pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("VOX_DB_PATH") {
        return PathBuf::from(p);
    }
    config_dir().join("vox.db")
}

pub fn clones_dir() -> PathBuf {
    config_dir().join("clones")
}

pub fn packs_dir() -> PathBuf {
    config_dir().join("packs")
}

/// Model configuration loaded from models.toml.
/// User override: ~/.config/vox/models.toml
/// Fallback: bundled defaults compiled into the binary.
static MODELS_TOML: &str = include_str!("../models.toml");

/// Load model config: user override takes precedence, then bundled defaults.
pub fn load_models_config() -> toml::Table {
    let user_path = config_dir().join("models.toml");
    if user_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&user_path) {
            if let Ok(table) = content.parse::<toml::Table>() {
                return table;
            }
            eprintln!(
                "Warning: invalid models.toml at {}, using defaults",
                user_path.display()
            );
        }
    }
    MODELS_TOML
        .parse::<toml::Table>()
        .expect("bundled models.toml is invalid")
}

/// Get a string value from model config: models_config["section"]["key"].
pub fn model_config_str(section: &str, key: &str) -> Option<String> {
    let config = load_models_config();
    config
        .get(section)?
        .as_table()?
        .get(key)?
        .as_str()
        .map(String::from)
}
