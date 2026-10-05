//! SQLite database — preferences, voice clones, usage logging, and statistics.
//!
//! All state is persisted in `~/.config/vox/vox.db` with WAL mode enabled.
//! Schema is auto-migrated on first open.

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::config;

#[derive(Debug, Clone, Default)]
pub struct Preferences {
    pub backend: Option<String>,
    pub voice: Option<String>,
    pub lang: Option<String>,
    pub rate: Option<u32>,
    pub gender: Option<String>,
    pub style: Option<String>,
    pub model: Option<String>,
    pub stt_model: Option<String>,
    pub pack: Option<String>,
}

impl Preferences {
    /// The preferences as `vox config show` and the `vox_config_show` MCP tool
    /// print them. Built once so that a preference added later shows in both.
    pub fn summary_lines(&self) -> Vec<String> {
        let text = |value: &Option<String>| value.clone().unwrap_or_else(|| "(default)".into());
        vec![
            format!("backend: {}", text(&self.backend)),
            format!("voice:   {}", text(&self.voice)),
            format!("lang:    {}", text(&self.lang)),
            format!("rate:    {}", text(&self.rate.map(|r| r.to_string()))),
            format!("gender:  {}", text(&self.gender)),
            format!("style:   {}", text(&self.style)),
            format!("model:   {}", text(&self.model)),
            format!("stt_model: {}", text(&self.stt_model)),
            format!("pack:    {}", self.pack.as_deref().unwrap_or("(none)")),
        ]
    }
}

/// Every preference `set_preference` accepts, in the order `vox config show`
/// prints them. The help of `vox config set` and the description of the MCP
/// `vox_config_set` tool are written from this list, so a key accepted here
/// is a key documented there.
pub const PREFERENCE_KEYS: &[&str] = &[
    "backend",
    "voice",
    "lang",
    "rate",
    "gender",
    "style",
    "model",
    "stt_model",
    "pack",
];

/// Stored and shown, but read by no backend yet.
const UNREAD_PREFERENCE_KEYS: &[&str] = &["gender", "style"];

/// The preference keys as one line of help: the ones that act first, then
/// the ones that are only stored.
pub fn preference_keys_help() -> String {
    let read: Vec<&str> = PREFERENCE_KEYS
        .iter()
        .copied()
        .filter(|key| !UNREAD_PREFERENCE_KEYS.contains(key))
        .collect();
    format!(
        "{}; {} are accepted but have no effect yet",
        read.join(", "),
        UNREAD_PREFERENCE_KEYS.join(" and ")
    )
}

#[derive(Debug, Clone)]
pub struct VoiceClone {
    pub name: String,
    pub ref_audio: String,
    pub ref_text: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct UsageEntry {
    pub timestamp: String,
    pub backend: String,
    pub voice: Option<String>,
    pub lang: Option<String>,
    pub text_len: usize,
    pub duration_ms: Option<u64>,
}

pub fn open() -> Result<Connection> {
    let path = config::db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create config directory")?;
    }
    let conn = Connection::open(&path).context("Failed to open database")?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS preferences (
            id      INTEGER PRIMARY KEY CHECK (id = 1),
            backend TEXT,
            voice   TEXT,
            lang    TEXT,
            rate    INTEGER,
            gender  TEXT,
            style   TEXT,
            model   TEXT
        );

        CREATE TABLE IF NOT EXISTS usage_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now')),
            backend     TEXT NOT NULL,
            voice       TEXT,
            lang        TEXT,
            text_len    INTEGER NOT NULL,
            duration_ms INTEGER
        );

        CREATE TABLE IF NOT EXISTS voice_clones (
            name       TEXT PRIMARY KEY,
            ref_audio  TEXT NOT NULL,
            ref_text   TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now'))
        );",
    )?;

    // Columns added after the initial schema. Each is probed rather than
    // versioned because the table has always been migrated this way.
    for (column, ddl) in [
        ("pack", "ALTER TABLE preferences ADD COLUMN pack TEXT;"),
        (
            "stt_model",
            "ALTER TABLE preferences ADD COLUMN stt_model TEXT;",
        ),
    ] {
        let exists = conn
            .prepare(&format!("SELECT {column} FROM preferences LIMIT 0"))
            .is_ok();
        if !exists {
            conn.execute_batch(ddl)?;
        }
    }

    Ok(())
}

// --- Preferences ---

