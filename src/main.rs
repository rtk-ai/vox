use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use vox::backend::{self, SpeakOptions, TtsBackend};
use vox::config::DEFAULT_BACKEND;
use vox::{clone, daemon, db, init, input, mcp, pack, tui};

fn parse_volume(s: &str) -> Result<f32, String> {
    let v: f32 = s.parse().map_err(|e| format!("{e}"))?;
    if !(0.0..=5.0).contains(&v) {
        return Err("volume must be between 0.0 and 5.0".to_string());
    }
    Ok(v)
}

#[derive(Parser)]
#[command(name = "vox", version, about = "Voice Command — read text aloud")]
struct Cli {
    /// Text to speak (when no subcommand is used)
    text: Vec<String>,

    // No `default_value`: clap would fill it in, and `-b pocket` could no
    // longer be told apart from no flag at all. The help states the default.
    #[arg(short = 'b', long, help = backend_help())]
    backend: Option<String>,

    /// Voice name (or clone name)
    #[arg(short = 'v', long)]
    voice: Option<String>,

    /// Language code (picks the voice; other languages than English default to the piper backend)
    #[arg(short = 'l', long)]
    lang: Option<String>,

    /// Speech rate (words per minute, for say backend)
    #[arg(short = 'r', long)]
    rate: Option<u32>,

    /// No effect yet: accepted, but no backend reads it (feminine, masculine)
    #[arg(long)]
    gender: Option<String>,

    /// No effect yet: accepted, but no backend reads it (calm, energetic, warm, authoritative, cheerful, serious)
    #[arg(long)]
    style: Option<String>,

    /// TTS model for the qwen-native backend (e.g. Qwen/Qwen3-TTS-12Hz-0.6B-Base)
    #[arg(short = 'm', long)]
    model: Option<String>,

    /// Volume multiplier (1.0 = normal, 0.5 = half, 2.0 = double, range: 0.0–5.0)
    #[arg(long, default_value = "1.0", value_parser = parse_volume)]
    volume: f32,

    /// Write the audio to a WAV file instead of playing it
    #[arg(short = 'o', long, value_name = "FILE")]
    output: Option<std::path::PathBuf>,

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
    /// Interactive voice configuration (TUI for humans)
    Setup,
    /// Time every backend on a test sentence (rendered to a file: nothing is played)
    Bench {
        /// Store the fastest backend as the default. It then applies to every
        /// language, in place of the default chosen per language
        #[arg(long)]
        set: bool,
    },
    /// Manage the TTS daemon (keeps models warm for fast inference)
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Set up AI assistant integration for the tools installed here (Claude Code, Cursor, VS Code and 11 others)
    Init {
        /// Language for the generated instructions and Stop hook
        /// (default: your `vox config set lang`, else the system locale)
        #[arg(short = 'l', long)]
        lang: Option<String>,
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
    // No doc comment: the help names the default model, which is a constant.
    #[cfg(target_os = "macos")]
    #[command(about = format!(
        "Start a voice conversation with Claude (macOS only; model: VOX_CHAT_MODEL, default {DEFAULT_CHAT_MODEL})"
    ))]
    Chat {
        /// Voice clone name
        #[arg(short = 'v', long)]
        voice: Option<String>,
        /// Language code
        #[arg(short = 'l', long)]
        lang: Option<String>,
    },
    /// Record from microphone and transcribe to text (local Whisper, all platforms)
    Hear {
        /// Language code for transcription (default: auto-detect)
        #[arg(short = 'l', long)]
        lang: Option<String>,
        /// Whisper model repo (default: openai/whisper-base; see `vox config set stt_model`, VOX_STT_MODEL or models.toml)
        #[arg(short = 'm', long)]
        model: Option<String>,
        /// Transcribe this WAV file instead of recording from the microphone
        #[arg(short = 'f', long)]
        file: Option<String>,
        /// Maximum recording duration in seconds
        #[arg(short = 't', long, default_value = "30")]
        timeout: u32,
        /// Seconds of silence before stopping
        #[arg(short = 's', long, default_value = "2.0")]
        silence: f64,
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
    /// Install a sound pack from the peon-ping registry
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
    // No doc comment: the keys come from the list `db::set_preference` checks
    // against, so the help cannot name fewer keys than are accepted.
    #[command(about = format!("Set a preference ({})", db::preference_keys_help()))]
    Set {
        /// Preference key
        key: String,
        /// Preference value
        value: String,
    },
    /// Reset all preferences to defaults
    Reset,
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon (background process)
    Start {
        /// Idle timeout in seconds before auto-shutdown (0 = no timeout)
        #[arg(long, default_value = "300")]
        idle_timeout: u64,
    },
    /// Stop the daemon
    Stop,
    /// Show daemon status
    Status,
    /// Internal: run daemon in foreground (used by `start`)
    #[command(name = "_run", hide = true)]
    Run {
        #[arg(long, default_value = "300")]
        idle_timeout: u64,
    },
}

