pub mod claude_api;
pub mod sentence;
pub mod streaming;

use std::io::{self, BufRead};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::backend::{self, SpeakOptions};
use crate::db;

pub const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
pub const SYSTEM_PROMPT: &str = "Tu es un assistant vocal. Réponds de manière concise et naturelle, comme dans une conversation orale.";
pub const MAX_TOKENS: u32 = 1024;
pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2023-06-01";

const EXIT_WORDS: &[&str] = &[
    "quit",
    "exit",
    "au revoir",
    "bye",
    "goodbye",
    "stop",
    "arrête",
    "arrete",
];

pub struct ChatConfig {
    pub voice_clone: Option<db::VoiceClone>,
    pub lang: Option<String>,
    pub api_key: String,
    pub model: String,
}

#[derive(Serialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            voice_clone: None,
            lang: None,
            api_key: String::new(),
            model: DEFAULT_MODEL.to_string(),
        }
    }
}

pub fn is_exit(text: &str) -> bool {
    let lower = text.to_lowercase();
    let trimmed = lower.trim().trim_end_matches(['.', '!', '?']);
    EXIT_WORDS.contains(&trimmed)
}

pub fn record_until_enter(output_path: &str) -> Result<()> {
    let mut child = Command::new("rec")
        .arg(output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context(crate::clone::sox_install_hint())?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;

    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

pub fn speak_text(text: &str, config: &ChatConfig) -> Result<()> {
    let (backend_name, opts) = if let Some(ref vc) = config.voice_clone {
        (
            "qwen",
            SpeakOptions {
                ref_audio: Some(vc.ref_audio.clone()),
                ref_text: vc.ref_text.clone(),
                lang: config.lang.clone(),
                ..Default::default()
            },
        )
    } else {
        (
            "qwen",
            SpeakOptions {
                lang: config.lang.clone(),
                ..Default::default()
            },
        )
    };
    let be = backend::get_backend(backend_name)?;
    be.speak(text, &opts)?;
    Ok(())
}

pub use claude_api::{ClaudeRequest, build_claude_request, parse_claude_response};
pub use streaming::run_chat_loop;
