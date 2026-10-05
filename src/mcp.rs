//! MCP (Model Context Protocol) server — JSON-RPC 2.0 over stdio.
//!
//! Exposes 14 tools for AI assistants: speak, hear, voice cloning, config, stats, packs.
//! Launched via `vox serve` and auto-configured by `vox init` for 14 AI tools.

use std::io::{self, BufRead, Write};

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::{self, SpeakOptions};
use crate::clone;
use crate::db;
use crate::mic;
use crate::pack;
use crate::stt;

const SERVER_NAME: &str = "vox";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";

const VOX_INSTRUCTIONS_TEMPLATE: &str = "\
You have access to vox, a text-to-speech tool. Use it to give spoken feedback to the user.\n\
\n\
WHEN TO SPEAK (vox_speak):\n\
- After completing a significant task (feature, bug fix, refactor): summarize what was done\n\
- When the user asks you to explain something verbally\n\
- For important warnings or status updates the user should hear\n\
\n\
WHEN NOT TO SPEAK:\n\
- Trivial operations (formatting, single-line fixes, file reads)\n\
- When the user is clearly reading the output already\n\
- Rapid back-and-forth conversation\n\
\n\
GUIDELINES:\n\
- Keep summaries under 2 sentences\n\
{language_guideline}\n\
- Use vox_config_show to check the user's preferred voice/backend before first use\n\
- For longer explanations, use vox_speak with a concise summary, not the full text\n\
\n\
VOICE CONVERSATION (vox_hear + vox_speak):\n\
You can have a voice conversation with the user — you ARE the brain, no API key needed.\n\
Loop: vox_hear (listen) → you think → vox_speak (respond). Repeat until the user says goodbye.\n\
When the user asks to \"chat\" or \"talk\" or \"parler\", start this loop.\n\
\n\
CLAUDE CODE PLUGIN:\n\
{plugin_note}";

fn vox_instructions(lang: Option<&str>) -> String {
    let language_guideline = match lang {
        Some(lang) => format!(
            "- Speak in the user's language ({lang}); honour any language they explicitly ask for"
        ),
        None => "- Match the language the user is writing in".to_string(),
    };
    VOX_INSTRUCTIONS_TEMPLATE
        .replace("{language_guideline}", &language_guideline)
        .replace("{plugin_note}", &crate::init::plugin_note())
}

#[derive(Deserialize)]
struct JsonRpcMessage {
    id: Option<Value>,
    method: Option<String>,
    params: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

impl JsonRpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(json!({"code": code, "message": message})),
        }
    }
}

/// Run the vox MCP server on stdio.
pub fn run_server() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: JsonRpcMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                let resp = JsonRpcResponse::err(Value::Null, -32700, format!("parse error: {e}"));
                write_response(&mut stdout, &resp)?;
                continue;
            }
        };

        let method = msg.method.as_deref().unwrap_or("");

        // Notifications have no id
        let id = match msg.id {
            Some(id) => id,
            None => continue,
        };

        let response = match method {
            "initialize" => handle_initialize(id),
            "ping" => JsonRpcResponse::ok(id, json!({})),
            "tools/list" => handle_tools_list(id),
            "tools/call" => handle_tools_call(id, &msg.params),
            _ => JsonRpcResponse::err(id, -32601, format!("method not found: {method}")),
        };

        write_response(&mut stdout, &response)?;
    }

    Ok(())
}

fn write_response(stdout: &mut io::Stdout, resp: &JsonRpcResponse) -> Result<()> {
    let json = serde_json::to_string(resp)?;
    writeln!(stdout, "{json}")?;
    stdout.flush()?;
    Ok(())
}

fn handle_initialize(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::ok(
        id,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            },
            "instructions": vox_instructions(crate::lang::default_lang().as_deref())
        }),
    )
}

fn handle_tools_list(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::ok(id, json!({ "tools": tool_definitions() }))
}