fn main() -> Result<()> {
    vox::timing::start();
    let cli = Cli::parse();
    vox::timing::mark("arguments parsed");

    match cli.command {
        Some(Commands::Clone { action }) => handle_clone(action),
        Some(Commands::Config { action }) => handle_config(action),
        Some(Commands::Stats) => handle_stats(),
        Some(Commands::Setup) => tui::run(),
        Some(Commands::Bench { set }) => handle_bench(set),
        Some(Commands::Daemon { action }) => handle_daemon(action),
        Some(Commands::Init { mode, lang }) => handle_init(mode, lang),
        Some(Commands::Serve) => mcp::run_server(),
        Some(Commands::Pack { action }) => handle_pack(action),
        #[cfg(target_os = "macos")]
        Some(Commands::Chat { voice, lang }) => handle_chat(voice, lang),

        Some(Commands::Hear {
            lang,
            timeout,
            silence,
            model,
            file,
        }) => handle_hear(lang, timeout, silence, model, file),
        None => handle_speak(cli),
    }
}

/// Help for `-b`, with the default spelled out: the flag itself has none, so
/// that naming the default backend still counts as naming a backend.
fn backend_help() -> String {
    format!(
        "TTS backend ({}) [default: {DEFAULT_BACKEND}; piper for languages other than English]",
        backend::supported_backends().join(", ")
    )
}

