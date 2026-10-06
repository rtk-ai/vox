//! Daemon — a second vox process that keeps TTS models loaded between calls.
//!
//! `vox daemon start` spawns `vox daemon _run` in the background: a small
//! HTTP server on 127.0.0.1 with three routes, `GET /health`, `POST /speak`
//! and `POST /shutdown`. While it runs, `vox "text"` sends the text and its
//! options to `/speak` instead of loading a model itself (`main.rs` decides
//! which calls: a model-backed backend, no `-o`). The daemon calls the backend
//! in its own process, plays the audio itself and answers once playback is
//! over, one request at a time.
//!
//! Nothing is loaded at start: the first request for a backend pays the load
//! (and the download, the first time ever), later ones find the model in
//! memory. The daemon only speaks. It has no transcription route, so
//! `vox hear` never comes here.
//!
//! It stops by itself after `--idle-timeout` seconds without a request
//! (300 unless the flag says otherwise, 0 to never stop). What it prints goes
//! to `daemon.log` in the config directory, since it has no terminal.

use std::io::Write as _;
use std::path::{Path, PathBuf};
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

pub fn daemon_port() -> u16 {
    std::env::var("VOX_DAEMON_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

pub fn pid_path() -> PathBuf {
    config::config_dir().join("daemon.pid")
}

/// Where the daemon's stderr goes: it outlives the terminal that started it.
pub fn log_path() -> PathBuf {
    config::config_dir().join("daemon.log")
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

    /// Whether the daemon has gone more than `limit` seconds without work.
    ///
    /// `last_request` alone is not enough: it is stamped when a request
    /// arrives, and one request can outlast the limit (a first model download,
    /// a long text read aloud, a queue of callers). A held `speak_lock` is a
    /// request being served, with any others waiting behind it.
    fn idle_expired(&self, limit: u64) -> bool {
        self.speak_lock.try_lock().is_ok() && self.idle_secs() > limit
    }

    fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Which models are actually resident in this process.
    ///
    /// This used to report only voxtream, so `vox daemon status` said
    /// "(none loaded yet)" even with pocket or qwen-native warm — the whole
    /// point of the daemon, reported as if it were not working.
    ///
    /// Whisper is not in the list: nothing here transcribes, so this process
    /// never loads it.
    fn loaded_backends(&self) -> Vec<String> {
        let mut backends = Vec::new();
        if crate::backend::pocket::is_loaded() {
            backends.push("pocket".into());
        }
        if crate::backend::piper::is_loaded() {
            backends.push("piper".into());
        }
        if crate::backend::qwen_native::is_loaded() {
            backends.push("qwen-native".into());
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

/// The idle limit `--idle-timeout` asks for: 0 is the documented way to ask
/// for none. The default is clap's, so a 0 here was typed by the user.
fn idle_limit(idle_timeout: u64) -> Option<u64> {
    (idle_timeout > 0).then_some(idle_timeout)
}

/// Run the daemon HTTP server.
pub async fn run(idle_timeout: u64) -> Result<()> {
    let port = daemon_port();
    let limit = idle_limit(idle_timeout);

    let state = Arc::new(DaemonState::new());

    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .with_context(|| format!("failed to bind port {port}"))?;

    // Only a daemon that holds the port may claim the PID file: written
    // before the bind, a start that failed left its dead PID there, over the
    // PID of the daemon that does hold the port if there is one.
    write_pid()?;

    match limit {
        Some(secs) => {
            eprintln!("[daemon] vox daemon listening on 127.0.0.1:{port} (idle timeout: {secs}s)")
        }
        None => eprintln!("[daemon] vox daemon listening on 127.0.0.1:{port} (no idle timeout)"),
    }

    // Idle watchdog
    if let Some(limit) = limit {
        let watchdog_state = Arc::clone(&state);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                if watchdog_state.idle_expired(limit) {
                    eprintln!("[daemon] Idle timeout ({limit}s) — shutting down.");
                    remove_pid();
                    std::process::exit(0);
                }
            }
        });
    }

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
                // The daemon plays; saving to a file is handled in-process by
                // the CLI, which never routes an --output call here.
                output: None,
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

            // Count idleness from the end of the speech, not from its start.
            state.touch();

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

    post_speak(&daemon_url("/speak"), &req)
}

/// Send one speak request and wait for the daemon's verdict.
fn post_speak(url: &str, req: &SpeakRequest) -> Result<()> {
    // No deadline: the answer comes when playback is over, after any model
    // download and after the requests queued ahead, and none of that has an
    // upper bound. The same call made without the daemon has none either. A
    // daemon that dies closes the connection, which ends the wait.
    let resp: SpeakResponse = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .context("failed to build the daemon client")?
        .post(url)
        .json(req)
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
    let log = log_path();
    // Without the log the daemon still works; it only goes back to being mute.
    let opened = open_log(&log);
    if let Err(e) = &opened {
        eprintln!("Warning: {e:#}; the daemon will run without a log.");
    }
    let logged = opened.is_ok();
    let stderr = opened.map_or_else(|_| Stdio::null(), Stdio::from);
    let mut command = std::process::Command::new(exe);
    command
        .args([
            "daemon",
            "_run",
            "--idle-timeout",
            &idle_timeout.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    detach_from_caller(&mut command);
    let child = command.spawn().context("failed to spawn daemon process")?;

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

    // The reason is whatever the daemon said before dying, e.g. a busy port.
    let said = logged.then(|| std::fs::read_to_string(&log).unwrap_or_default());
    anyhow::bail!("{}", start_failure(said.as_deref(), &log))
}

/// Keep the daemon from holding on to whoever started it.
///
/// On Windows a child inherits every inheritable handle of its parent, not
/// only the three it is given. `vox daemon start` itself received its
/// standard handles from its caller, so the daemon kept the caller's pipes
/// open for as long as it lived: a script or an agent reading the output of
/// `vox daemon start` waited until the daemon stopped. The standard handles of
/// this process are marked non-inheritable before the spawn, and the daemon
/// gets no console and its own process group, so closing the caller's window
/// or pressing Ctrl+C there does not take it down.
#[cfg(windows)]
fn detach_from_caller(command: &mut std::process::Command) {
    use std::ffi::c_void;
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    // STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE
    const STANDARD_HANDLES: [u32; 3] = [-10i32 as u32, -11i32 as u32, -12i32 as u32];

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
    }

    for which in STANDARD_HANDLES {
        // SAFETY: GetStdHandle takes no pointer and returns a handle owned by
        // the process, null or INVALID_HANDLE_VALUE when there is none.
        // SetHandleInformation only changes a flag on that handle; a failure
        // (a console handle on old systems) leaves it as it was.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() && handle as isize != -1 {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// Unix gives a child only the descriptors it is handed: nothing to do.
#[cfg(not(windows))]
fn detach_from_caller(_command: &mut std::process::Command) {}

/// What a failed start reports. `said` is the daemon's log, `None` when the
/// log could not be opened: the message must not then send the reader to a
/// file this start never wrote, which may not exist or may date from an
/// earlier run.
fn start_failure(said: Option<&str>, log: &Path) -> String {
    match said {
        Some(said) => format!(
            "Daemon failed to start within 15 seconds\n{}\n(daemon log: {})",
            said.trim_end(),
            log.display()
        ),
        None => "Daemon failed to start within 15 seconds (it had no log to say why)".into(),
    }
}

/// Open the file that receives the daemon's stderr, emptied for this run.
///
/// The daemon is where models load when a call goes through it, so download
/// notices, `Using device` and its own errors are only ever printed there.
/// One run per file keeps it from growing across restarts.
fn open_log(path: &Path) -> Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // Emptied, then reopened to append: a writer that appends never leaves a
    // hole behind if the file is emptied again under it.
    std::fs::File::create(path)
        .and_then(|_| std::fs::OpenOptions::new().append(true).open(path))
        .with_context(|| format!("failed to open {}", path.display()))
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
            println!("  Log:     {}", log_path().display());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead as _, Read as _};

    fn request() -> SpeakRequest {
        SpeakRequest {
            text: "hello".into(),
            backend: "piper".into(),
            voice: None,
            lang: None,
            rate: None,
            gender: None,
            style: None,
            ref_audio: None,
            ref_text: None,
            model: None,
            volume: 1.0,
        }
    }

    /// A stand-in for the daemon: reads one request, waits `delay`, answers
    /// `body`. It plays nothing. Returns the URL to post to.
    fn fake_daemon(delay: Duration, body: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/speak", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream);
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(val) = line.to_lowercase().strip_prefix("content-length:") {
                    content_length = val.trim().parse().unwrap();
                }
            }
            let mut sent = vec![0u8; content_length];
            reader.read_exact(&mut sent).unwrap();
            std::thread::sleep(delay);
            write!(
                reader.into_inner(),
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        url
    }

    #[test]
    fn idle_timeout_zero_means_no_limit() {
        // `vox daemon start --help`: "0 = no timeout". It used to mean 300 s.
        assert_eq!(idle_limit(0), None);
        assert_eq!(idle_limit(1), Some(1));
        assert_eq!(idle_limit(300), Some(300));
    }

    #[test]
    fn a_request_being_served_is_not_idleness() {
        let state = DaemonState::new();
        state
            .last_request
            .store(now_epoch_secs() - 1000, Ordering::Relaxed);
        assert!(state.idle_expired(300));

        // A speech that started 1000 s ago and is still going: the watchdog
        // used to exit the process in the middle of it.
        let speaking = state.speak_lock.try_lock().unwrap();
        assert!(!state.idle_expired(300));
        drop(speaking);

        state.touch();
        assert!(!state.idle_expired(300));
    }

    #[test]
    fn a_daemon_error_reaches_the_caller() {
        let url = fake_daemon(
            Duration::ZERO,
            r#"{"success":false,"error":"no such voice"}"#,
        );
        let err = post_speak(&url, &request()).unwrap_err();
        assert!(format!("{err:#}").contains("no such voice"), "{err:#}");
    }

    #[test]
    #[ignore = "takes 125 s: it has to outlast the 120 s the client used to allow"]
    fn the_client_waits_as_long_as_the_daemon_needs() {
        let url = fake_daemon(Duration::from_secs(125), r#"{"success":true}"#);
        post_speak(&url, &request()).unwrap();
    }

    #[test]
    fn the_log_is_emptied_for_each_run() {
        let dir = tempfile::tempdir().unwrap();
        // The config directory may not exist yet on a first `daemon start`.
        let path = dir.path().join("config").join("daemon.log");
        writeln!(open_log(&path).unwrap(), "first run").unwrap();
        writeln!(open_log(&path).unwrap(), "second run").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second run\n");
    }

    #[test]
    fn a_failed_start_cites_only_a_log_it_wrote() {
        let log = Path::new("/cfg/daemon.log");
        let said = start_failure(Some("Error: failed to bind port 19876\n"), log);
        assert!(said.contains("failed to bind port 19876"), "{said}");
        assert!(said.contains("/cfg/daemon.log"), "{said}");

        // The log could not be opened (config directory unusable): naming it
        // pointed at a file that was not there.
        let mute = start_failure(None, log);
        assert!(!mute.contains("daemon.log"), "{mute}");
    }
}
