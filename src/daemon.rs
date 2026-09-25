//! Lazy daemon — keeps heavy TTS models warm in memory.
//!
//! `vox daemon start` launches a local HTTP server. Subsequent `vox "text"`
//! calls route through the daemon for ~1-2s latency instead of 20-60s cold start.
//! Auto-shuts down after idle timeout (default 5min).
//!
//! then proxies speak requests through a Python WebSocket client script.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::backend::{self, SpeakOptions};
use crate::config;

const DEFAULT_PORT: u16 = 19876;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;

pub fn daemon_port() -> u16 {
    std::env::var("VOX_DAEMON_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

pub fn pid_path() -> PathBuf {
    config::config_dir().join("daemon.pid")
}

fn daemon_url(path: &str) -> String {
    format!("http://127.0.0.1:{}{}", daemon_port(), path)
}

// ── PID file management ──────────────────────────────────────

fn write_pid() -> Result<()> {
    let path = pid_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut f = std::fs::File::create(&path).context("failed to write PID file")?;
    write!(f, "{}", std::process::id())?;
    Ok(())
}

pub fn read_pid() -> Option<u32> {
    std::fs::read_to_string(pid_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn remove_pid() {
    let _ = std::fs::remove_file(pid_path());
}

// ── Health check (client side) ───────────────────────────────

pub fn is_running() -> bool {
    reqwest::blocking::Client::new()
        .get(daemon_url("/health"))
        .timeout(Duration::from_millis(500))
        .send()
        .is_ok_and(|r| r.status().is_success())
}

// ── Request / Response types ─────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct SpeakRequest {
    pub text: String,
    pub backend: String,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub rate: Option<u32>,
    #[serde(default)]
    pub gender: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub ref_audio: Option<String>,
    #[serde(default)]
    pub ref_text: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_volume")]
    pub volume: f32,
}

fn default_volume() -> f32 {
    1.0
}

impl SpeakRequest {
    /// Clamp volume to valid range after deserialization.
    pub fn validated(mut self) -> Self {
        self.volume = self.volume.clamp(0.0, 5.0);
        self
    }
}

#[derive(Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    uptime_secs: u64,
    loaded_backends: Vec<String>,
    pid: u32,
}

#[derive(Serialize, Deserialize)]
struct SpeakResponse {
    success: bool,
    error: Option<String>,
    duration_ms: Option<u64>,
}

struct DaemonState {
    start_time: Instant,
    last_request: AtomicU64,
    speak_lock: Mutex<()>,
}

impl DaemonState {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            last_request: AtomicU64::new(now_epoch_secs()),
            speak_lock: Mutex::new(()),
        }
    }

    fn touch(&self) {
        self.last_request.store(now_epoch_secs(), Ordering::Relaxed);
    }

    fn idle_secs(&self) -> u64 {
        now_epoch_secs().saturating_sub(self.last_request.load(Ordering::Relaxed))
    }

    fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Which models are actually resident in this process.
    ///
    /// This used to report only voxtream, so `vox daemon status` said
    /// "(none loaded yet)" even with pocket or qwen-native warm — the whole
    /// point of the daemon, reported as if it were not working.
    fn loaded_backends(&self) -> Vec<String> {
        let mut backends = Vec::new();
        if crate::backend::pocket::is_loaded() {
            backends.push("pocket".into());
        }
        if crate::backend::qwen_native::is_loaded() {
            backends.push("qwen-native".into());
        }
        if crate::stt::is_loaded() {
            backends.push("whisper (stt)".into());
        }
        backends
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Run the daemon HTTP server.
pub async fn run(idle_timeout: u64) -> Result<()> {
    let port = daemon_port();
    let timeout = if idle_timeout > 0 {
        idle_timeout
    } else {
        DEFAULT_IDLE_TIMEOUT_SECS
    };

    write_pid()?;
    let state = Arc::new(DaemonState::new());

    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .with_context(|| format!("failed to bind port {port}"))?;

    eprintln!("[daemon] vox daemon listening on 127.0.0.1:{port} (idle timeout: {timeout}s)");

    // Idle watchdog
    let watchdog_state = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            if watchdog_state.idle_secs() > timeout {
                eprintln!("[daemon] Idle timeout ({timeout}s) — shutting down.");
                remove_pid();
                std::process::exit(0);
            }
        }
    });

    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, state).await {
                eprintln!("[daemon] Connection error: {e}");
            }
        });
    }
}

