use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config;

const DEFAULT_PORT: u16 = 9876;
const DEFAULT_MODEL: &str = "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-bf16";

fn pid_file() -> PathBuf {
    config::config_dir().join("server.pid")
}

fn port_file() -> PathBuf {
    config::config_dir().join("server.port")
}

/// Return the port the server listens on, or the default.
pub fn server_port() -> u16 {
    std::fs::read_to_string(port_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Base URL for the running server.
pub fn base_url() -> String {
    format!("http://127.0.0.1:{}", server_port())
}

/// Check whether the server is reachable.
pub fn is_running() -> bool {
    let url = format!("{}/v1/models", base_url());
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()
        .and_then(|c| c.get(&url).send().ok())
        .is_some_and(|r| r.status().is_success())
}

/// Start the built-in mlx-audio server in the background.
///
/// This runs `python3 -m mlx_audio.server` which exposes an OpenAI-compatible
/// REST API and keeps the TTS model warm in memory.
pub fn start(model: Option<&str>, port: Option<u16>) -> Result<()> {
    if is_running() {
        println!("Server already running on port {}.", server_port());
        return Ok(());
    }

    let model = model.unwrap_or(DEFAULT_MODEL);
    let port = port.unwrap_or(DEFAULT_PORT);

    let mut child = Command::new("python3")
        .arg("-m")
        .arg("mlx_audio.server")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(
            "Failed to start mlx-audio server. Is mlx-audio[server] installed?\n\
             Install with: pip install 'mlx-audio[server]'",
        )?;

    let pid = child.id();

    // Wait for the server to become reachable (poll health endpoint)
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    let health_url = format!("http://127.0.0.1:{port}/v1/models");

    let mut ready = false;
    for _ in 0..60 {
        // up to 120s
        std::thread::sleep(Duration::from_secs(2));
        if client
            .get(&health_url)
            .send()
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        // Check the child is still alive
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
    }

    if !ready {
        let _ = child.kill();
        // Try to read stderr for diagnostics
        let stderr = child
            .stderr
            .take()
            .and_then(|s| {
                let reader = std::io::BufReader::new(s);
                let lines: Vec<String> = reader.lines().take(10).flatten().collect();
                if lines.is_empty() {
                    None
                } else {
                    Some(lines.join("\n"))
                }
            })
            .unwrap_or_default();
        anyhow::bail!(
            "Server failed to start within 120s.\n\
             Make sure mlx-audio[server] is installed: pip install 'mlx-audio[server]'\n\
             {stderr}"
        );
    }

    // Pre-load the model via the /v1/models endpoint
    let load_url = format!("http://127.0.0.1:{port}/v1/models");
    let load_resp = client
        .post(&load_url)
        .json(&serde_json::json!({"model": model}))
        .send();

    if let Ok(resp) = load_resp
        && !resp.status().is_success()
    {
        eprintln!(
            "Warning: model pre-load returned {}: {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
    }

    // Save PID and port
    std::fs::create_dir_all(config::config_dir())?;
    std::fs::write(pid_file(), pid.to_string())?;
    std::fs::write(port_file(), port.to_string())?;

    // Detach from child (drop the handle, process continues)
    std::mem::forget(child);

    println!("Server started (pid {pid}, port {port}, model {model}).");
    Ok(())
}

/// Stop the running server.
pub fn stop() -> Result<()> {
    let pf = pid_file();
    if !pf.exists() {
        println!("No server PID file found.");
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pf)?;
    let pid: u32 = pid_str
        .trim()
        .parse()
        .context("invalid PID in server.pid")?;

    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .context("failed to send kill signal")?;

    let _ = std::fs::remove_file(&pf);

    if status.success() {
        println!("Server stopped (pid {pid}).");
    } else {
        println!("Process {pid} not found (may have already exited).");
    }
    Ok(())
}

/// Print server status.
pub fn status() -> Result<()> {
    if is_running() {
        let port = server_port();
        let pid_info = std::fs::read_to_string(pid_file())
            .map(|s| format!("pid {}", s.trim()))
            .unwrap_or_else(|_| "pid unknown".into());
        println!("Server running ({pid_info}, port {port}).");
    } else {
        println!("Server not running.");
        // Clean stale PID file
        let _ = std::fs::remove_file(pid_file());
    }
    Ok(())
}
