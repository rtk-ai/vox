#![cfg(target_os = "macos")]

use std::path::Path;

use vox::backend::qwen::QwenBackend;
use vox::backend::{SpeakOptions, TtsBackend};

#[test]
fn test_qwen_name() {
    let backend = QwenBackend;
    assert_eq!(backend.name(), "qwen");
}

#[test]
fn test_qwen_list_voices() {
    let backend = QwenBackend;
    let voices = backend.list_voices().unwrap();
    assert!(voices.contains(&"Chelsie".to_string()));
    assert!(voices.contains(&"Ryan".to_string()));
}

#[test]
fn test_qwen_build_generate_command_basic() {
    let opts = SpeakOptions::default();
    let cmd = QwenBackend::build_generate_command("hello", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(cmd.get_program(), "python3");
    assert!(args.contains(&std::ffi::OsStr::new("--text")));
    assert!(args.contains(&std::ffi::OsStr::new("hello")));
    assert!(args.contains(&std::ffi::OsStr::new("--model")));
    assert!(args.contains(&std::ffi::OsStr::new("--play")));
    assert!(args.contains(&std::ffi::OsStr::new("--stream")));
}

#[test]
fn test_qwen_build_generate_command_with_voice() {
    let opts = SpeakOptions {
        voice: Some("Ryan".into()),
        ..Default::default()
    };
    let cmd = QwenBackend::build_generate_command("hello", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert!(args.contains(&std::ffi::OsStr::new("--voice")));
    assert!(args.contains(&std::ffi::OsStr::new("Ryan")));
}

#[test]
fn test_qwen_build_generate_command_with_lang() {
    let opts = SpeakOptions {
        lang: Some("fr".into()),
        ..Default::default()
    };
    let cmd = QwenBackend::build_generate_command("bonjour", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert!(args.contains(&std::ffi::OsStr::new("--lang_code")));
    assert!(args.contains(&std::ffi::OsStr::new("fr")));
}

#[test]
fn test_qwen_build_generate_command_with_ref_audio() {
    let opts = SpeakOptions {
        ref_audio: Some("/path/to/ref.wav".into()),
        ..Default::default()
    };
    let cmd = QwenBackend::build_generate_command("hello", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert!(args.contains(&std::ffi::OsStr::new("--ref_audio")));
    assert!(args.contains(&std::ffi::OsStr::new("/path/to/ref.wav")));
}

#[test]
fn test_qwen_build_generate_command_with_ref_audio_and_text() {
    let opts = SpeakOptions {
        ref_audio: Some("/path/to/ref.wav".into()),
        ref_text: Some("reference transcription".into()),
        ..Default::default()
    };
    let cmd = QwenBackend::build_generate_command("hello", &opts);
    let args: Vec<_> = cmd.get_args().collect();
    assert!(args.contains(&std::ffi::OsStr::new("--ref_audio")));
    assert!(args.contains(&std::ffi::OsStr::new("/path/to/ref.wav")));
    assert!(args.contains(&std::ffi::OsStr::new("--ref_text")));
    assert!(args.contains(&std::ffi::OsStr::new("reference transcription")));
}

// --- build_generate_command_to_file ---

#[test]
fn test_qwen_build_generate_command_to_file_basic() {
    let opts = SpeakOptions::default();
    let dir = Path::new("/tmp/test_output");
    let cmd = QwenBackend::build_generate_command_to_file("hello", &opts, dir);
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(cmd.get_program(), "python3");
    assert!(args.contains(&std::ffi::OsStr::new("--text")));
    assert!(args.contains(&std::ffi::OsStr::new("hello")));
    assert!(args.contains(&std::ffi::OsStr::new("--model")));
    // Must NOT have --play or --stream
    assert!(!args.contains(&std::ffi::OsStr::new("--play")));
    assert!(!args.contains(&std::ffi::OsStr::new("--stream")));
    // Must have current_dir set
    assert_eq!(cmd.get_current_dir(), Some(dir.as_ref()));
}

#[test]
fn test_qwen_build_generate_command_to_file_with_voice() {
    let opts = SpeakOptions {
        voice: Some("Ryan".into()),
        ..Default::default()
    };
    let dir = Path::new("/tmp/test_output");
    let cmd = QwenBackend::build_generate_command_to_file("hello", &opts, dir);
    let args: Vec<_> = cmd.get_args().collect();
    assert!(args.contains(&std::ffi::OsStr::new("--voice")));
    assert!(args.contains(&std::ffi::OsStr::new("Ryan")));
    assert!(!args.contains(&std::ffi::OsStr::new("--play")));
    assert!(!args.contains(&std::ffi::OsStr::new("--stream")));
}

#[test]
fn test_qwen_build_generate_command_to_file_with_ref_audio() {
    let opts = SpeakOptions {
        ref_audio: Some("/path/to/ref.wav".into()),
        ref_text: Some("reference transcription".into()),
        ..Default::default()
    };
    let dir = Path::new("/tmp/test_output");
    let cmd = QwenBackend::build_generate_command_to_file("hello", &opts, dir);
    let args: Vec<_> = cmd.get_args().collect();
    assert!(args.contains(&std::ffi::OsStr::new("--ref_audio")));
    assert!(args.contains(&std::ffi::OsStr::new("/path/to/ref.wav")));
    assert!(args.contains(&std::ffi::OsStr::new("--ref_text")));
    assert!(args.contains(&std::ffi::OsStr::new("reference transcription")));
    assert!(!args.contains(&std::ffi::OsStr::new("--play")));
    assert!(!args.contains(&std::ffi::OsStr::new("--stream")));
    assert_eq!(cmd.get_current_dir(), Some(dir.as_ref()));
}

// --- split_sentences ---

#[test]
fn test_split_sentences_basic() {
    // Short sentences are merged to reduce subprocess calls
    let result = QwenBackend::split_sentences("Hello world. How are you?");
    assert_eq!(result, vec!["Hello world. How are you?"]);
}

#[test]
fn test_split_sentences_long_splits() {
    // Long sentences are kept separate
    let long1 = "This is a very long sentence that contains enough characters to exceed the minimum chunk threshold for splitting.";
    let long2 = "And here is another lengthy sentence that also exceeds the threshold so they should remain separate chunks.";
    let text = format!("{long1} {long2}");
    let result = QwenBackend::split_sentences(&text);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_split_sentences_no_punctuation() {
    let result = QwenBackend::split_sentences("Hello world");
    assert_eq!(result, vec!["Hello world"]);
}

#[test]
fn test_split_sentences_multiple_types() {
    // Short sentences are merged to reduce subprocess overhead
    let result = QwenBackend::split_sentences("Bonjour! Comment vas-tu? Bien. Merci; au revoir");
    assert_eq!(
        result,
        vec!["Bonjour! Comment vas-tu? Bien. Merci; au revoir"]
    );
}

#[test]
fn test_split_sentences_empty() {
    let result = QwenBackend::split_sentences("");
    assert!(result.is_empty());
}

#[test]
fn test_split_sentences_single_sentence() {
    let result = QwenBackend::split_sentences("Just one sentence.");
    assert_eq!(result, vec!["Just one sentence."]);
}