fn handle_tools_call(id: Value, params: &Option<Value>) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => return JsonRpcResponse::err(id, -32602, "missing params".into()),
    };

    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return JsonRpcResponse::err(id, -32602, "missing tool name".into()),
    };

    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let result = call_tool(tool_name, &args);

    JsonRpcResponse::ok(id, json!(result))
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

/// The backends this binary can speak with, for the tool descriptions. An
/// agent takes the list literally, so it names only what the build contains:
/// kokoro sits behind a feature and `say` exists on macOS alone.
fn backend_list() -> String {
    backend::supported_backends().join(", ")
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "vox_speak",
            "description": "Read text aloud using text-to-speech. Supports multiple backends, voices and languages.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to speak aloud"
                    },
                    "voice": {
                        "type": "string",
                        "description": "Voice name or clone name (optional)"
                    },
                    "lang": {
                        "type": "string",
                        "description": "Language code: en, fr, es, de, it, pt, zh, ja, ko, ru, ar, nl"
                    },
                    "backend": {
                        "type": "string",
                        "description": format!("TTS backend: {} (default: the stored preference, else pocket for English and piper for other languages)", backend_list())
                    },
                    "style": {
                        "type": "string",
                        "description": "No effect yet: accepted, but no backend reads it (calm, energetic, warm, authoritative, cheerful, serious)"
                    },
                    "gender": {
                        "type": "string",
                        "description": "No effect yet: accepted, but no backend reads it (feminine, masculine)"
                    },
                    "rate": {
                        "type": "integer",
                        "description": "Speech rate in words per minute. Only the say backend, on macOS, reads it"
                    },
                    "volume": {
                        "type": "number",
                        "description": "Volume multiplier (1.0 = normal, 0.5 = half, 2.0 = double)"
                    }
                },
                "required": ["text"]
            }
        },
        {
            "name": "vox_list_voices",
            "description": "List available voices for a given TTS backend.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "backend": {
                        "type": "string",
                        "description": format!("TTS backend: {} (default: the one vox_speak would use)", backend_list())
                    }
                }
            }
        },
        {
            "name": "vox_clone_list",
            "description": "List all saved voice clones.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "vox_clone_add",
            "description": "Add a voice clone from an audio file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name for the voice clone"
                    },
                    "audio": {
                        "type": "string",
                        "description": "Path to the reference audio file (wav, mp3, flac or ogg); it is converted and kept as a WAV in vox's clones directory"
                    },
                    "text": {
                        "type": "string",
                        "description": "Transcription of the reference audio (improves quality)"
                    }
                },
                "required": ["name", "audio"]
            }
        },
        {
            "name": "vox_clone_remove",
            "description": "Remove a voice clone by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the voice clone to remove"
                    }
                },
                "required": ["name"]
            }
        },
        {
            "name": "vox_config_show",
            "description": "Show current vox preferences (backend, voice, language, rate, model, stt_model, pack).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "vox_config_set",
            "description": "Set a vox preference.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": format!("Preference key: {}: no backend reads them", db::preference_keys_help())
                    },
                    "value": {
                        "type": "string",
                        "description": "Preference value"
                    }
                },
                "required": ["key", "value"]
            }
        },
        {
            "name": "vox_stats",
            "description": "Show vox usage statistics (total requests, characters spoken, recent history).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "vox_pack_list",
            "description": "List installed and available fun sound packs (peon-ping compatible: Warcraft, StarCraft, Red Alert voices).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "vox_pack_install",
            "description": "Install a fun sound pack. Available: peon, peon_fr, peon_pl, peasant, peasant_fr, sc_kerrigan, sc_battlecruiser, ra2_soviet_engineer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Pack name to install"
                    }
                },
                "required": ["name"]
            }
        },
        {
            "name": "vox_pack_set",
            "description": "Set the active sound pack.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Pack name to activate"
                    }
                },
                "required": ["name"]
            }
        },
        {
            "name": "vox_pack_play",
            "description": "Play a random sound from a pack category. Categories: greeting, acknowledge, complete, error, permission, resource_limit, annoyed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Sound category (default: greeting)"
                    },
                    "pack": {
                        "type": "string",
                        "description": "Pack name (uses active pack if omitted)"
                    }
                }
            }
        },
        {
            "name": "vox_pack_remove",
            "description": "Remove an installed sound pack.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Pack name to remove"
                    }
                },
                "required": ["name"]
            }
        },
        {
            "name": "vox_hear",
            "description": "Record audio from the microphone and transcribe it to text (speech-to-text, local Whisper, 99 languages, all platforms). Recording starts when voice is detected and stops automatically after silence. Use this with vox_speak to create a voice conversation loop — Claude Code is the brain, no API key needed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "lang": {
                        "type": "string",
                        "description": "Language code for transcription: en, fr, es, de, ja, zh, etc. (default: auto-detect)"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Maximum recording duration in seconds (default: 30)"
                    },
                    "silence": {
                        "type": "number",
                        "description": "Seconds of silence before stopping (default: 2.0)"
                    },
                    "model": {
                        "type": "string",
                        "description": "Whisper model repo on Hugging Face (default: openai/whisper-base, or the stt_model preference / VOX_STT_MODEL / models.toml; openai/whisper-large-v3-turbo for best quality on GPU)"
                    },
                    "file": {
                        "type": "string",
                        "description": "Transcribe this WAV file instead of recording from the microphone"
                    }
                }
            }
        }
    ])
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ToolResult {
    content: Vec<ToolContent>,
    #[serde(rename = "isError", skip_serializing_if = "std::ops::Not::not")]
    is_error: bool,
}

