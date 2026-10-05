//! `vox daemon start` must hand control back to whoever started it.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// A daemon of its own for the test: its config directory and a port nobody
/// else uses, stopped when the test ends however it ends.
struct Daemon {
    dir: tempfile::TempDir,
    port: u16,
}

impl Daemon {
    fn new() -> Self {
        // Bound and released at once: the system picks a free port.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        Self {
            dir: tempfile::tempdir().unwrap(),
            port,
        }
    }

    fn vox(&self) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("vox"));
        cmd.env("VOX_CONFIG_DIR", self.dir.path())
            .env("VOX_DB_PATH", self.dir.path().join("vox.db"))
            .env("VOX_DAEMON_PORT", self.port.to_string());
        cmd
    }

    fn log(&self) -> String {
        std::fs::read_to_string(Path::new(self.dir.path()).join("daemon.log")).unwrap_or_default()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.vox().args(["daemon", "stop"]).output();
    }
}

/// A script or an agent that reads the output of `vox daemon start` waits for
/// the end of that output. On Windows the daemon used to inherit the caller's
/// pipes and keep them open for as long as it ran, so the caller waited until
/// the daemon was stopped.
#[test]
fn daemon_start_releases_the_caller_that_reads_its_output() {
    let daemon = Daemon::new();

    let child = daemon
        .vox()
        .args(["daemon", "start", "--idle-timeout", "0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // wait_with_output reads both pipes to their end, as a caller would.
    let (done, output) = mpsc::channel::<Output>();
    thread::spawn(move || {
        let _ = done.send(child.wait_with_output().unwrap());
    });
    let output = output
        .recv_timeout(Duration::from_secs(60))
        .unwrap_or_else(|_| {
            panic!(
                "`vox daemon start` did not release its caller within 60 s: \
             the daemon is holding the caller's pipes open.\ndaemon log:\n{}",
                daemon.log()
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("Daemon ready."),
        "stdout: {stdout}\nstderr: {}\ndaemon log:\n{}",
        String::from_utf8_lossy(&output.stderr),
        daemon.log()
    );

    // And it is still there afterwards: released, not killed.
    let status = daemon.vox().args(["daemon", "status"]).output().unwrap();
    let status = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(status.contains("Daemon running"), "{status}");

    let stopped = daemon.vox().args(["daemon", "stop"]).output().unwrap();
    assert!(
        String::from_utf8_lossy(&stopped.stdout).contains("Daemon stopped."),
        "{}",
        String::from_utf8_lossy(&stopped.stdout)
    );
}
