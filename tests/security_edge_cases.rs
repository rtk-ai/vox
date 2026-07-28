
//! Security edge cases tests

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
#[cfg(target_os = "macos")]
fn command_injection_in_say_backend_is_prevented() {
    // This text should be read literally, not execute `ls`
    let malicious_text = "; ls -la #";
    cargo_bin_cmd!("vox")
        .args(["-b", "say", malicious_text])
        .assert()
        .success()
        // We can't check for audio output, but we can check that it didn't print file listings
        .stdout(predicate::str::contains("Cargo.toml").not());
}

#[test]
fn command_injection_in_qwen_backend_is_prevented() {
    // This text should be passed to python, not executed by shell
    let malicious_text = "; import os; os.system('echo pwned') #";
    cargo_bin_cmd!("vox")
        .args(["-b", "qwen", malicious_text])
        .assert()
        // The python script for qwen shouldn't be vulnerable, but we check anyway
        .stdout(predicate::str::contains("pwned").not());
}

#[test]
fn path_traversal_in_voice_name_is_handled() {
    cargo_bin_cmd!("vox")
        .args(["-v", "../../some/path"])
        .assert()
        .success() // Should probably succeed, finding no such voice
        .stderr(predicate::str::contains("not found").or(predicate::str::is_empty()));
}

