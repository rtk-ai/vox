use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use vox::backend::{self, SpeakOptions};
use vox::config::DEFAULT_BACKEND;
use vox::{clone, db, init, input, mcp, pack};

#[derive(Parser)]
#[command(name = "vox", version, about = "Voice Command — read text aloud")]
struct Cli {
    /// Text to speak (when no subcommand is used)
    text: Vec<String>,

    /// TTS backend (say, qwen, qwen-native)
    #[arg(short = 'b', long, default_value = DEFAULT_BACKEND)]
    backend: String,

    /// Voice name (or clone name)
    #[arg(short = 'v', long)]
    voice: Option<String>,

    /// Language code (for qwen backend)
    #[arg(short = 'l', long)]
    lang: Option<String>,

    /// Speech rate (words per minute, for say backend)
    #[arg(short = 'r', long)]
    rate: Option<u32>,

    /// Gender hint (feminine, masculine)
    #[arg(long)]
    gender: Option<String>,

    /// Intonation style (calm, energetic, warm, authoritative, cheerful, serious)
    #[arg(long)]
    style: Option<String>,

    /// TTS model (e.g. mlx-community/Qwen3-TTS-12Hz-0.6B-Base-4bit for faster inference)
    #[arg(short = 'm', long)]
    model: Option<String>,

    /// List available voices for the selected backend
    #[arg(long)]
    list_voices: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage voice clones
    Clone {
        #[command(subcommand)]
        action: CloneAction,
    },
    /// Manage preferences
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Show usage statistics
    Stats,
    /// Set up AI assistant integration (Claude Code + Claude Desktop)
    Init {
        /// Integration mode: mcp, cli, skill, or all (default: mcp)
        #[arg(short, long, default_value = "mcp")]
        mode: InitMode,
    },
    /// Launch MCP server (stdio transport for Claude Code / Claude Desktop)
    Serve,
    /// Manage fun sound packs (peon-ping compatible)
    Pack {
        #[command(subcommand)]
        action: PackAction,
    },
    /// Start a voice conversation with Claude (macOS only)
    #[cfg(target_os = "macos")]
    Chat {
        /// Voice clone name
        #[arg(short = 'v', long)]
        voice: Option<String>,
        /// Language code
        #[arg(short = 'l', long)]
        lang: Option<String>,
    },
}

#[derive(Subcommand)]
enum CloneAction {
    /// Add a voice clone from an audio file
    Add {
        /// Name for the voice clone
        name: String,
        /// Path to the reference audio file
        #[arg(long)]
        audio: String,
        /// Optional transcription of the reference audio
        #[arg(long)]
        text: Option<String>,
    },
    /// Record a voice clone from microphone
    Record {
        /// Name for the voice clone
        name: String,
        /// Recording duration in seconds
        #[arg(long, default_value = "10")]
        duration: u32,
        /// Optional transcription of what you'll say during recording
        #[arg(long)]
        text: Option<String>,
    },
    /// List all voice clones
    List,
    /// Remove a voice clone
    Remove {
        /// Name of the voice clone to remove
        name: String,
    },
}

#[derive(Clone, ValueEnum)]
enum InitMode {
    /// MCP server plugin (Claude calls vox tools natively)
    Mcp,
    /// CLAUDE.md instructions + Stop hook (Claude calls vox via Bash)
    Cli,
    /// Claude Code slash command /speak
    Skill,
    /// All integration modes
    All,
}

#[derive(Subcommand)]
enum PackAction {
    /// List available and installed sound packs
    List,
    /// Install a sound pack from peon-ping repository
    Install {
        /// Pack name (e.g. peon, peon_fr, sc_kerrigan)
        name: String,
    },
    /// Remove an installed sound pack
    Remove {
        /// Pack name
        name: String,
    },
    /// Set the active sound pack
    Set {
        /// Pack name
        name: String,
    },
    /// Play a random sound from the active pack (or a specific pack)
    Play {
        /// Sound category (greeting, acknowledge, complete, error, permission, annoyed)
        #[arg(default_value = "greeting")]
        category: String,
        /// Pack name (uses active pack if omitted)
        #[arg(short = 'p', long)]
        pack: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current preferences
    Show,
    /// Set a preference (backend, voice, lang, rate, gender, style, model)
    Set {
        /// Preference key
        key: String,
        /// Preference value
        value: String,
    },
    /// Reset all preferences to defaults
    Reset,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Clone { action }) => handle_clone(action),
        Some(Commands::Config { action }) => handle_config(action),
        Some(Commands::Stats) => handle_stats(),
        Some(Commands::Init { mode }) => handle_init(mode),
        Some(Commands::Serve) => mcp::run_server(),
        Some(Commands::Pack { action }) => handle_pack(action),
        #[cfg(target_os = "macos")]
        Some(Commands::Chat { voice, lang }) => handle_chat(voice, lang),
        None => handle_speak(cli),
    }
}