fn handle_speak(cli: Cli) -> Result<()> {
    let conn = db::open()?;
    let prefs = db::get_preferences(&conn)?;

    // Said where the flag was typed: nothing downstream reads either value,
    // and silence would read as "applied".
    if cli.gender.is_some() || cli.style.is_some() {
        eprintln!("Note: --gender and --style have no effect yet: no backend reads them.");
    }

    let lang = cli.lang.clone().or(prefs.lang);

    let mut voice = cli.voice.or(prefs.voice);
    let rate = cli.rate.or(prefs.rate);
    let gender = cli.gender.or(prefs.gender);
    let style = cli.style.or(prefs.style);
    let model = cli.model.or(prefs.model);

    // Resolve voice clone
    let voice_clone = match voice.as_deref() {
        Some(voice_name) => clone::resolve_voice(&conn, voice_name)?,
        None => None,
    };

    // Merge: CLI flag > DB preference > language-aware default, with a clone
    // moved to a backend that can use it unless the flag named one.
    let effective_backend = clone::speak_backend(
        cli.backend.as_deref(),
        prefs.backend.as_deref(),
        lang.as_deref(),
        voice_clone.is_some(),
        clone::pocket_can_clone(),
    );

    let mut ref_audio = None;
    let mut ref_text = None;
    if let Some(vc) = voice_clone {
        if !clone::can_clone(&effective_backend) {
            eprintln!(
                "Note: the {effective_backend} backend cannot clone voices, so '{}' is ignored. \
                 Drop -b, or use -b qwen-native.",
                vc.name
            );
        }
        ref_audio = Some(vc.ref_audio);
        ref_text = vc.ref_text;
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
        volume: cli.volume,
        output: cli.output.clone(),
    };

    vox::timing::mark("preferences and backend resolved");
    let start = Instant::now();

    // Try the daemon for backends that load a model: a warm one skips the load,
    // which is the largest fixed cost of a short utterance (0.3 to 0.4 s for piper).
    let is_heavy = matches!(
        effective_backend.as_str(),
        "qwen-native" | "kokoro" | "pocket" | "piper"
    );
    // Saving bypasses the daemon on purpose: the daemon renders in its own
    // process, and a relative path there would resolve against its working
    // directory, not the user's.
    if is_heavy && opts.output.is_none() && daemon::is_running() {
        daemon::speak_via_daemon(&text, &effective_backend, &opts)?;
    } else {
        backend.speak(&text, &opts)?;
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    // Log usage
    let _ = db::log_speech(
        &conn,
        &effective_backend,
        opts.voice.as_deref(),
        opts.lang.as_deref(),
        &text,
        Some(duration_ms),
    );

    Ok(())
}

fn handle_clone(action: CloneAction) -> Result<()> {
    let conn = db::open()?;

    match action {
        CloneAction::Add { name, audio, text } => {
            let stored = clone::add_clone_from_file(&conn, &name, &audio, text.as_deref())?;
            eprintln!("Reference saved to {stored}");
            println!("Voice clone '{name}' added.");
        }
        CloneAction::Record {
            name,
            duration,
            text,
        } => {
            // Before the microphone opens: the recording is written under the
            // name, so a taken one would replace that clone's reference, and
            // only then fail to register.
            clone::new_reference_path(&conn, &name)?;
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
            if clone::remove_clone(&conn, &name)? {
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
            for line in prefs.summary_lines() {
                println!("{line}");
            }
            println!("{}", vox::accel::config_line());
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

/// The Claude model `vox chat` talks to when VOX_CHAT_MODEL names none: the
/// fastest current one. A spoken reply is short, and what is felt is the wait
/// before its first word. The request carries no `thinking` setting, and this
/// model then answers at once, where the larger current models think first.
#[cfg(target_os = "macos")]
const DEFAULT_CHAT_MODEL: &str = "claude-haiku-4-5";

/// The model for `vox chat`: VOX_CHAT_MODEL when it names one. An empty value
/// counts as unset, which is what `VOX_CHAT_MODEL=` in a shell means.
#[cfg(target_os = "macos")]
fn chat_model(from_env: Option<String>) -> String {
    from_env
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| DEFAULT_CHAT_MODEL.to_string())
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
        model: chat_model(std::env::var("VOX_CHAT_MODEL").ok()),
    };

    chat::run_chat_loop(config)
}

fn handle_hear(
    lang: Option<String>,
    timeout: u32,
    silence: f64,
    model: Option<String>,
    file: Option<String>,
) -> Result<()> {
    use vox::{mic, stt};

    let samples = match file {
        Some(path) => mic::read_wav_16k(std::path::Path::new(&path))?,
        None => {
            eprintln!(
                "Listening... (speak now, stops after {silence}s of silence, max {timeout}s)"
            );
            mic::record(&mic::RecordOptions::until_silence(silence, timeout as f64))?
        }
    };
    if samples.len() < mic::TARGET_RATE as usize / 4 {
        eprintln!("(no speech detected)");
        return Ok(());
    }

    eprintln!("Transcribing...");
    let text = stt::transcribe_samples_with(&samples, lang.as_deref(), model.as_deref())?;
    if text.is_empty() {
        eprintln!("(no speech detected)");
    } else {
        println!("{text}");
    }
    Ok(())
}

fn handle_init(mode: InitMode, lang: Option<String>) -> Result<()> {
    let lang = vox::lang::resolve(lang.as_deref())?;
    let do_cli = matches!(mode, InitMode::Cli | InitMode::All);
    let do_mcp = matches!(mode, InitMode::Mcp | InitMode::All);
    let do_skill = matches!(mode, InitMode::Skill | InitMode::All);

    // The tools this run changed something for: the ones to restart.
    let mut restart: Vec<&str> = Vec::new();
    let mut no_tool_found = false;

    // --- CLI mode: CLAUDE.md + Stop hook ---
    if do_cli {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        let result = init::run_init(&cwd, lang.as_deref())?;

        if result.claude_md_written {
            println!("[cli] CLAUDE.md configured with vox instructions.");
        }
        if result.settings_written {
            println!("[cli] .claude/settings.json configured with Stop hook.");
        }
        if result.claude_md_written || result.settings_written {
            restart.push("Claude Code");
        } else {
            println!("[cli] already configured.");
        }
    }

    // --- MCP mode: configure the MCP server for the AI tools found here ---
    if do_mcp {
        let vox_bin = std::env::current_exe().context("cannot determine vox binary path")?;
        let home = dirs::home_dir().context("cannot determine home directory")?;
        let reports = init::configure_mcp(
            &home,
            &init::app_config_dir(&home),
            &vox_bin.to_string_lossy(),
        );

        for report in &reports {
            println!("[mcp] {:<20} {}", report.label, report.status);
            if report.newly_configured {
                restart.push(report.label);
            }
        }
        no_tool_found = reports.iter().all(|report| !report.installed);
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
                 vox \"$ARGUMENTS\"\n\
                 ```\n",
            )
            .context("cannot write skill file")?;
            println!("[skill] /speak command created.");
            restart.push("Claude Code");
        }
    }

    println!();
    for line in init_closing_lines(&restart, no_tool_found) {
        println!("{line}");
    }
    println!();
    println!("Claude Code plugin (live voice visualizer), in a Claude Code session:");
    for command in init::PLUGIN_INSTALL_COMMANDS {
        println!("  {command}");
    }

    Ok(())
}

/// What `vox init` says last: the tools to restart are the ones this run
/// changed something for, and no others.
fn init_closing_lines(restart: &[&str], no_tool_found: bool) -> Vec<String> {
    let mut tools: Vec<&str> = Vec::new();
    for tool in restart {
        if !tools.contains(tool) {
            tools.push(tool);
        }
    }
    if !tools.is_empty() {
        return vec![format!("Restart {} to activate.", tools.join(", "))];
    }
    if no_tool_found {
        return vec![
            "No supported AI tool was found on this machine: nothing was configured.".to_string(),
            "Install one, start it once, then run `vox init` again.".to_string(),
        ];
    }
    vec!["Nothing changed, so there is nothing to restart.".to_string()]
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

fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}h {mins:02}m {secs:02}s")
    } else if mins > 0 {
        format!("{mins}m {secs:02}s")
    } else {
        format!("{secs}s")
    }
}

