use std::io::{self, BufRead, Write};
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::backend::{self, SpeakOptions};
use crate::db;
use crate::stt;

const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
const SYSTEM_PROMPT: &str = "Tu es un assistant vocal. Réponds de manière concise et naturelle, comme dans une conversation orale.";
const MAX_TOKENS: u32 = 1024;
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

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

#[derive(Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub max_tokens: u32,
    pub system: String,
    pub messages: Vec<Message>,
}

#[derive(Deserialize)]
pub struct ClaudeResponse {
    pub content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
pub struct ContentBlock {
    pub text: String,
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

pub fn build_claude_request(model: &str, messages: &[Message]) -> ClaudeRequest {
    ClaudeRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: SYSTEM_PROMPT.to_string(),
        messages: messages.to_vec(),
    }
}

pub fn parse_claude_response(body: &str) -> Result<String> {
    let resp: ClaudeResponse =
        serde_json::from_str(body).context("Failed to parse Claude API response")?;
    resp.content
        .first()
        .map(|b| b.text.clone())
        .ok_or_else(|| anyhow::anyhow!("Empty response from Claude"))
}

fn call_claude(api_key: &str, model: &str, messages: &[Message]) -> Result<String> {
    let request = build_claude_request(model, messages);
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .context("Failed to call Claude API")?;

    let status = resp.status();
    let body = resp.text().context("Failed to read Claude API response")?;
    if !status.is_success() {
        anyhow::bail!("Claude API error ({status}): {body}");
    }
    parse_claude_response(&body)
}

pub fn record_until_enter(output_path: &str) -> Result<()> {
    let mut child = Command::new("rec")
        .arg(output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context(crate::clone::sox_install_hint())?;

    // Wait for Enter
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;

    // Kill rec
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

fn speak_text(text: &str, config: &ChatConfig) -> Result<()> {
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

fn is_exit(text: &str) -> bool {
    let lower = text.to_lowercase();
    let trimmed = lower.trim().trim_end_matches(['.', '!', '?']);
    EXIT_WORDS.contains(&trimmed)
}

pub fn run_chat_loop(config: ChatConfig) -> Result<()> {
    let mut messages: Vec<Message> = Vec::new();
    let greeting = "Bonjour, je t'écoute.";

    eprintln!("{greeting}");
    speak_text(greeting, &config)?;

    let tmp_dir = std::env::temp_dir();
    let audio_path = tmp_dir.join("vox_chat_input.wav");
    let audio_str = audio_path.to_string_lossy().to_string();

    loop {
        eprintln!("\n[Appuie sur Enter quand tu as fini de parler]");
        io::stderr().flush()?;

        record_until_enter(&audio_str)?;

        eprint!("Transcription...");
        io::stderr().flush()?;
        let user_text = stt::transcribe(&audio_str, config.lang.as_deref())?;
        eprintln!(" \"{user_text}\"");

        if user_text.is_empty() {
            eprintln!("(rien détecté, réessaie)");
            continue;
        }

        if is_exit(&user_text) {
            let farewell = "Au revoir !";
            eprintln!("{farewell}");
            speak_text(farewell, &config)?;
            break;
        }

        messages.push(Message {
            role: "user".to_string(),
            content: user_text,
        });

        eprint!("Réflexion...");
        io::stderr().flush()?;
        let reply = call_claude(&config.api_key, &config.model, &messages)?;
        eprintln!(" OK");
        eprintln!("Claude: {reply}");

        messages.push(Message {
            role: "assistant".to_string(),
            content: reply.clone(),
        });

        speak_text(&reply, &config)?;
    }

    // Cleanup temp audio
    let _ = std::fs::remove_file(&audio_path);
    Ok(())
}