#[derive(Serialize)]
struct ToolContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

fn tool_ok(text: String) -> ToolResult {
    ToolResult {
        content: vec![ToolContent {
            content_type: "text".into(),
            text,
        }],
        is_error: false,
    }
}

fn tool_err(text: String) -> ToolResult {
    ToolResult {
        content: vec![ToolContent {
            content_type: "text".into(),
            text,
        }],
        is_error: true,
    }
}

fn call_tool(name: &str, args: &Value) -> ToolResult {
    match name {
        "vox_speak" => tool_speak(args),
        "vox_list_voices" => tool_list_voices(args),
        "vox_clone_list" => tool_clone_list(),
        "vox_clone_add" => tool_clone_add(args),
        "vox_clone_remove" => tool_clone_remove(args),
        "vox_config_show" => tool_config_show(),
        "vox_config_set" => tool_config_set(args),
        "vox_stats" => tool_stats(),
        "vox_pack_list" => tool_pack_list(),
        "vox_pack_install" => tool_pack_install(args),
        "vox_pack_set" => tool_pack_set(args),
        "vox_pack_play" => tool_pack_play(args),
        "vox_pack_remove" => tool_pack_remove(args),
        "vox_hear" => tool_hear(args),
        _ => tool_err(format!("unknown tool: {name}")),
    }
}

/// What a `vox_speak` call resolves to, before any audio is produced.
struct SpeakRequest {
    backend: String,
    opts: SpeakOptions,
    /// A clone the caller named that the chosen backend cannot use.
    ignored_clone: Option<String>,
}