fn handle_stats() -> Result<()> {
    let conn = db::open()?;
    let (count, total_chars) = db::get_usage_summary(&conn)?;

    if count == 0 {
        println!("No usage recorded yet.");
        return Ok(());
    }

    let total_duration_ms = db::get_total_duration_ms(&conn)?;
    let backend_stats = db::get_backend_stats(&conn)?;
    let lang_stats = db::get_lang_stats(&conn)?;

    // Header
    let total_secs = total_duration_ms as f64 / 1000.0;
    let speech_str = format_duration(total_duration_ms);

    println!("📊 vox stats");
    println!("═══════════════════════════════════════════════════");
    println!("  🎙  Total speech time:  {speech_str}");
    println!("  📞  Total calls:        {count}");
    println!("  📝  Total characters:   {total_chars}");
    if count > 0 && total_secs > 0.0 {
        println!(
            "  ⚡  Avg latency:        {:.1}s/call",
            total_secs / count as f64
        );
        println!(
            "  📏  Avg length:         {} chars/call",
            total_chars / count
        );
        let chars_per_sec = total_chars as f64 / total_secs;
        println!("  🔄  Throughput:         {chars_per_sec:.0} chars/s");
    }

    // By backend
    println!("\n  Backend breakdown:");
    println!("  ─────────────────────────────────────────────────");
    for b in &backend_stats {
        let pct = (b.calls as f64 / count as f64) * 100.0;
        let dur = format_duration(b.total_duration_ms);
        let avg = if b.calls > 0 {
            format!(
                "{:.1}s avg",
                b.total_duration_ms as f64 / 1000.0 / b.calls as f64
            )
        } else {
            "-".into()
        };
        println!(
            "    {:<14} {:>4} calls ({pct:>2.0}%)  {:>6} chars  {dur:>10}  {avg}",
            b.backend, b.calls, b.total_chars,
        );
    }

    // By language
    println!("\n  Language breakdown:");
    println!("  ─────────────────────────────────────────────────");
    for l in &lang_stats {
        let pct = (l.calls as f64 / count as f64) * 100.0;
        let bar_len = (pct / 5.0).round() as usize;
        let bar: String = "█".repeat(bar_len);
        println!(
            "    {:<6} {:>4} calls ({pct:>2.0}%)  {bar}",
            l.lang, l.calls
        );
    }

    // Recent 10
    let entries = db::get_usage_stats(&conn)?;
    println!("\n  Recent:");
    println!("  ─────────────────────────────────────────────────");
    for e in entries.iter().take(10) {
        let lang_str = e.lang.as_deref().unwrap_or("?");
        let dur_str = e
            .duration_ms
            .map(|d| format!("{:.1}s", d as f64 / 1000.0))
            .unwrap_or_else(|| "-".into());
        let ts = if e.timestamp.len() >= 16 {
            &e.timestamp[..16]
        } else {
            &e.timestamp
        };
        println!(
            "    {ts}  {:<14} {lang_str:<4} {:>5} chars  {dur_str:>6}",
            e.backend, e.text_len,
        );
    }

    Ok(())
}

