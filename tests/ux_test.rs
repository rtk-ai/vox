//! UX tests — error messages, CLI output, edge cases.

use assert_cmd::Command;
use predicates::prelude::*;

// ---------------------------------------------------------------------------
// Helpful error messages on invalid input
// ---------------------------------------------------------------------------

#[test]
fn empty_text_shows_helpful_error() {
    Command::cargo_bin("vox")
        .unwrap()
        .args(["  ", "  "])
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty"));
}

#[test]
fn invalid_backend_shows_available_options() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "backend", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("piper").or(predicate::str::contains("qwen-native")));
}

#[test]
fn invalid_lang_shows_supported_list() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "lang", "zz"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Supported"));
}

#[test]
fn invalid_gender_shows_valid_options() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "gender", "neutral"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("feminine").and(predicate::str::contains("masculine")));
}

#[test]
fn invalid_style_shows_valid_options() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "style", "angry"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("calm"));
}

#[test]
fn invalid_rate_shows_error() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "rate", "not_a_number"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("positive integer").or(predicate::str::contains("Rate")));
}

#[test]
fn unknown_preference_key_shows_valid_keys() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "foo", "bar"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Unknown preference")
                .and(predicate::str::contains("Valid keys")),
        );
}

// ---------------------------------------------------------------------------
// Stats output is readable on empty DB
// ---------------------------------------------------------------------------

#[test]
fn stats_on_fresh_db_says_no_usage() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No usage recorded yet."));
}

// ---------------------------------------------------------------------------
// Config show works on fresh DB
// ---------------------------------------------------------------------------

#[test]
fn config_show_on_fresh_db_shows_defaults() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backend").and(predicate::str::contains("(default)")));
}

// ---------------------------------------------------------------------------
// Config set/show roundtrip
// ---------------------------------------------------------------------------

#[test]
fn config_set_then_show_reflects_change() {
    let tmp = tempfile::NamedTempFile::new().unwrap();

    // Set lang to fr
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "lang", "fr"])
        .assert()
        .success();

    // Show should reflect fr
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fr"));
}

// ---------------------------------------------------------------------------
// Config reset clears everything
// ---------------------------------------------------------------------------

#[test]
fn config_reset_clears_preferences() {
    let tmp = tempfile::NamedTempFile::new().unwrap();

    // Set something
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "lang", "ja"])
        .assert()
        .success();

    // Reset
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "reset"])
        .assert()
        .success();

    // Show should not contain ja
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ja").not());
}

// ---------------------------------------------------------------------------
// Clone operations error messages
// ---------------------------------------------------------------------------

#[test]
fn clone_remove_nonexistent_shows_message() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["clone", "remove", "does_not_exist"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not found").or(predicate::str::contains("No")));
}

#[test]
fn clone_list_empty_shows_no_clones() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    Command::cargo_bin("vox")
        .unwrap()
        .env("VOX_DB_PATH", tmp.path())
        .args(["clone", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No voice clones"));
}

// ---------------------------------------------------------------------------
// Help and version output
// ---------------------------------------------------------------------------

#[test]
fn help_lists_all_subcommands() {
    Command::cargo_bin("vox")
        .unwrap()
        .args(["--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("config")
                .and(predicate::str::contains("clone"))
                .and(predicate::str::contains("stats"))
                .and(predicate::str::contains("init"))
                .and(predicate::str::contains("serve"))
                .and(predicate::str::contains("pack")),
        );
}

#[test]
fn version_outputs_semver() {
    Command::cargo_bin("vox")
        .unwrap()
        .args(["--version"])
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"vox \d+\.\d+\.\d+").unwrap());
}
