//! Auto-configuration for 14 AI tools (Claude Code, Cursor, VS Code, Zed, etc.).
//!
//! `vox init` injects MCP server config into the settings file of each tool
//! found on the machine. Idempotent — safe to run multiple times without
//! duplicating entries.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

const VOX_START: &str = "<!-- vox:start -->";
const VOX_END: &str = "<!-- vox:end -->";

/// What a user types in a Claude Code session to get the vox plugin
/// (`plugins/vox`, listed by `.claude-plugin/marketplace.json`).
pub const PLUGIN_INSTALL_COMMANDS: [&str; 2] = [
    "/plugin marketplace add rtk-ai/vox",
    "/plugin install vox@vox",
];

/// Tells the agent that the Claude Code plugin exists and how the user gets
/// it. An agent cannot discover a plugin that is not installed, so every text
/// vox hands to an agent carries this.
pub fn plugin_note() -> String {
    let [add, install] = PLUGIN_INSTALL_COMMANDS;
    format!(
        "vox has a Claude Code plugin that shows a live voice visualizer above the prompt \
         while it speaks. If the user asks about it, tell them to type `{add}` and then \
         `{install}` (Claude Code 2.1.287 or later)."
    )
}

pub struct InitResult {
    pub claude_md_written: bool,
    pub settings_written: bool,
}