fn handle_daemon(action: DaemonAction) -> Result<()> {
    match action {
        DaemonAction::Start { idle_timeout } => daemon::handle_start(idle_timeout),
        DaemonAction::Stop => daemon::handle_stop(),
        DaemonAction::Status => daemon::handle_status(),
        DaemonAction::Run { idle_timeout } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(daemon::run(idle_timeout))
        }
    }
}

/// Time one backend rendering `text` to a WAV file in `dir`. `output` is what
/// makes a backend write instead of play: a benchmark has no reason to be
/// heard, and playing would time the length of the sentence.
fn time_render(backend: &dyn TtsBackend, text: &str, dir: &std::path::Path) -> Result<u128> {
    let opts = SpeakOptions {
        output: Some(dir.join(format!("{}.wav", backend.name()))),
        ..Default::default()
    };
    let start = Instant::now();
    backend.speak(text, &opts)?;
    Ok(start.elapsed().as_millis())
}

/// Store the fastest backend when `--set` asks for it, and return the lines
/// that close the report. Without `--set` nothing is stored: a stored backend
/// applies to every language, so it turns off the default chosen per language.
fn conclude_bench(conn: &rusqlite::Connection, fastest: &str, set: bool) -> Result<Vec<String>> {
    let per_language = format!("{DEFAULT_BACKEND} for English, piper for the other languages");
    if set {
        db::set_preference(conn, "backend", fastest)?;
        return Ok(vec![
            format!("Saved {fastest} as the default backend, for every language."),
            format!("It replaces the default chosen per language ({per_language})."),
        ]);
    }
    Ok(vec![
        "Nothing was changed. To make it the default backend:".to_string(),
        format!("  vox config set backend {fastest}    (or: vox bench --set)"),
        format!("A stored backend applies to every language, in place of {per_language}."),
    ])
}