/// The backend to use for voice cloning (auto-detected by platform).
fn voice_clone_backend() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "qwen"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "qwen-native"
    }
}

fn handle_speak(cli: Cli) -> Result<()> {
    let conn = db::open()?;
    let prefs = db::get_preferences(&conn)?;

    // Merge: CLI flags > DB preferences > defaults
    let backend_name = if cli.backend != DEFAULT_BACKEND {
        cli.backend.clone()
    } else {
        prefs.backend.unwrap_or_else(|| cli.backend.clone())
    };

    let mut voice = cli.voice.or(prefs.voice);
    let lang = cli.lang.or(prefs.lang);
    let rate = cli.rate.or(prefs.rate);
    let gender = cli.gender.or(prefs.gender);
    let style = cli.style.or(prefs.style);
    let model = cli.model.or(prefs.model);

    // Resolve voice clone
    let mut ref_audio = None;
    let mut ref_text = None;
    let mut effective_backend = backend_name.clone();

    if let Some(ref voice_name) = voice
        && let Some(vc) = clone::resolve_voice(&conn, voice_name)?
    {
        ref_audio = Some(vc.ref_audio);
        ref_text = vc.ref_text;
        // Auto-switch to a qwen backend for voice clones (unless already on one)
        if effective_backend != "qwen" && effective_backend != "qwen-native" {
            effective_backend = voice_clone_backend().to_string();
        }
        voice = None; // don't pass clone name as --voice
    }

    let backend = backend::get_backend(&effective_backend)?;

    if cli.list_voices {
        let voices = backend.list_voices()?;
        for v in &voices {
            println!("{v}");
        }
        return Ok(());
    }

    let text = input::read_text(&cli.text)?;

    let opts = SpeakOptions {
        voice,
        lang: lang.clone(),
        rate,
        gender,
        style,
        ref_audio,
        ref_text,
        model,
    };

    let start = Instant::now();
    backend.speak(&text, &opts)?;
    let duration_ms = start.elapsed().as_millis() as u64;

    // Log usage
    let _ = db::log_usage(
        &conn,
        &effective_backend,
        opts.voice.as_deref(),
        opts.lang.as_deref(),
        text.len(),
        Some(duration_ms),
    );

    Ok(())
}

fn handle_clone(action: CloneAction) -> Result<()> {
    let conn = db::open()?;

    match action {
        CloneAction::Add { name, audio, text } => {
            clone::validate_audio(&audio)?;
            db::add_clone(&conn, &name, &audio, text.as_deref())?;
            println!("Voice clone '{name}' added.");
        }
        CloneAction::Record {
            name,
            duration,
            text,
        } => {
            let audio_path = clone::record_clone(&name, duration)?;
            db::add_clone(&conn, &name, &audio_path, text.as_deref())?;
            println!("Voice clone '{name}' recorded and saved.");
        }
        CloneAction::List => {
            let clones = db::list_clones(&conn)?;
            if clones.is_empty() {
                println!("No voice clones.");
            } else {
                for c in &clones {
                    let text_info = c
                        .ref_text
                        .as_deref()
                        .map(|t| format!(" (text: \"{t}\")"))
                        .unwrap_or_default();
                    println!(
                        "{}: {}{} [{}]",
                        c.name, c.ref_audio, text_info, c.created_at
                    );
                }
            }
        }
        CloneAction::Remove { name } => {
            if db::remove_clone(&conn, &name)? {
                println!("Voice clone '{name}' removed.");
            } else {
                println!("Voice clone '{name}' not found.");
            }
        }
    }
    Ok(())
}