/// The full markdown block written into a fresh CLAUDE.md.
///
/// `lang` is the resolved language. When it is `None` we do not pass `-l` and
/// ask the agent to follow the user's own language instead of forcing one.
pub fn claude_md_block(lang: Option<&str>) -> String {
    let (command, guideline) = speak_example(lang);
    let plugin = plugin_note();
    format!(
        r#"{VOX_START}
## Voice feedback (vox)

This project uses [vox](https://github.com/rtk-ai/vox) for spoken feedback.

After completing a significant task (feature, bug fix, refactor), provide an oral summary:

```bash
{command}
```

Keep summaries under 2 sentences. {guideline}
Do not speak for trivial operations (formatting, single-line fixes).

{plugin}
{VOX_END}"#
    )
}

/// Returns a short block to append to an existing CLAUDE.md.
pub fn claude_md_append_block(lang: Option<&str>) -> String {
    let (command, guideline) = speak_example(lang);
    let plugin = plugin_note();
    format!(
        r#"{VOX_START}
After completing a significant task, call `{command}` to give a spoken summary. {guideline}
{plugin}
{VOX_END}"#
    )
}

/// The example `vox` invocation and the language guideline for a given language.
fn speak_example(lang: Option<&str>) -> (String, String) {
    match lang.and_then(|code| crate::lang::lang_name(code).map(|name| (code, name))) {
        Some((code, name)) => (
            format!(r#"vox -l {code} "Short summary of what was done, in {name}""#),
            format!("Speak {name}."),
        ),
        None => (
            r#"vox "Short summary of what was done""#.to_string(),
            "Match the language the user is writing in.".to_string(),
        ),
    }
}

/// Checks whether the CLAUDE.md content already contains vox markers.
pub fn claude_md_has_vox(content: &str) -> bool {
    content.contains(VOX_START)
}

/// Checks whether a parsed settings.json already has a vox hook.
pub fn has_vox_hook(settings: &Value) -> bool {
    if let Some(hooks) = settings.get("hooks")
        && let Some(stop) = hooks.get("Stop")
        && let Some(arr) = stop.as_array()
    {
        for entry in arr {
            if let Some(inner_hooks) = entry.get("hooks")
                && let Some(inner_arr) = inner_hooks.as_array()
            {
                for h in inner_arr {
                    if let Some(cmd) = h.get("command").and_then(|c| c.as_str())
                        && cmd.starts_with("vox ")
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// The Stop-hook command line: a short "done" phrase in the resolved language.
pub fn stop_hook_command(lang: Option<&str>) -> String {
    let phrase = crate::lang::done_phrase(lang);
    match lang {
        Some(code) => format!(r#"vox -l {code} "{phrase}""#),
        None => format!(r#"vox "{phrase}""#),
    }
}

/// Builds the settings.json content, merging with existing content if provided.
pub fn build_settings(existing: Option<&str>, lang: Option<&str>) -> Result<String> {
    let vox_hook = serde_json::json!({
        "hooks": {
            "Stop": [
                {
                    "matcher": "",
                    "hooks": [
                        { "type": "command", "command": stop_hook_command(lang) }
                    ]
                }
            ]
        }
    });

    let merged = match existing {
        Some(content) => {
            let mut base: Value =
                serde_json::from_str(content).context("Invalid JSON in settings.json")?;

            if has_vox_hook(&base) {
                return serde_json::to_string_pretty(&base).context("Failed to serialize settings");
            }

            // Merge hooks
            if let Some(new_hooks) = vox_hook.get("hooks")
                && let Some(new_stop) = new_hooks.get("Stop")
            {
                let base_obj = base
                    .as_object_mut()
                    .context("settings.json is not an object")?;
                let hooks_obj = base_obj
                    .entry("hooks")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                let hooks_map = hooks_obj
                    .as_object_mut()
                    .context("hooks is not an object")?;

                let stop_arr = hooks_map
                    .entry("Stop")
                    .or_insert_with(|| Value::Array(vec![]));
                if let Some(arr) = stop_arr.as_array_mut()
                    && let Some(new_entries) = new_stop.as_array()
                {
                    arr.extend(new_entries.clone());
                }
            }

            base
        }
        None => vox_hook,
    };

    serde_json::to_string_pretty(&merged).context("Failed to serialize settings")
}

/// Orchestrates the full init: writes CLAUDE.md and .claude/settings.json.
pub fn run_init(project_dir: &Path, lang: Option<&str>) -> Result<InitResult> {
    let mut result = InitResult {
        claude_md_written: false,
        settings_written: false,
    };

    // --- CLAUDE.md ---
    let claude_md_path = project_dir.join("CLAUDE.md");
    if claude_md_path.exists() {
        let content = fs::read_to_string(&claude_md_path).context("Failed to read CLAUDE.md")?;
        if !claude_md_has_vox(&content) {
            let new_content = format!(
                "{}\n\n{}\n",
                content.trim_end(),
                claude_md_append_block(lang)
            );
            fs::write(&claude_md_path, new_content).context("Failed to write CLAUDE.md")?;
            result.claude_md_written = true;
        }
    } else {
        fs::write(&claude_md_path, format!("{}\n", claude_md_block(lang)))
            .context("Failed to create CLAUDE.md")?;
        result.claude_md_written = true;
    }

    // --- .claude/settings.json ---
    let claude_dir = project_dir.join(".claude");
    let settings_path = claude_dir.join("settings.json");

    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path).context("Failed to read settings.json")?;
        let parsed: Value =
            serde_json::from_str(&content).context("Invalid JSON in settings.json")?;

        if !has_vox_hook(&parsed) {
            let new_content = build_settings(Some(&content), lang)?;
            fs::write(&settings_path, format!("{}\n", new_content))
                .context("Failed to write settings.json")?;
            result.settings_written = true;
        }
    } else {
        fs::create_dir_all(&claude_dir).context("Failed to create .claude directory")?;
        let content = build_settings(None, lang)?;
        fs::write(&settings_path, format!("{}\n", content))
            .context("Failed to create settings.json")?;
        result.settings_written = true;
    }

    Ok(result)
}

const CONFIGURED: &str = "configured";
const ALREADY_CONFIGURED: &str = "already configured";
const NOT_INSTALLED: &str = "not installed, skipped";

/// How a tool's configuration file names its MCP servers.
#[derive(Clone, Copy)]
enum McpFormat {
    /// A JSON object under this top-level key: `mcpServers` for most tools,
    /// `servers` for VS Code, `mcp` for OpenCode.
    Json(&'static str),
    /// Zed `settings.json`.
    Zed,
    /// Codex `config.toml`.
    Codex,
}

/// One AI tool `vox init` can register the MCP server with.
pub struct McpTarget {
    pub label: &'static str,
    /// The file the server entry goes into.
    pub config: PathBuf,
    /// A directory the tool makes for itself. vox never creates it: it is how
    /// a tool that is installed is told from one that is not.
    pub data_dir: PathBuf,
    format: McpFormat,
}

impl McpTarget {
    fn in_dir(label: &'static str, data_dir: PathBuf, file: &str, format: McpFormat) -> Self {
        Self {
            label,
            config: data_dir.join(file),
            data_dir,
            format,
        }
    }

    /// Whether the tool is on this machine: its configuration file is there,
    /// or the directory it keeps its own data in.
    pub fn is_installed(&self) -> bool {
        self.config.is_file() || self.data_dir.is_dir()
    }
}

/// What `vox init` did for one tool.
pub struct McpReport {
    pub label: &'static str,
    /// As printed: configured, already configured, not installed, or the error.
    pub status: String,
    pub installed: bool,
    /// The entry was written by this run, so the tool has to be restarted.
    pub newly_configured: bool,
}

/// Where desktop applications keep their settings on this platform: the
/// parent of Claude Desktop's and VS Code's directories.
pub fn app_config_dir(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support")
    }
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir().unwrap_or_else(|| home.join("AppData/Roaming"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        home.join(".config")
    }
}

/// The 14 tools, with the place each one reads its MCP servers from.
pub fn mcp_targets(home: &Path, app_config: &Path) -> Vec<McpTarget> {
    let servers = McpFormat::Json("mcpServers");
    let vscode_user = app_config.join("Code/User");
    // Cline and its forks are VS Code extensions: each keeps its settings in
    // its own directory, which exists once the extension has run.
    let extension = |label, id: &str| McpTarget {
        label,
        config: vscode_user
            .join("globalStorage")
            .join(id)
            .join("settings/cline_mcp_settings.json"),
        data_dir: vscode_user.join("globalStorage").join(id),
        format: servers,
    };

    vec![
        // The one tool whose file sits in the home directory itself, so its
        // own directory is `~/.claude`.
        McpTarget {
            label: "Claude Code",
            config: home.join(".claude.json"),
            data_dir: home.join(".claude"),
            format: servers,
        },
        McpTarget::in_dir(
            "Claude Desktop",
            app_config.join("Claude"),
            "claude_desktop_config.json",
            servers,
        ),
        McpTarget::in_dir("Cursor", home.join(".cursor"), "mcp.json", servers),
        McpTarget::in_dir(
            "Windsurf",
            home.join(".codeium/windsurf"),
            "mcp_config.json",
            servers,
        ),
        McpTarget::in_dir(
            "VS Code / Copilot",
            vscode_user.clone(),
            "mcp.json",
            McpFormat::Json("servers"),
        ),
        McpTarget::in_dir(
            "Zed",
            home.join(".config/zed"),
            "settings.json",
            McpFormat::Zed,
        ),
        McpTarget::in_dir(
            "Codex",
            home.join(".codex"),
            "config.toml",
            McpFormat::Codex,
        ),
        McpTarget::in_dir(
            "OpenCode",
            home.join(".config/opencode"),
            "opencode.json",
            McpFormat::Json("mcp"),
        ),
        McpTarget::in_dir("Gemini", home.join(".gemini"), "settings.json", servers),
        McpTarget::in_dir("Amazon Q", home.join(".aws/amazonq"), "mcp.json", servers),
        extension("Cline", "saoudrizwan.claude-dev"),
        extension("Roo Code", "rooveterinaryinc.roo-cline"),
        extension("Kilo Code", "kilocode.kilo-code"),
        McpTarget::in_dir("Amp", home.join(".ampcode"), "settings.json", servers),
    ]
}

/// Register `vox serve` with every tool found under `home`, and report on
/// all 14. A tool that is not installed is left alone: writing its file would
/// create the directories of an application that is not there.
pub fn configure_mcp(home: &Path, app_config: &Path, vox_bin: &str) -> Vec<McpReport> {
    let entry = serde_json::json!({
        "command": vox_bin,
        "args": ["serve"],
        "env": {}
    });

    mcp_targets(home, app_config)
        .into_iter()
        .map(|target| {
            let installed = target.is_installed();
            let status = if installed {
                let written = match target.format {
                    McpFormat::Json(key) => inject_mcp_json(&target.config, key, "vox", &entry),
                    McpFormat::Zed => inject_zed_mcp(&target.config, "vox", vox_bin),
                    McpFormat::Codex => inject_codex_mcp(&target.config, "vox", vox_bin),
                };
                written.unwrap_or_else(|e| format!("error: {e}"))
            } else {
                NOT_INSTALLED.to_string()
            };
            McpReport {
                label: target.label,
                installed,
                newly_configured: status == CONFIGURED,
                status,
            }
        })
        .collect()
}

/// Inject an MCP server into a JSON config with a configurable top-level key.
/// Works for: Claude (`mcpServers`), Cursor (`mcpServers`), Windsurf (`mcpServers`),
/// OpenCode (`mcp`).
pub fn inject_mcp_json(
    config_path: &PathBuf,
    top_key: &str,
    name: &str,
    entry: &Value,
) -> Result<String> {
    let mut config: Value = if config_path.exists() {
        let content = fs::read_to_string(config_path)
            .with_context(|| format!("cannot read {}", config_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("invalid JSON in {}", config_path.display()))?
    } else {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        serde_json::json!({})
    };

    let servers = config
        .as_object_mut()
        .context("config is not a JSON object")?
        .entry(top_key)
        .or_insert_with(|| serde_json::json!({}));

    if let Some(existing) = servers.get(name)
        && existing.get("command").and_then(|v| v.as_str())
            == entry.get("command").and_then(|v| v.as_str())
    {
        return Ok(ALREADY_CONFIGURED.into());
    }

    servers
        .as_object_mut()
        .unwrap()
        .insert(name.to_string(), entry.clone());

    let output = serde_json::to_string_pretty(&config)?;
    fs::write(config_path, output)
        .with_context(|| format!("cannot write {}", config_path.display()))?;

    Ok(CONFIGURED.into())
}

/// Shorthand for Claude/Cursor/Windsurf style (`mcpServers` key).
pub fn inject_mcp_server(config_path: &PathBuf, name: &str, entry: &Value) -> Result<String> {
    inject_mcp_json(config_path, "mcpServers", name, entry)
}

/// Inject into VS Code / Copilot `.vscode/mcp.json` (uses `servers` key, no `env` wrapper).
pub fn inject_vscode_mcp(config_path: &PathBuf, name: &str, entry: &Value) -> Result<String> {
    inject_mcp_json(config_path, "servers", name, entry)
}

/// Inject into Zed `settings.json` (uses `context_servers` with nested `command` object).
pub fn inject_zed_mcp(config_path: &PathBuf, name: &str, command: &str) -> Result<String> {
    let mut config: Value = if config_path.exists() {
        let content = fs::read_to_string(config_path)
            .with_context(|| format!("cannot read {}", config_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("invalid JSON in {}", config_path.display()))?
    } else {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        serde_json::json!({})
    };

    let servers = config
        .as_object_mut()
        .context("config is not a JSON object")?
        .entry("context_servers")
        .or_insert_with(|| serde_json::json!({}));

    if servers.get(name).is_some() {
        return Ok(ALREADY_CONFIGURED.into());
    }

    let zed_entry = serde_json::json!({
        "command": {
            "path": command,
            "args": ["serve"],
            "env": {}
        },
        "settings": {}
    });

    servers
        .as_object_mut()
        .unwrap()
        .insert(name.to_string(), zed_entry);

    let output = serde_json::to_string_pretty(&config)?;
    fs::write(config_path, output)
        .with_context(|| format!("cannot write {}", config_path.display()))?;

    Ok(CONFIGURED.into())
}

/// Inject into Codex `config.toml` (TOML format).
pub fn inject_codex_mcp(config_path: &PathBuf, name: &str, command: &str) -> Result<String> {
    let content = if config_path.exists() {
        fs::read_to_string(config_path)
            .with_context(|| format!("cannot read {}", config_path.display()))?
    } else {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        String::new()
    };

    let section_header = format!("[mcp_servers.{name}]");
    if content.contains(&section_header) {
        return Ok(ALREADY_CONFIGURED.into());
    }

    let toml_block = format!("\n{section_header}\ncommand = \"{command}\"\nargs = [\"serve\"]\n");

    let new_content = format!("{}{}", content.trim_end(), toml_block);
    fs::write(config_path, format!("{new_content}\n"))
        .with_context(|| format!("cannot write {}", config_path.display()))?;

    Ok(CONFIGURED.into())
}

/// Inject into OpenCode `opencode.json` (uses `mcp` key with `command`+`args`).
pub fn inject_opencode_mcp(config_path: &PathBuf, name: &str, entry: &Value) -> Result<String> {
    inject_mcp_json(config_path, "mcp", name, entry)
}
