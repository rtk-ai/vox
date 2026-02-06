#![cfg(target_os = "macos")]

use vox::backend::say::SayBackend;
use vox::backend::{SpeakOptions, TtsBackend};

#[test]
fn test_say_is_available() {
    let backend = SayBackend;
    assert!(backend.is_available());
}

#[test]
fn test_say_name() {
    let backend = SayBackend;
    assert_eq!(backend.name(), "say");
}

#[test]
fn test_say_list_voices() {
    let backend = SayBackend;
    let voices = backend.list_voices().unwrap();
    assert!(!voices.is_empty());
}

#[test]
fn test_say_build_command_basic() {
    let opts = SpeakOptions::default();
    let cmd = SayBackend::build_command("hello", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, &["hello"]);
    assert_eq!(cmd.get_program(), "/usr/bin/say");
}

#[test]
fn test_say_build_command_with_voice() {
    let opts = SpeakOptions {
        voice: Some("Thomas".into()),
        ..Default::default()
    };
    let cmd = SayBackend::build_command("bonjour", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, &["-v", "Thomas", "bonjour"]);
}

#[test]
fn test_say_build_command_with_rate() {
    let opts = SpeakOptions {
        rate: Some(200),
        ..Default::default()
    };
    let cmd = SayBackend::build_command("fast", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, &["-r", "200", "fast"]);
}

#[test]
fn test_say_build_command_with_voice_and_rate() {
    let opts = SpeakOptions {
        voice: Some("Samantha".into()),
        rate: Some(150),
        ..Default::default()
    };
    let cmd = SayBackend::build_command("hello", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, &["-v", "Samantha", "-r", "150", "hello"]);
}