fn handle_bench(set: bool) -> Result<()> {
    println!("vox bench — timing each backend on this machine\n");

    // Detect platform
    let os = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    };

    // What this binary was built with, not what the machine has: a CPU build
    // uses no GPU even on a Mac.
    println!("  Platform:      {os}");
    println!("  Acceleration:  {}", vox::accel::describe());
    println!();

    // List backends to test
    let mut candidates: Vec<&str> = vec!["piper", "pocket"];
    #[cfg(target_os = "macos")]
    candidates.push("say");
    // Only test backends that are available
    if backend::get_backend("qwen-native")
        .map(|b| b.is_available())
        .unwrap_or(false)
    {
        candidates.push("qwen-native");
    }
    #[cfg(feature = "kokoro")]
    if backend::get_backend("kokoro")
        .map(|b| b.is_available())
        .unwrap_or(false)
    {
        candidates.push("kokoro");
    }

    let test_text = "Hello, this is a quick benchmark test.";
    let mut results: Vec<(&str, u128)> = Vec::new();
    let scratch = tempfile::tempdir().context("Failed to create a temporary directory")?;

    println!("  Testing {} backends...\n", candidates.len());

    for name in &candidates {
        print!("  {:<14} ", name);
        match backend::get_backend(name) {
            Ok(b) => match time_render(b.as_ref(), test_text, scratch.path()) {
                Ok(ms) => {
                    results.push((name, ms));
                    let bar_len = (ms / 500).min(20) as usize;
                    let bar: String = "\u{2588}".repeat(bar_len);
                    println!("{ms:>6}ms  {bar}");
                }
                Err(e) => {
                    println!("FAILED  ({e})");
                }
            },
            Err(e) => {
                println!("SKIP    ({e})");
            }
        }
    }

    if results.is_empty() {
        println!("\n  No backends available!");
        return Ok(());
    }

    // Sort by latency
    results.sort_by_key(|r| r.1);

    let best = results[0].0;
    let best_ms = results[0].1;

    println!(
        "\n  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}"
    );
    println!("  Fastest: {best} ({best_ms}ms)");

    let conn = db::open()?;
    for line in conclude_bench(&conn, best, set)? {
        println!("  {line}");
    }

    println!("\n  Run `vox bench` again after installing new backends.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A backend that records what it was asked instead of rendering.
    struct Recorder {
        asked: RefCell<Vec<SpeakOptions>>,
    }

    impl TtsBackend for Recorder {
        fn name(&self) -> &str {
            "recorder"
        }
        fn speak(&self, _text: &str, opts: &SpeakOptions) -> Result<()> {
            self.asked.borrow_mut().push(opts.clone());
            Ok(())
        }
        fn list_voices(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn is_available(&self) -> bool {
            true
        }
    }

    /// `vox bench` used to call `speak` with no output file, which is the
    /// request to play: the test sentence came out of the speakers once per
    /// backend.
    #[test]
    fn bench_renders_to_a_file_and_plays_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Recorder {
            asked: RefCell::new(Vec::new()),
        };

        time_render(&backend, "Hello.", dir.path()).unwrap();

        let asked = backend.asked.borrow();
        assert_eq!(asked.len(), 1);
        let output = asked[0].output.as_deref().expect("an output file");
        assert_eq!(output, dir.path().join("recorder.wav"));
    }

    /// The fastest backend used to be stored on every run, and a stored
    /// backend turns off the default chosen per language.
    #[test]
    fn bench_stores_nothing_unless_asked() {
        let conn = db::open_in_memory().unwrap();

        let lines = conclude_bench(&conn, "piper", false).unwrap();

        assert_eq!(db::get_preferences(&conn).unwrap().backend, None);
        let report = lines.join("\n");
        assert!(report.contains("vox config set backend piper"), "{report}");
        assert!(report.contains("vox bench --set"), "{report}");
        assert!(report.contains("Nothing was changed"), "{report}");
    }

    #[test]
    fn bench_set_stores_the_fastest_backend_and_says_what_it_replaces() {
        let conn = db::open_in_memory().unwrap();

        let lines = conclude_bench(&conn, "piper", true).unwrap();

        assert_eq!(
            db::get_preferences(&conn).unwrap().backend.as_deref(),
            Some("piper")
        );
        let report = lines.join("\n");
        assert!(report.contains("Saved piper"), "{report}");
        assert!(report.contains("every language"), "{report}");
    }

    #[test]
    fn init_names_only_the_tools_it_changed() {
        assert_eq!(
            init_closing_lines(&["Claude Code", "Cursor", "Claude Code"], false),
            ["Restart Claude Code, Cursor to activate."]
        );
        let unchanged = init_closing_lines(&[], false).join("\n");
        assert!(unchanged.contains("Nothing changed"), "{unchanged}");
        assert!(!unchanged.contains("Restart "), "{unchanged}");
        let none = init_closing_lines(&[], true).join("\n");
        assert!(none.contains("No supported AI tool was found"), "{none}");
        assert!(!none.contains("Claude"), "{none}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn chat_model_comes_from_the_environment_or_the_default() {
        assert_eq!(chat_model(None), DEFAULT_CHAT_MODEL);
        assert_eq!(chat_model(Some(String::new())), DEFAULT_CHAT_MODEL);
        assert_eq!(chat_model(Some("  ".into())), DEFAULT_CHAT_MODEL);
        assert_eq!(
            chat_model(Some(" claude-opus-5-5 ".into())),
            "claude-opus-5-5"
        );
    }
}