/// Merge the tool arguments with the stored preferences, the way the command
/// line merges its flags: argument > preference > language-aware default.
fn speak_request(conn: &Connection, args: &Value) -> SpeakRequest {
    let prefs = db::get_preferences(conn).unwrap_or_default();
    let arg = |key: &str| args.get(key).and_then(|v| v.as_str());

    let lang = arg("lang").map(String::from).or(prefs.lang);
    let mut voice = arg("voice").map(String::from).or(prefs.voice);
    let rate = args
        .get("rate")
        .and_then(|v| v.as_u64())
        .map(|r| r as u32)
        .or(prefs.rate);
    let gender = arg("gender").map(String::from).or(prefs.gender);
    let style = arg("style").map(String::from).or(prefs.style);
    let volume = args
        .get("volume")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(1.0)
        .clamp(0.0, 5.0);

    // Resolve voice clone
    let voice_clone = voice
        .as_deref()
        .and_then(|name| clone::resolve_voice(conn, name).ok().flatten());

    let backend = clone::speak_backend(
        arg("backend"),
        prefs.backend.as_deref(),
        lang.as_deref(),
        voice_clone.is_some(),
        clone::pocket_can_clone(),
    );

    let mut ref_audio = None;
    let mut ref_text = None;
    let mut ignored_clone = None;
    if let Some(vc) = voice_clone {
        if !clone::can_clone(&backend) {
            ignored_clone = Some(vc.name);
        }
        ref_audio = Some(vc.ref_audio);
        ref_text = vc.ref_text;
        voice = None;
    }

    let opts = SpeakOptions {
        voice,
        lang,
        rate,
        gender,
        style,
        ref_audio,
        ref_text,
        // No tool argument for it, but the stored preference still applies,
        // as it does on the command line.
        model: prefs.model,
        volume,
        // Deliberately not exposed over MCP: an agent-supplied path would be
        // an arbitrary file write. Saving stays a local CLI concern.
        output: None,
    };

    SpeakRequest {
        backend,
        opts,
        ignored_clone,
    }
}

fn tool_speak(args: &Value) -> ToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return tool_err("missing required parameter: text".into()),
    };

    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };
    let SpeakRequest {
        backend: effective_backend,
        opts,
        ignored_clone,
    } = speak_request(&conn, args);

    let bk = match backend::get_backend(&effective_backend) {
        Ok(b) => b,
        Err(e) => return tool_err(format!("backend error: {e}")),
    };

    let start = std::time::Instant::now();
    if let Err(e) = bk.speak(text, &opts) {
        return tool_err(format!("speak error: {e}"));
    }
    let duration_ms = start.elapsed().as_millis() as u64;

    let _ = db::log_speech(
        &conn,
        &effective_backend,
        opts.voice.as_deref(),
        opts.lang.as_deref(),
        text,
        Some(duration_ms),
    );

    let note = ignored_clone
        .map(|name| {
            format!(
                " Note: the {effective_backend} backend cannot clone voices, so '{name}' was \
                 ignored; omit backend or use qwen-native."
            )
        })
        .unwrap_or_default();
    tool_ok(format!(
        "Spoken: \"{}\" ({duration_ms}ms, {effective_backend}){note}",
        truncate_for_echo(text)
    ))
}

/// Shorten text for the tool result. Truncating by byte index aborts the whole
/// MCP server on any multi-byte character, so count characters instead.
pub fn truncate_for_echo(text: &str) -> String {
    const MAX_CHARS: usize = 80;
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX_CHARS - 3).collect();
    format!("{head}...")
}

fn tool_list_voices(args: &Value) -> ToolResult {
    // Without an argument, list the voices of the backend vox_speak would
    // use. Resolved by the same code, so a stored voice that is a clone
    // counts here as it does there.
    let backend_name = match db::open() {
        Ok(conn) => speak_request(&conn, args).backend,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    let bk = match backend::get_backend(&backend_name) {
        Ok(b) => b,
        Err(e) => return tool_err(format!("backend error: {e}")),
    };

    match bk.list_voices() {
        Ok(voices) => tool_ok(voices.join("\n")),
        Err(e) => tool_err(format!("error listing voices: {e}")),
    }
}

fn tool_clone_list() -> ToolResult {
    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    match db::list_clones(&conn) {
        Ok(clones) => {
            if clones.is_empty() {
                tool_ok("No voice clones.".into())
            } else {
                let lines: Vec<String> = clones
                    .iter()
                    .map(|c| {
                        let text_info = c
                            .ref_text
                            .as_deref()
                            .map(|t| format!(" (text: \"{t}\")"))
                            .unwrap_or_default();
                        format!(
                            "{}: {}{} [{}]",
                            c.name, c.ref_audio, text_info, c.created_at
                        )
                    })
                    .collect();
                tool_ok(lines.join("\n"))
            }
        }
        Err(e) => tool_err(format!("error: {e}")),
    }
}

fn tool_clone_add(args: &Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_err("missing required parameter: name".into()),
    };
    let audio = match args.get("audio").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return tool_err("missing required parameter: audio".into()),
    };
    let text = args.get("text").and_then(|v| v.as_str());

    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    match clone::add_clone_from_file(&conn, name, audio, text) {
        Ok(stored) => tool_ok(format!(
            "Voice clone '{name}' added (reference saved to {stored})."
        )),
        Err(e) => tool_err(format!("error: {e}")),
    }
}

