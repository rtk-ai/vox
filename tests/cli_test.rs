use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_help_flag() {
    cargo_bin_cmd!("vox")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Voice Command"));
}

#[test]
fn test_version_flag() {
    cargo_bin_cmd!("vox")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("vox"));
}

#[test]
fn test_unknown_backend() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["--backend", "nonexistent", "hello"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown backend"));
}

#[cfg(target_os = "macos")]
#[test]
fn test_list_voices_say() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["--backend", "say", "--list-voices"])
        .assert()
        .success();
}

#[cfg(target_os = "macos")]
#[test]
fn test_list_voices_qwen() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["--backend", "qwen", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Chelsie"));
}

#[test]
fn test_list_voices_qwen_native() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["--backend", "qwen-native", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("voice clones"));
}

#[test]
fn test_no_text_no_stdin() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Text cannot be empty")
                .or(predicate::str::contains("No text provided")),
        );
}

#[cfg(target_os = "macos")]
#[test]
fn test_stdin_pipe() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["--backend", "say"])
        .write_stdin("Hello from stdin")
        .assert()
        .success();
}

// --- Config subcommand ---

#[test]
fn test_config_show() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backend:"))
        .stdout(predicate::str::contains("(default)"));
}

#[test]
fn test_config_set_and_show() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    // Set
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "lang", "fr"])
        .assert()
        .success()
        .stdout(predicate::str::contains("lang = fr"));

    // Show
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("lang:    fr"));
}

#[test]
fn test_config_set_invalid_key() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "invalid", "value"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown preference"));
}

#[test]
fn test_config_reset() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    // Set then reset
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "lang", "fr"])
        .assert()
        .success();

    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "reset"])
        .assert()
        .success()
        .stdout(predicate::str::contains("reset"));

    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("lang:    (default)"));
}

// --- Clone subcommand ---

#[test]
fn test_clone_list_empty() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["clone", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No voice clones"));
}

#[test]
fn test_clone_add_missing_audio() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["clone", "add", "test", "--audio", "/nonexistent.wav"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_clone_add_and_list() {
    let tmp_db = tempfile::NamedTempFile::new().unwrap();
    let tmp_audio = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
    let audio_path = tmp_audio.path().to_string_lossy().to_string();

    // Add
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp_db.path())
        .args(["clone", "add", "testvoice", "--audio", &audio_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    // List
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp_db.path())
        .args(["clone", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("testvoice"));
}

#[test]
fn test_clone_remove() {
    let tmp_db = tempfile::NamedTempFile::new().unwrap();
    let tmp_audio = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
    let audio_path = tmp_audio.path().to_string_lossy().to_string();

    // Add then remove
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp_db.path())
        .args(["clone", "add", "todel", "--audio", &audio_path])
        .assert()
        .success();

    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp_db.path())
        .args(["clone", "remove", "todel"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));
}

#[test]
fn test_clone_remove_not_found() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["clone", "remove", "ghost"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not found"));
}

// --- Stats subcommand ---

#[test]
fn test_stats_empty() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No usage recorded yet."));
}

// --- Init subcommand ---

#[test]
fn test_init_creates_files() {
    let dir = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("vox")
        .args(["init", "-m", "cli"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("CLAUDE.md configured"))
        .stdout(predicate::str::contains("settings.json configured"));

    assert!(dir.path().join("CLAUDE.md").exists());
    assert!(dir.path().join(".claude/settings.json").exists());
}

#[test]
fn test_init_idempotent() {
    let dir = tempfile::tempdir().unwrap();

    // First run
    cargo_bin_cmd!("vox")
        .args(["init", "-m", "cli"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Second run
    cargo_bin_cmd!("vox")
        .args(["init", "-m", "cli"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("already configured"));
}

// --- Help subcommands ---

#[test]
fn test_clone_help() {
    cargo_bin_cmd!("vox")
        .args(["clone", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("voice clones"));
}

#[test]
fn test_config_help() {
    cargo_bin_cmd!("vox")
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("preferences"));
}