/// Minimal HTTP/1.1 handler.
async fn handle_connection(stream: tokio::net::TcpStream, state: Arc<DaemonState>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);

    let mut request_line = String::new();
    buf_reader.read_line(&mut request_line).await?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0].to_string();
    let path = parts[1].to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
        let lower = line.to_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        buf_reader.read_exact(&mut body).await?;
    }

    let (status, response_body) = route(&method, &path, &body, &state).await;

    let http = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    writer.write_all(http.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

async fn route(
    method: &str,
    path: &str,
    body: &[u8],
    state: &Arc<DaemonState>,
) -> (&'static str, String) {
    match (method, path) {
        ("GET", "/health") => {
            state.touch();
            let resp = serde_json::json!({
                "status": "ok",
                "uptime_secs": state.uptime_secs(),
                "loaded_backends": state.loaded_backends(),
                "pid": std::process::id(),
            });
            ("200 OK", resp.to_string())
        }
        ("POST", "/speak") => {
            let req: SpeakRequest = match serde_json::from_slice(body) {
                Ok(r) => SpeakRequest::validated(r),
                Err(e) => {
                    let resp = serde_json::json!({"success": false, "error": format!("invalid JSON: {e}")});
                    return ("400 Bad Request", resp.to_string());
                }
            };

            state.touch();
            let _lock = state.speak_lock.lock().await;

            // Hand off to the backend; the model stays warm in this process.
            let opts = SpeakOptions {
                voice: req.voice.clone(),
                lang: req.lang.clone(),
                rate: req.rate,
                gender: req.gender.clone(),
                style: req.style.clone(),
                ref_audio: req.ref_audio.clone(),
                ref_text: req.ref_text.clone(),
                model: req.model.clone(),
                volume: req.volume,
            };
            let backend_name = req.backend.clone();
            let text = req.text.clone();

            let result = tokio::task::spawn_blocking(move || {
                let start = Instant::now();
                let b = backend::get_backend(&backend_name)?;
                b.speak(&text, &opts)?;
                Ok::<_, anyhow::Error>(start.elapsed())
            })
            .await;

            match result {
                Ok(Ok(dur)) => {
                    let resp =
                        serde_json::json!({"success": true, "duration_ms": dur.as_millis() as u64});
                    ("200 OK", resp.to_string())
                }
                Ok(Err(e)) => {
                    let resp = serde_json::json!({"success": false, "error": format!("{e:#}")});
                    ("500 Internal Server Error", resp.to_string())
                }
                Err(e) => {
                    let resp = serde_json::json!({"success": false, "error": format!("task panicked: {e}")});
                    ("500 Internal Server Error", resp.to_string())
                }
            }
        }
        ("POST", "/shutdown") => {
            eprintln!("[daemon] Shutdown requested.");
            tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                remove_pid();
                std::process::exit(0);
            });
            let resp = serde_json::json!({"status": "shutting_down"});
            ("200 OK", resp.to_string())
        }
        _ => {
            let resp = serde_json::json!({"error": "not found"});
            ("404 Not Found", resp.to_string())
        }
    }
}

// ── Client: speak through daemon ─────────────────────────────

pub fn speak_via_daemon(text: &str, backend: &str, opts: &SpeakOptions) -> Result<()> {
    let req = SpeakRequest {
        text: text.to_string(),
        backend: backend.to_string(),
        voice: opts.voice.clone(),
        lang: opts.lang.clone(),
        rate: opts.rate,
        gender: opts.gender.clone(),
        style: opts.style.clone(),
        ref_audio: opts.ref_audio.clone(),
        ref_text: opts.ref_text.clone(),
        model: opts.model.clone(),
        volume: opts.volume,
    };

    let resp: SpeakResponse = reqwest::blocking::Client::new()
        .post(daemon_url("/speak"))
        .json(&req)
        .timeout(Duration::from_secs(120))
        .send()
        .context("failed to connect to vox daemon")?
        .json()
        .context("invalid daemon response")?;

    if resp.success {
        Ok(())
    } else {
        anyhow::bail!(
            "daemon speak failed: {}",
            resp.error.unwrap_or_else(|| "unknown error".into())
        )
    }
}

// ── CLI handlers ─────────────────────────────────────────────

pub fn handle_start(idle_timeout: u64) -> Result<()> {
    if is_running() {
        println!("Daemon already running (port {}).", daemon_port());
        return Ok(());
    }

    let exe = std::env::current_exe().context("cannot find vox binary")?;
    let child = std::process::Command::new(exe)
        .args([
            "daemon",
            "_run",
            "--idle-timeout",
            &idle_timeout.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn daemon process")?;

    println!(
        "Daemon starting (pid {}, port {})...",
        child.id(),
        daemon_port()
    );

    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(500));
        if is_running() {
            println!("Daemon ready.");
            return Ok(());
        }
    }

    anyhow::bail!("Daemon failed to start within 15 seconds")
}

pub fn handle_stop() -> Result<()> {
    if !is_running() {
        println!("Daemon not running.");
        remove_pid();
        return Ok(());
    }

    reqwest::blocking::Client::new()
        .post(daemon_url("/shutdown"))
        .timeout(Duration::from_secs(5))
        .send()
        .ok();

    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(300));
        if !is_running() {
            println!("Daemon stopped.");
            remove_pid();
            return Ok(());
        }
    }

    if let Some(pid) = read_pid() {
        #[cfg(unix)]
        {
            std::process::Command::new("kill")
                .arg(pid.to_string())
                .status()
                .ok();
        }
        remove_pid();
        println!("Daemon killed (pid {pid}).");
    }

    Ok(())
}

pub fn handle_status() -> Result<()> {
    match reqwest::blocking::Client::new()
        .get(daemon_url("/health"))
        .timeout(Duration::from_millis(1000))
        .send()
    {
        Ok(resp) if resp.status().is_success() => {
            let health: HealthResponse = resp.json().context("invalid health response")?;
            println!(
                "Daemon running (pid {}, port {})",
                health.pid,
                daemon_port()
            );
            println!("  Uptime:  {}s", health.uptime_secs);
            if health.loaded_backends.is_empty() {
                println!("  Models:  (none loaded yet)");
            } else {
                println!("  Models:  {}", health.loaded_backends.join(", "));
            }
        }
        _ => {
            println!("Daemon not running.");
            if let Some(pid) = read_pid() {
                println!("  (stale PID file: {pid})");
                remove_pid();
            }
        }
    }
    Ok(())
}