fn tool_clone_remove(args: &Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_err("missing required parameter: name".into()),
    };

    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    match clone::remove_clone(&conn, name) {
        Ok(true) => tool_ok(format!("Voice clone '{name}' removed.")),
        Ok(false) => tool_err(format!("Voice clone '{name}' not found.")),
        Err(e) => tool_err(format!("error: {e}")),
    }
}

fn tool_config_show() -> ToolResult {
    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    match db::get_preferences(&conn) {
        Ok(prefs) => {
            let mut lines = prefs.summary_lines();
            lines.push(crate::accel::config_line());
            tool_ok(lines.join("\n"))
        }
        Err(e) => tool_err(format!("error: {e}")),
    }
}

fn tool_config_set(args: &Value) -> ToolResult {
    let key = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return tool_err("missing required parameter: key".into()),
    };
    let value = match args.get("value").and_then(|v| v.as_str()) {
        Some(v) => v,
        None => return tool_err("missing required parameter: value".into()),
    };

    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    match db::set_preference(&conn, key, value) {
        Ok(_) => tool_ok(format!("{key} = {value}")),
        Err(e) => tool_err(format!("error: {e}")),
    }
}

fn tool_stats() -> ToolResult {
    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    let (count, total_chars) = match db::get_usage_summary(&conn) {
        Ok(s) => s,
        Err(e) => return tool_err(format!("error: {e}")),
    };

    let mut output = format!("Total requests: {count}\nTotal characters: {total_chars}");

    if count > 0
        && let Ok(entries) = db::get_usage_stats(&conn)
    {
        output.push_str("\n\nRecent usage:");
        for e in entries.iter().take(10) {
            let voice_str = e.voice.as_deref().unwrap_or("-");
            let lang_str = e.lang.as_deref().unwrap_or("-");
            let dur_str = e
                .duration_ms
                .map(|d| format!("{d}ms"))
                .unwrap_or_else(|| "-".into());
            output.push_str(&format!(
                "\n  {} | {} | voice={} lang={} | {}chars | {}",
                e.timestamp, e.backend, voice_str, lang_str, e.text_len, dur_str
            ));
        }
    }

    tool_ok(output)
}

// ---------------------------------------------------------------------------
// Sound pack tools
// ---------------------------------------------------------------------------

fn tool_pack_list() -> ToolResult {
    let installed = match pack::list_installed() {
        Ok(p) => p,
        Err(e) => return tool_err(format!("error: {e}")),
    };

    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };
    let active = db::get_preferences(&conn)
        .ok()
        .and_then(|p| p.pack)
        .unwrap_or_default();

    let mut output = String::new();

    if installed.is_empty() {
        output.push_str("No packs installed.\n");
    } else {
        output.push_str("Installed:\n");
        for p in &installed {
            let marker = if p.name == active { " (active)" } else { "" };
            output.push_str(&format!("  {} — {}{}\n", p.name, p.display_name, marker));
        }
    }

    let installed_names: Vec<&str> = installed.iter().map(|p| p.name.as_str()).collect();
    let not_installed: Vec<&&str> = pack::list_available()
        .iter()
        .filter(|n| !installed_names.contains(*n))
        .collect();

    if !not_installed.is_empty() {
        output.push_str("\nAvailable for install:\n");
        for name in &not_installed {
            output.push_str(&format!("  {name}\n"));
        }
    }

    tool_ok(output)
}

fn tool_pack_install(args: &Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_err("missing required parameter: name".into()),
    };

    match pack::install(name) {
        Ok(()) => tool_ok(format!("Pack '{name}' installed.")),
        Err(e) => tool_err(format!("install error: {e}")),
    }
}

