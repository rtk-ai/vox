use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use vox::backend::{self, SpeakOptions};
use vox::config::DEFAULT_BACKEND;
use vox::{clone, db, init, input};

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
    /// Set up AI assistant integration (Claude Code)
    Init,
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
        Some(Commands::Init) => handle_init(),
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

fn handle_init() -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let result = init::run_init(&cwd)?;

    if !result.claude_md_written && !result.settings_written {
        println!("Already configured — nothing to do.");
    } else {
        if result.claude_md_written {
            println!("CLAUDE.md configured with vox instructions.");
        }
        if result.settings_written {
            println!(".claude/settings.json configured with Stop hook.");
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
