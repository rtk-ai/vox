//! MCP (Model Context Protocol) server — JSON-RPC 2.0 over stdio.
//!
//! Exposes 14 tools for AI assistants: speak, hear, voice cloning, config, stats, packs.
//! Launched via `vox serve` and auto-configured by `vox init` for 14 AI tools.

use std::io::{self, BufRead, Write};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::{self, SpeakOptions};
use crate::clone;
use crate::db;
use crate::pack;
#[cfg(target_os = "macos")]
use crate::stt;

const SERVER_NAME: &str = "vox";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";

const VOX_INSTRUCTIONS: &str = "\
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
- Use French by default (the user prefers it)\n\
- Use vox_config_show to check the user's preferred voice/backend before first use\n\
- For longer explanations, use vox_speak with a concise summary, not the full text\n\
\n\
VOICE CONVERSATION (vox_hear + vox_speak):\n\
You can have a voice conversation with the user — you ARE the brain, no API key needed.\n\
Loop: vox_hear (listen) → you think → vox_speak (respond). Repeat until the user says goodbye.\n\
When the user asks to \"chat\" or \"talk\" or \"parler\", start this loop.";

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
            "instructions": VOX_INSTRUCTIONS
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

fn tool_definitions() -> Value {
    json!([
        {
            "name": "vox_speak",
            "description": "Read text aloud using text-to-speech. Supports multiple backends, voices, languages, and styles.",
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
                        "description": "TTS backend: kokoro, say (macOS), qwen (macOS), qwen-native, voxtream (fastest, zero-shot)"
                    },
                    "style": {
                        "type": "string",
                        "description": "Intonation style: calm, energetic, warm, authoritative, cheerful, serious"
                    },
                    "gender": {
                        "type": "string",
                        "description": "Gender hint: feminine, masculine"
                    },
                    "rate": {
                        "type": "integer",
                        "description": "Speech rate in words per minute (say backend only)"
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
                        "description": "TTS backend: kokoro, say, qwen, qwen-native, voxtream"
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
                        "description": "Path to the reference audio file"
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
            "description": "Show current vox preferences (backend, voice, language, rate, style).",
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
                        "description": "Preference key: backend, voice, lang, rate, gender, style, model"
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
            "description": "Record audio from the microphone and transcribe it to text (speech-to-text). Recording starts when voice is detected and stops automatically after silence. Use this with vox_speak to create a voice conversation loop — Claude Code is the brain, no API key needed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "lang": {
                        "type": "string",
                        "description": "Language code for transcription: en, fr, es, de, etc. (default: fr)"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Maximum recording duration in seconds (default: 30)"
                    },
                    "silence": {
                        "type": "number",
                        "description": "Seconds of silence before stopping (default: 2.0)"
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

fn tool_speak(args: &Value) -> ToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return tool_err("missing required parameter: text".into()),
    };

    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };
    let prefs = db::get_preferences(&conn).unwrap_or_default();

    // Merge MCP args > DB preferences > defaults
    let backend_name = args
        .get("backend")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or(prefs.backend)
        .unwrap_or_else(|| crate::config::DEFAULT_BACKEND.to_string());

    let mut voice = args
        .get("voice")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or(prefs.voice);
    let lang = args
        .get("lang")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or(prefs.lang);
    let rate = args
        .get("rate")
        .and_then(|v| v.as_u64())
        .map(|r| r as u32)
        .or(prefs.rate);
    let gender = args
        .get("gender")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or(prefs.gender);
    let style = args
        .get("style")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or(prefs.style);
    let volume = args
        .get("volume")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(1.0)
        .clamp(0.0, 5.0);

    // Resolve voice clone
    let mut ref_audio = None;
    let mut ref_text = None;
    let mut effective_backend = backend_name;

    if let Some(ref voice_name) = voice
        && let Ok(Some(vc)) = clone::resolve_voice(&conn, voice_name)
    {
        ref_audio = Some(vc.ref_audio);
        ref_text = vc.ref_text;
        if effective_backend != "qwen" && effective_backend != "qwen-native" {
            #[cfg(target_os = "macos")]
            {
                effective_backend = "qwen".to_string();
            }
            #[cfg(not(target_os = "macos"))]
            {
                effective_backend = "qwen-native".to_string();
            }
        }
        voice = None;
    }

    let bk = match backend::get_backend(&effective_backend) {
        Ok(b) => b,
        Err(e) => return tool_err(format!("backend error: {e}")),
    };

    let opts = SpeakOptions {
        voice,
        lang: lang.clone(),
        rate,
        gender,
        style,
        ref_audio,
        ref_text,
        model: None,
        volume,
    };

    let start = std::time::Instant::now();
    if let Err(e) = bk.speak(text, &opts) {
        return tool_err(format!("speak error: {e}"));
    }
    let duration_ms = start.elapsed().as_millis() as u64;

    let _ = db::log_usage(
        &conn,
        &effective_backend,
        opts.voice.as_deref(),
        opts.lang.as_deref(),
        text.len(),
        Some(duration_ms),
    );

    tool_ok(format!(
        "Spoken: \"{}\" ({duration_ms}ms, {effective_backend})",
        if text.len() > 80 {
            format!("{}...", &text[..77])
        } else {
            text.to_string()
        }
    ))
}

fn tool_list_voices(args: &Value) -> ToolResult {
    let backend_name = args
        .get("backend")
        .and_then(|v| v.as_str())
        .unwrap_or(crate::config::DEFAULT_BACKEND);

    let bk = match backend::get_backend(backend_name) {
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

    if let Err(e) = clone::validate_audio(audio) {
        return tool_err(format!("invalid audio: {e}"));
    }

    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => return tool_err(format!("database error: {e}")),
    };

    match db::add_clone(&conn, name, audio, text) {
        Ok(_) => tool_ok(format!("Voice clone '{name}' added.")),
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

    match db::remove_clone(&conn, name) {
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
            let lines = [
                format!(
                    "backend: {}",
                    prefs.backend.as_deref().unwrap_or("(default)")
                ),
                format!("voice:   {}", prefs.voice.as_deref().unwrap_or("(default)")),
                format!("lang:    {}", prefs.lang.as_deref().unwrap_or("(default)")),
                format!(
                    "rate:    {}",
                    prefs
                        .rate
                        .map(|r| r.to_string())
                        .as_deref()
                        .unwrap_or("(default)")
                ),
                format!(
                    "gender:  {}",
                    prefs.gender.as_deref().unwrap_or("(default)")
                ),
                format!("style:   {}", prefs.style.as_deref().unwrap_or("(default)")),
                format!("model:   {}", prefs.model.as_deref().unwrap_or("(default)")),
                format!("pack:    {}", prefs.pack.as_deref().unwrap_or("(none)")),
            ];
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
    #[cfg(not(target_os = "macos"))]
    {
        let _ = args;
        tool_err(
            "vox_hear requires macOS (mlx-audio STT). Linux/Windows support coming soon.".into(),
        )
    }

    #[cfg(target_os = "macos")]
    {
        let lang = args.get("lang").and_then(|v| v.as_str()).unwrap_or("fr");
        let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30) as u32;
        let silence_secs = args.get("silence").and_then(|v| v.as_f64()).unwrap_or(2.0);

        let tmp_dir = std::env::temp_dir();
        let audio_path = tmp_dir.join("vox_hear_input.wav");
        let audio_str = audio_path.to_string_lossy().to_string();

        // Record with silence detection using sox `rec`.
        // silence 1 0.1 1% = skip leading silence (start on voice)
        // 1 <silence_secs> 1% = stop after N seconds of silence
        // trim 0 <timeout> = safety max duration
        let status = std::process::Command::new("rec")
            .arg(&audio_str)
            .arg("rate")
            .arg("16k")
            .arg("silence")
            .arg("1")
            .arg("0.1")
            .arg("1%")
            .arg("1")
            .arg(format!("{silence_secs}"))
            .arg("1%")
            .arg("trim")
            .arg("0")
            .arg(timeout.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        let status = match status {
            Ok(s) => s,
            Err(e) => {
                return tool_err(format!(
                    "Failed to record audio: {e}. {}",
                    clone::sox_install_hint()
                ));
            }
        };

        if !status.success() {
            return tool_err("Recording failed (sox exited with error)".into());
        }

        // Check that the file exists and has content
        match std::fs::metadata(&audio_path) {
            Ok(m) if m.len() < 1000 => {
                let _ = std::fs::remove_file(&audio_path);
                return tool_ok("(silence — no speech detected)".into());
            }
            Err(_) => {
                return tool_err("Recording file not found".into());
            }
            _ => {}
        }

        // Transcribe
        let text = match stt::transcribe(&audio_str, Some(lang)) {
            Ok(t) => t,
            Err(e) => {
                let _ = std::fs::remove_file(&audio_path);
                return tool_err(format!("Transcription failed: {e}"));
            }
        };

        let _ = std::fs::remove_file(&audio_path);

        if text.is_empty() {
            tool_ok("(no speech detected)".into())
        } else {
            tool_ok(text)
        }
    }
}