fn tool_pack_set(args: &Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_err("missing required parameter: name".into()),
    };

    if let Err(e) = pack::load_manifest(name) {
        return tool_err(format!("error: {e}"));
    }

    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    match db::set_preference(&conn, "pack", name) {
        Ok(()) => tool_ok(format!("Active pack set to '{name}'.")),
        Err(e) => tool_err(format!("error: {e}")),
    }
}

fn tool_pack_play(args: &Value) -> ToolResult {
    let category = args
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("greeting");

    let pack_name = match args.get("pack").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            let conn = match db::open() {
                Ok(c) => c,
                Err(e) => return tool_err(format!("database error: {e}")),
            };
            db::get_preferences(&conn)
                .ok()
                .and_then(|p| p.pack)
                .unwrap_or_default()
        }
    };

    if pack_name.is_empty() {
        return tool_err(
            "No active pack. Set one with vox_pack_set or pass the 'pack' parameter.".into(),
        );
    }

    match pack::play(&pack_name, Some(category)) {
        Ok(line) => tool_ok(format!("[{pack_name}/{category}] {line}")),
        Err(e) => tool_err(format!("play error: {e}")),
    }
}

fn tool_pack_remove(args: &Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_err("missing required parameter: name".into()),
    };

    match pack::remove(name) {
        Ok(true) => {
            // Clear active pack if it was the removed one
            if let Ok(conn) = db::open()
                && let Ok(prefs) = db::get_preferences(&conn)
                && prefs.pack.as_deref() == Some(name)
            {
                let _ = db::set_preference(&conn, "pack", "");
            }
            tool_ok(format!("Pack '{name}' removed."))
        }
        Ok(false) => tool_err(format!("Pack '{name}' not found.")),
        Err(e) => tool_err(format!("error: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Speech-to-text tool
// ---------------------------------------------------------------------------

fn tool_hear(args: &Value) -> ToolResult {
    let lang = args
        .get("lang")
        .and_then(|v| v.as_str())
        .filter(|l| !l.trim().is_empty());
    let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30) as f64;
    let silence_secs = args.get("silence").and_then(|v| v.as_f64()).unwrap_or(2.0);
    let model = args.get("model").and_then(|v| v.as_str());
    let file = args.get("file").and_then(|v| v.as_str());

    let recorded = match file {
        Some(path) => mic::read_wav_16k(std::path::Path::new(path)),
        None => mic::record(&mic::RecordOptions::until_silence(silence_secs, timeout)),
    };
    let samples = match recorded {
        Ok(s) => s,
        Err(e) => return tool_err(format!("Failed to get audio: {e}")),
    };
    if samples.len() < mic::TARGET_RATE as usize / 4 {
        return tool_ok("(silence — no speech detected)".into());
    }

    match stt::transcribe_samples_with(&samples, lang, model) {
        Ok(t) if t.is_empty() => tool_ok("(no speech detected)".into()),
        Ok(t) => tool_ok(t),
        Err(e) => tool_err(format!("Transcription failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_default_to_user_language_when_unset() {
        let instructions = vox_instructions(None);
        assert!(instructions.contains("Match the language the user is writing in"));
        assert!(!instructions.contains("Use French by default"));
    }

    #[test]
    fn instructions_tell_the_agent_about_the_claude_code_plugin() {
        // An agent cannot see a plugin that is not installed: the server is
        // the one place that reaches it in every session.
        for lang in [None, Some("fr")] {
            let instructions = vox_instructions(lang);
            assert!(instructions.contains("CLAUDE CODE PLUGIN:"));
            for command in crate::init::PLUGIN_INSTALL_COMMANDS {
                assert!(instructions.contains(command), "{command}");
            }
            assert!(!instructions.contains("{plugin_note}"));
        }
    }

    /// The stored model preference applies over MCP as on the command line:
    /// it used to be dropped, so `vox config set model` only reached the CLI.
    #[test]
    fn speak_request_uses_the_stored_model() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(speak_request(&conn, &json!({})).opts.model, None);

        db::set_preference(&conn, "model", "Qwen/Qwen3-TTS-12Hz-1.7B-Base").unwrap();
        let request = speak_request(&conn, &json!({"text": "hi"}));
        assert_eq!(
            request.opts.model.as_deref(),
            Some("Qwen/Qwen3-TTS-12Hz-1.7B-Base")
        );
    }

    /// The `backend` argument is respected as given. A clone used to force
    /// qwen-native over it, and nothing but qwen-native could ever clone.
    #[test]
    fn speak_request_follows_the_shared_backend_rule() {
        let conn = db::open_in_memory().unwrap();
        db::add_clone(&conn, "me", "/clones/me.wav", Some("hello")).unwrap();

        let named = speak_request(&conn, &json!({"voice": "me", "backend": "pocket"}));
        assert_eq!(named.backend, "pocket");
        assert_eq!(named.opts.ref_audio.as_deref(), Some("/clones/me.wav"));
        assert_eq!(named.opts.ref_text.as_deref(), Some("hello"));
        assert_eq!(named.opts.voice, None, "a clone name is not a voice name");
        assert_eq!(named.ignored_clone, None);

        // Unnamed, it is the rule the command line uses, whatever this
        // machine's HF_TOKEN says.
        let unnamed = speak_request(&conn, &json!({"voice": "me"}));
        assert_eq!(
            unnamed.backend,
            clone::speak_backend(None, None, None, true, clone::pocket_can_clone())
        );

        // A backend that cannot clone is still respected, and reported.
        let piper = speak_request(&conn, &json!({"voice": "me", "backend": "piper"}));
        assert_eq!(piper.backend, "piper");
        assert_eq!(piper.ignored_clone.as_deref(), Some("me"));

        // No clone: argument, then preference, then the language default.
        db::set_preference(&conn, "lang", "fr").unwrap();
        assert_eq!(speak_request(&conn, &json!({})).backend, "piper");
        assert_eq!(
            speak_request(&conn, &json!({"backend": "pocket"})).backend,
            "pocket"
        );
    }

    #[test]
    fn truncate_for_echo_never_panics_on_multibyte() {
        // A byte-index slice at 77 landed mid-character here and aborted the
        // whole server. By characters it is short, so it comes back intact.
        let ja =
            "終わりました。認証モジュールのリファクタリングとユニットテストの修正が完了しました。";
        assert!(ja.len() > 80, "must be long in bytes");
        assert!(ja.chars().count() < 80, "but short in characters");
        assert_eq!(truncate_for_echo(ja), ja);

        // Genuinely long multi-byte text is truncated on a character boundary.
        let long_ja = ja.repeat(3);
        let out = truncate_for_echo(&long_ja);
        assert!(out.ends_with("..."));
        assert_eq!(out.chars().count(), 80);
        assert!(long_ja.starts_with(out.trim_end_matches('.')));

        for text in [
            "Les dépendances vulnérables ont été mises à jour et la CI est de nouveau verte partout, enfin.",
            "완료했습니다. 인증 모듈의 리팩터링과 단위 테스트 수정이 모두 끝났습니다. 배포 준비가 되었습니다.",
            "🎉 ✅ 🚀 ".repeat(40).as_str(),
        ] {
            let _ = truncate_for_echo(text);
        }
    }

    #[test]
    fn truncate_for_echo_leaves_short_text_alone() {
        assert_eq!(truncate_for_echo("Terminé."), "Terminé.");
        let exactly_80: String = "é".repeat(80);
        assert_eq!(truncate_for_echo(&exactly_80), exactly_80);
    }

    #[test]
    fn instructions_follow_configured_language() {
        assert!(vox_instructions(Some("fr")).contains("Speak in the user's language (fr)"));
        assert!(vox_instructions(Some("es")).contains("Speak in the user's language (es)"));
        assert!(vox_instructions(Some("ja")).contains("Speak in the user's language (ja)"));
    }
}