pub fn get_preferences(conn: &Connection) -> Result<Preferences> {
    let mut stmt = conn.prepare(
        "SELECT backend, voice, lang, rate, gender, style, model, stt_model, pack \
         FROM preferences WHERE id = 1",
    )?;
    let result = stmt.query_row([], |row| {
        Ok(Preferences {
            backend: row.get(0)?,
            voice: row.get(1)?,
            lang: row.get(2)?,
            rate: row.get::<_, Option<u32>>(3)?,
            gender: row.get(4)?,
            style: row.get(5)?,
            model: row.get(6)?,
            stt_model: row.get(7)?,
            pack: row.get(8)?,
        })
    });
    match result {
        Ok(prefs) => Ok(prefs),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Preferences::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn set_preference(conn: &Connection, key: &str, value: &str) -> Result<()> {
    if !PREFERENCE_KEYS.contains(&key) {
        anyhow::bail!(
            "Unknown preference: {key}. Valid keys: {}",
            PREFERENCE_KEYS.join(", ")
        );
    }

    // Validate specific keys
    match key {
        "gender" => {
            config::Gender::parse(value)?;
        }
        "style" => {
            config::IntonationStyle::parse(value)?;
        }
        "rate" => {
            value
                .parse::<u32>()
                .context("Rate must be a positive integer")?;
        }
        "lang" => {
            if !config::SUPPORTED_LANGS.contains(&value) {
                anyhow::bail!(
                    "Unsupported language: {value}. Supported: {}",
                    config::SUPPORTED_LANGS.join(", ")
                );
            }
        }
        "backend" => {
            let valid_backends = crate::backend::supported_backends();
            if !valid_backends.contains(&value) {
                anyhow::bail!(
                    "Unknown backend: {value}. Must be one of: {}",
                    valid_backends.join(", ")
                );
            }
        }
        _ => {}
    }

    // Upsert: insert or update. The insert names the id alone: every other
    // column is NULL by default, so a column `migrate` adds needs nothing here.
    conn.execute(
        "INSERT INTO preferences (id) VALUES (1) ON CONFLICT(id) DO NOTHING",
        [],
    )?;
    let sql = format!("UPDATE preferences SET {key} = ?1 WHERE id = 1");
    conn.execute(&sql, [value])?;
    Ok(())
}

pub fn reset_preferences(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM preferences WHERE id = 1", [])?;
    Ok(())
}

// --- Voice Clones ---

pub fn add_clone(
    conn: &Connection,
    name: &str,
    ref_audio: &str,
    ref_text: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO voice_clones (name, ref_audio, ref_text) VALUES (?1, ?2, ?3)",
        rusqlite::params![name, ref_audio, ref_text],
    )?;
    Ok(())
}

pub fn get_clone(conn: &Connection, name: &str) -> Result<Option<VoiceClone>> {
    let mut stmt = conn.prepare(
        "SELECT name, ref_audio, ref_text, created_at FROM voice_clones WHERE name = ?1",
    )?;
    let result = stmt.query_row([name], |row| {
        Ok(VoiceClone {
            name: row.get(0)?,
            ref_audio: row.get(1)?,
            ref_text: row.get(2)?,
            created_at: row.get(3)?,
        })
    });
    match result {
        Ok(clone) => Ok(Some(clone)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn list_clones(conn: &Connection) -> Result<Vec<VoiceClone>> {
    let mut stmt = conn
        .prepare("SELECT name, ref_audio, ref_text, created_at FROM voice_clones ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        Ok(VoiceClone {
            name: row.get(0)?,
            ref_audio: row.get(1)?,
            ref_text: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    let mut clones = Vec::new();
    for row in rows {
        clones.push(row?);
    }
    Ok(clones)
}

pub fn remove_clone(conn: &Connection, name: &str) -> Result<bool> {
    let count = conn.execute("DELETE FROM voice_clones WHERE name = ?1", [name])?;
    Ok(count > 0)
}

// --- Usage Log ---

pub fn log_usage(
    conn: &Connection,
    backend: &str,
    voice: Option<&str>,
    lang: Option<&str>,
    text_len: usize,
    duration_ms: Option<u64>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_log (backend, voice, lang, text_len, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![backend, voice, lang, text_len as i64, duration_ms.map(|d| d as i64)],
    )?;
    Ok(())
}

/// Log one utterance by its text. `vox stats` reports characters, so the
/// length is counted in characters: `str::len` is bytes, and counts "é" twice.
pub fn log_speech(
    conn: &Connection,
    backend: &str,
    voice: Option<&str>,
    lang: Option<&str>,
    text: &str,
    duration_ms: Option<u64>,
) -> Result<()> {
    log_usage(
        conn,
        backend,
        voice,
        lang,
        text.chars().count(),
        duration_ms,
    )
}

pub fn get_usage_stats(conn: &Connection) -> Result<Vec<UsageEntry>> {
    let mut stmt = conn.prepare(
        "SELECT timestamp, backend, voice, lang, text_len, duration_ms FROM usage_log ORDER BY id DESC LIMIT 50",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(UsageEntry {
            timestamp: row.get(0)?,
            backend: row.get(1)?,
            voice: row.get(2)?,
            lang: row.get(3)?,
            text_len: row.get::<_, i64>(4)? as usize,
            duration_ms: row.get::<_, Option<i64>>(5)?.map(|d| d as u64),
        })
    })?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

pub fn get_usage_summary(conn: &Connection) -> Result<(u64, u64)> {
    let mut stmt = conn.prepare("SELECT COUNT(*), COALESCE(SUM(text_len), 0) FROM usage_log")?;
    let (count, total_chars) = stmt.query_row([], |row| {
        Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))
    })?;
    Ok((count, total_chars))
}

#[derive(Debug)]
pub struct BackendStats {
    pub backend: String,
    pub calls: u64,
    pub total_chars: u64,
    pub total_duration_ms: u64,
}

#[derive(Debug)]
pub struct LangStats {
    pub lang: String,
    pub calls: u64,
}

pub fn get_backend_stats(conn: &Connection) -> Result<Vec<BackendStats>> {
    let mut stmt = conn.prepare(
        "SELECT backend, COUNT(*), COALESCE(SUM(text_len), 0), COALESCE(SUM(duration_ms), 0) FROM usage_log GROUP BY backend ORDER BY COUNT(*) DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(BackendStats {
            backend: row.get(0)?,
            calls: row.get::<_, i64>(1)? as u64,
            total_chars: row.get::<_, i64>(2)? as u64,
            total_duration_ms: row.get::<_, i64>(3)? as u64,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_lang_stats(conn: &Connection) -> Result<Vec<LangStats>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(lang, '?'), COUNT(*) FROM usage_log GROUP BY lang ORDER BY COUNT(*) DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(LangStats {
            lang: row.get(0)?,
            calls: row.get::<_, i64>(1)? as u64,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_total_duration_ms(conn: &Connection) -> Result<u64> {
    let mut stmt = conn.prepare("SELECT COALESCE(SUM(duration_ms), 0) FROM usage_log")?;
    let total = stmt.query_row([], |row| row.get::<_, i64>(0))?;
    Ok(total as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database created before `pack` and `stt_model` existed must gain both
    /// columns without losing the preferences already stored in it.
    #[test]
    fn migrate_upgrades_a_legacy_preferences_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE preferences (
                id      INTEGER PRIMARY KEY CHECK (id = 1),
                backend TEXT, voice TEXT, lang TEXT, rate INTEGER,
                gender  TEXT, style TEXT, model TEXT
            );
            INSERT INTO preferences (id, lang) VALUES (1, 'fr');",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let prefs = get_preferences(&conn).unwrap();
        assert_eq!(prefs.lang.as_deref(), Some("fr"));
        assert_eq!(prefs.pack, None);
        assert_eq!(prefs.stt_model, None);

        // And the new column is writable, not just readable.
        set_preference(&conn, "stt_model", "openai/whisper-tiny").unwrap();
        assert_eq!(
            get_preferences(&conn).unwrap().stt_model.as_deref(),
            Some("openai/whisper-tiny")
        );
    }

    /// Every key the setter accepts has a column to land in and a line in
    /// `config show`: a key added to the list alone fails here, not at a user.
    #[test]
    fn every_preference_key_is_stored_and_shown() {
        let conn = open_in_memory().unwrap();
        for key in PREFERENCE_KEYS {
            let value = match *key {
                "backend" => "piper",
                "lang" => "fr",
                "rate" => "180",
                "gender" => "feminine",
                "style" => "calm",
                _ => "some-value",
            };
            set_preference(&conn, key, value).unwrap_or_else(|e| panic!("{key}: {e}"));
            let lines = get_preferences(&conn).unwrap().summary_lines();
            assert!(
                lines
                    .iter()
                    .any(|line| line.starts_with(&format!("{key}:")) && line.ends_with(value)),
                "{key} = {value} missing from {lines:?}"
            );
        }
    }

    /// The first preference ever set creates the row with every other column
    /// empty, the ones added by `migrate` included.
    #[test]
    fn first_preference_leaves_every_other_one_unset() {
        let conn = open_in_memory().unwrap();
        set_preference(&conn, "lang", "fr").unwrap();
        let prefs = get_preferences(&conn).unwrap();
        assert_eq!(prefs.lang.as_deref(), Some("fr"));
        let unset = [
            &prefs.backend,
            &prefs.voice,
            &prefs.gender,
            &prefs.style,
            &prefs.model,
            &prefs.stt_model,
            &prefs.pack,
        ];
        assert!(unset.iter().all(|value| value.is_none()), "{prefs:?}");
        assert_eq!(prefs.rate, None);
    }

    #[test]
    fn help_names_every_preference_key() {
        let help = preference_keys_help();
        for key in PREFERENCE_KEYS {
            assert!(help.contains(key), "{key} missing from {help:?}");
        }
        assert!(help.ends_with("gender and style are accepted but have no effect yet"));
    }

    /// Running migrate twice must be a no-op, not a duplicate-column error.
    #[test]
    fn migrate_is_idempotent() {
        let conn = open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        assert!(get_preferences(&conn).is_ok());
    }
}
