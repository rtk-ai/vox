use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub const DEFAULT_BACKEND: &str = "say";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_BACKEND: &str = "qwen-native";

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
