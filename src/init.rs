//! Auto-configuration for 14 AI tools (Claude Code, Cursor, VS Code, Zed, etc.).
//!
//! `vox init` injects MCP server config into each tool's settings file.
//! Idempotent — safe to run multiple times without duplicating entries.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

const VOX_START: &str = "<!-- vox:start -->";
const VOX_END: &str = "<!-- vox:end -->";

pub struct InitResult {
    pub claude_md_written: bool,
    pub settings_written: bool,
}

/// Returns the full markdown block for a new CLAUDE.md.
pub fn claude_md_block() -> String {
    format!(
        r#"{VOX_START}
## Voice feedback (vox)

This project uses [vox](https://github.com/rtk-ai/vox) for spoken feedback.

After completing a significant task (feature, bug fix, refactor), provide an oral summary:

```bash
vox -l fr "Short summary of what was done, in French"
```

Keep summaries under 2 sentences. Use French.
Do not speak for trivial operations (formatting, single-line fixes).
{VOX_END}"#
    )
}

/// Returns a short block to append to an existing CLAUDE.md.
pub fn claude_md_append_block() -> String {
    format!(
        r#"{VOX_START}
After completing a significant task, call `vox -l fr "résumé en français"` to give a spoken summary.
{VOX_END}"#
    )
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

/// Builds the settings.json content, merging with existing content if provided.
pub fn build_settings(existing: Option<&str>) -> Result<String> {
    let vox_hook: Value = serde_json::from_str(
        r#"{
  "hooks": {
    "Stop": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "vox -l fr \"Terminé.\""
          }
        ]
      }
    ]
  }
}"#,
    )?;

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
pub fn run_init(project_dir: &Path) -> Result<InitResult> {
    let mut result = InitResult {
        claude_md_written: false,
        settings_written: false,
    };

    // --- CLAUDE.md ---
    let claude_md_path = project_dir.join("CLAUDE.md");
    if claude_md_path.exists() {
        let content = fs::read_to_string(&claude_md_path).context("Failed to read CLAUDE.md")?;
        if !claude_md_has_vox(&content) {
            let new_content = format!("{}\n\n{}\n", content.trim_end(), claude_md_append_block());
            fs::write(&claude_md_path, new_content).context("Failed to write CLAUDE.md")?;
            result.claude_md_written = true;
        }
    } else {
        fs::write(&claude_md_path, format!("{}\n", claude_md_block()))
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
            let new_content = build_settings(Some(&content))?;
            fs::write(&settings_path, format!("{}\n", new_content))
                .context("Failed to write settings.json")?;
            result.settings_written = true;
        }
    } else {
        fs::create_dir_all(&claude_dir).context("Failed to create .claude directory")?;
        let content = build_settings(None)?;
        fs::write(&settings_path, format!("{}\n", content))
            .context("Failed to create settings.json")?;
        result.settings_written = true;
    }

    Ok(result)
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
        return Ok("already configured".into());
    }

    servers
        .as_object_mut()
        .unwrap()
        .insert(name.to_string(), entry.clone());

    let output = serde_json::to_string_pretty(&config)?;
    fs::write(config_path, output)
        .with_context(|| format!("cannot write {}", config_path.display()))?;

    Ok("configured".into())
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
        return Ok("already configured".into());
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

    Ok("configured".into())
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
        return Ok("already configured".into());
    }

    let toml_block = format!("\n{section_header}\ncommand = \"{command}\"\nargs = [\"serve\"]\n");

    let new_content = format!("{}{}", content.trim_end(), toml_block);
    fs::write(config_path, format!("{new_content}\n"))
        .with_context(|| format!("cannot write {}", config_path.display()))?;

    Ok("configured".into())
}

/// Inject into OpenCode `opencode.json` (uses `mcp` key with `command`+`args`).
pub fn inject_opencode_mcp(config_path: &PathBuf, name: &str, entry: &Value) -> Result<String> {
    inject_mcp_json(config_path, "mcp", name, entry)
}