fn handle_config(action: ConfigAction) -> Result<()> {
    let conn = db::open()?;

    match action {
        ConfigAction::Show => {
            let prefs = db::get_preferences(&conn)?;
            println!(
                "backend: {}",
                prefs.backend.as_deref().unwrap_or("(default)")
            );
            println!("voice:   {}", prefs.voice.as_deref().unwrap_or("(default)"));
            println!("lang:    {}", prefs.lang.as_deref().unwrap_or("(default)"));
            println!(
                "rate:    {}",
                prefs
                    .rate
                    .map(|r| r.to_string())
                    .as_deref()
                    .unwrap_or("(default)")
            );
            println!(
                "gender:  {}",
                prefs.gender.as_deref().unwrap_or("(default)")
            );
            println!("style:   {}", prefs.style.as_deref().unwrap_or("(default)"));
            println!("model:   {}", prefs.model.as_deref().unwrap_or("(default)"));
            println!("pack:    {}", prefs.pack.as_deref().unwrap_or("(none)"));
        }
        ConfigAction::Set { key, value } => {
            db::set_preference(&conn, &key, &value)?;
            println!("{key} = {value}");
        }
        ConfigAction::Reset => {
            db::reset_preferences(&conn)?;
            println!("Preferences reset to defaults.");
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn handle_chat(voice: Option<String>, lang: Option<String>) -> Result<()> {
    use vox::chat::{self, ChatConfig};

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .context("ANTHROPIC_API_KEY environment variable is required for chat mode")?;

    let conn = db::open()?;
    let prefs = db::get_preferences(&conn)?;

    let voice_name = voice.or(prefs.voice);
    let lang = lang.or(prefs.lang);

    let voice_clone = if let Some(ref name) = voice_name {
        clone::resolve_voice(&conn, name)?
    } else {
        None
    };

    let config = ChatConfig {
        voice_clone,
        lang,
        api_key,
        model: "claude-sonnet-4-20250514".to_string(),
    };

    chat::run_chat_loop(config)
}

fn handle_init(mode: InitMode) -> Result<()> {
    let do_cli = matches!(mode, InitMode::Cli | InitMode::All);
    let do_mcp = matches!(mode, InitMode::Mcp | InitMode::All);
    let do_skill = matches!(mode, InitMode::Skill | InitMode::All);

    // --- CLI mode: CLAUDE.md + Stop hook ---
    if do_cli {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        let result = init::run_init(&cwd)?;

        if result.claude_md_written {
            println!("[cli] CLAUDE.md configured with vox instructions.");
        }
        if result.settings_written {
            println!("[cli] .claude/settings.json configured with Stop hook.");
        }
        if !result.claude_md_written && !result.settings_written {
            println!("[cli] already configured.");
        }
    }

    // --- MCP mode: configure MCP server ---
    if do_mcp {
        let vox_bin = std::env::current_exe().context("cannot determine vox binary path")?;
        let vox_bin_str = vox_bin.to_string_lossy().to_string();
        let home = dirs::home_dir()
            .context("cannot determine home directory")?
            .to_string_lossy()
            .to_string();

        let mcp_entry = serde_json::json!({
            "command": vox_bin_str,
            "args": ["serve"],
            "env": {}
        });

        let home_path = std::path::PathBuf::from(&home);

        let code_path = home_path.join(".claude.json");
        let code_status = init::inject_mcp_server(&code_path, "vox", &mcp_entry)
            .unwrap_or_else(|e| format!("error: {e}"));
        println!("[mcp] Claude Code:    {code_status}");

        // Claude Desktop config path is platform-specific
        #[cfg(target_os = "macos")]
        let desktop_path =
            home_path.join("Library/Application Support/Claude/claude_desktop_config.json");
        #[cfg(target_os = "windows")]
        let desktop_path = dirs::config_dir()
            .map(|d| d.join("Claude/claude_desktop_config.json"))
            .unwrap_or_else(|| home_path.join("AppData/Roaming/Claude/claude_desktop_config.json"));
        #[cfg(target_os = "linux")]
        let desktop_path = home_path.join(".config/Claude/claude_desktop_config.json");

        let desktop_status = init::inject_mcp_server(&desktop_path, "vox", &mcp_entry)
            .unwrap_or_else(|e| format!("error: {e}"));
        println!("[mcp] Claude Desktop: {desktop_status}");
    }

    // --- Skill mode: create /speak slash command ---
    if do_skill {
        let home = dirs::home_dir()
            .context("cannot determine home directory")?
            .to_string_lossy()
            .to_string();
        let skills_dir = std::path::PathBuf::from(&home).join(".claude/commands");
        std::fs::create_dir_all(&skills_dir).ok();

        let skill_path = skills_dir.join("speak.md");
        if skill_path.exists() {
            println!("[skill] /speak already configured.");
        } else {
            std::fs::write(
                &skill_path,
                "Use vox to speak the following text aloud: $ARGUMENTS\n\
                 \n\
                 Call the vox_speak MCP tool if available, otherwise run:\n\
                 ```bash\n\
                 vox -b say \"$ARGUMENTS\"\n\
                 ```\n",
            )
            .context("cannot write skill file")?;
            println!("[skill] /speak command created.");
        }
    }

    println!();
    println!("Restart Claude Code / Claude Desktop to activate.");

    Ok(())
}

fn handle_pack(action: PackAction) -> Result<()> {
    match action {
        PackAction::List => {
            let installed = pack::list_installed()?;
            let available = pack::list_available();

            let conn = db::open()?;
            let prefs = db::get_preferences(&conn)?;
            let active = prefs.pack.as_deref().unwrap_or("");

            if installed.is_empty() {
                println!("No packs installed.\n");
            } else {
                println!("Installed:");
                for p in &installed {
                    let marker = if p.name == active { " (active)" } else { "" };
                    let cats: Vec<&str> = p.categories.keys().map(|k| k.as_str()).collect();
                    println!(
                        "  {} — {}{} [{}]",
                        p.name,
                        p.display_name,
                        marker,
                        cats.join(", ")
                    );
                }
                println!();
            }

            let installed_names: Vec<&str> = installed.iter().map(|p| p.name.as_str()).collect();
            let not_installed: Vec<&&str> = available
                .iter()
                .filter(|n| !installed_names.contains(*n))
                .collect();

            if !not_installed.is_empty() {
                println!("Available for install:");
                for name in &not_installed {
                    println!("  {name}");
                }
            }
        }
        PackAction::Install { name } => {
            println!("Installing pack '{name}'...");
            pack::install(&name)?;
            println!("Pack '{name}' installed.");
        }
        PackAction::Remove { name } => {
            if pack::remove(&name)? {
                // Clear active pack if it was the removed one
                let conn = db::open()?;
                let prefs = db::get_preferences(&conn)?;
                if prefs.pack.as_deref() == Some(&name) {
                    db::set_preference(&conn, "pack", "")?;
                }
                println!("Pack '{name}' removed.");
            } else {
                println!("Pack '{name}' not found.");
            }
        }
        PackAction::Set { name } => {
            // Verify pack is installed
            let _ = pack::load_manifest(&name)?;
            let conn = db::open()?;
            db::set_preference(&conn, "pack", &name)?;
            println!("Active pack set to '{name}'.");
        }
        PackAction::Play {
            category,
            pack: pack_name,
        } => {
            let name = match pack_name {
                Some(n) => n,
                None => {
                    let conn = db::open()?;
                    let prefs = db::get_preferences(&conn)?;
                    prefs.pack.unwrap_or_default()
                }
            };
            if name.is_empty() {
                anyhow::bail!("No active pack. Set one with: vox pack set <name>");
            }
            let line = pack::play(&name, Some(&category))?;
            println!("{line}");
        }
    }
    Ok(())
}

fn handle_stats() -> Result<()> {
    let conn = db::open()?;
    let (count, total_chars) = db::get_usage_summary(&conn)?;

    println!("Total requests: {count}");
    println!("Total characters: {total_chars}");

    if count > 0 {
        println!("\nRecent usage:");
        let entries = db::get_usage_stats(&conn)?;
        for e in &entries {
            let voice_str = e.voice.as_deref().unwrap_or("-");
            let lang_str = e.lang.as_deref().unwrap_or("-");
            let dur_str = e
                .duration_ms
                .map(|d| format!("{d}ms"))
                .unwrap_or_else(|| "-".into());
            println!(
                "  {} | {} | voice={} lang={} | {}chars | {}",
                e.timestamp, e.backend, voice_str, lang_str, e.text_len, dur_str
            );
        }
    }
    Ok(())
}
