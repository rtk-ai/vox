//! Security edge cases: shell-metacharacter text and path-like voice names must
//! never reach a shell or be interpreted as paths.
//!
//! The backend command builders are exercised directly so these tests are
//! deterministic and do not play audio. The `say` and `qwen` backends only
//! exist on macOS, so the whole file is gated.
#![cfg(target_os = "macos")]

use std::ffi::OsStr;
use std::process::Command;

use vox::backend::SpeakOptions;
use vox::backend::qwen::QwenBackend;
use vox::backend::say::SayBackend;

const MALICIOUS_TEXT: &str = "; ls -la # $(id) `id` | cat /etc/passwd";

fn args_of(cmd: &Command) -> Vec<String> {
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn say_backend_passes_text_as_single_argv_without_shell() {
    let cmd = SayBackend::build_command(MALICIOUS_TEXT, &SpeakOptions::default());
    assert_eq!(cmd.get_program(), OsStr::new("/usr/bin/say"));
    let args = args_of(&cmd);
    // The whole text is one argument, untouched — no `sh -c`, no splitting.
    assert_eq!(args.last().map(String::as_str), Some(MALICIOUS_TEXT));
    assert!(!args.iter().any(|a| a == "-c" || a.ends_with("sh")));
}

#[test]
fn say_backend_voice_is_separate_argv() {
    let opts = SpeakOptions {
        voice: Some("../../some/path".to_string()),
        ..Default::default()
    };
    let args = args_of(&SayBackend::build_command("hello", &opts));
    let pos = args.iter().position(|a| a == "-v").expect("-v flag");
    assert_eq!(args[pos + 1], "../../some/path");
    assert_eq!(args.last().map(String::as_str), Some("hello"));
}

#[test]
fn qwen_backend_passes_text_as_single_argv_without_shell() {
    let malicious = "; import os; os.system('echo pwned') #";
    let cmd = QwenBackend::build_generate_command(malicious, &SpeakOptions::default());
    assert_eq!(cmd.get_program(), OsStr::new("python3"));
    let args = args_of(&cmd);
    // Text goes through `--text <value>`, never interpolated into a `-c` script.
    let pos = args
        .iter()
        .position(|a| a == "--text")
        .expect("--text flag");
    assert_eq!(args[pos + 1], malicious);
    assert!(!args.iter().any(|a| a == "-c"));
}

/// End-to-end: a path-like voice name must not crash the CLI or be treated as a
/// filesystem path. Uses the `say` backend (no model download); macOS only.
#[test]
fn path_traversal_in_voice_name_is_handled() {
    use assert_cmd::cargo::cargo_bin_cmd;
    use predicates::prelude::*;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["-b", "say", "-v", "../../some/path", "hi"])
        .assert()
        .stderr(predicate::str::contains("panicked").not())
        .stdout(predicate::str::contains("Cargo.toml").not());
}
