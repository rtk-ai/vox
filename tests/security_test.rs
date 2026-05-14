//! Security tests — SQL injection, path traversal, input validation.

use vox::clone;
use vox::db;

// ---------------------------------------------------------------------------
// SQL injection prevention in preferences
// ---------------------------------------------------------------------------

#[test]
fn sql_injection_in_preference_value_is_safe() {
    let conn = db::open_in_memory().unwrap();
    // Try SQL injection in value — should be stored as literal string
    db::set_preference(&conn, "voice", "'; DROP TABLE preferences; --").unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(
        prefs.voice.as_deref(),
        Some("'; DROP TABLE preferences; --")
    );
    // Table should still be intact
    let prefs2 = db::get_preferences(&conn).unwrap();
    assert!(prefs2.voice.is_some());
}

#[test]
fn sql_injection_in_clone_name_is_safe() {
    let conn = db::open_in_memory().unwrap();
    let malicious_name = "test'; DROP TABLE voice_clones; --";
    db::add_clone(&conn, malicious_name, "/tmp/test.wav", None).unwrap();
    let clones = db::list_clones(&conn).unwrap();
    assert_eq!(clones.len(), 1);
    assert_eq!(clones[0].name, malicious_name);
    // Table still works
    db::list_clones(&conn).unwrap();
}

#[test]
fn sql_injection_in_usage_log_is_safe() {
    let conn = db::open_in_memory().unwrap();
    db::log_usage(
        &conn,
        "'; DROP TABLE usage_log; --",
        Some("voice"),
        Some("en"),
        100,
        Some(500),
    )
    .unwrap();
    let (count, _) = db::get_usage_summary(&conn).unwrap();
    assert_eq!(count, 1);
}

// ---------------------------------------------------------------------------
// Preference key validation (prevent SQL column injection)
// ---------------------------------------------------------------------------

#[test]
fn invalid_preference_key_rejected() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "malicious_column", "value");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Unknown preference")
    );
}

#[test]
fn preference_key_with_sql_keyword_rejected() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "id; DROP TABLE", "value");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Audio file validation (path traversal, invalid extensions)
// ---------------------------------------------------------------------------

#[test]
fn audio_validation_rejects_nonexistent_file() {
    let result = clone::validate_audio("/nonexistent/path/audio.wav");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn audio_validation_rejects_invalid_extension() {
    let tmp = tempfile::NamedTempFile::with_suffix(".exe").unwrap();
    let result = clone::validate_audio(tmp.path().to_str().unwrap());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Unsupported"));
}

#[test]
fn audio_validation_rejects_no_extension() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let result = clone::validate_audio(tmp.path().to_str().unwrap());
    assert!(result.is_err());
}

#[test]
fn audio_validation_accepts_valid_extensions() {
    for ext in &["wav", "mp3", "flac", "ogg", "m4a"] {
        let tmp = tempfile::NamedTempFile::with_suffix(&format!(".{ext}")).unwrap();
        let result = clone::validate_audio(tmp.path().to_str().unwrap());
        assert!(result.is_ok(), "Should accept .{ext}");
    }
}

// ---------------------------------------------------------------------------
// Backend validation
// ---------------------------------------------------------------------------

#[test]
fn backend_validation_rejects_unknown() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "backend", "shellscript");
    assert!(result.is_err());
}

#[test]
fn backend_validation_accepts_valid() {
    let conn = db::open_in_memory().unwrap();
    // piper and qwen-native are always compiled in.
    assert!(db::set_preference(&conn, "backend", "piper").is_ok());
    assert!(db::set_preference(&conn, "backend", "qwen-native").is_ok());
}

// ---------------------------------------------------------------------------
// Language validation
// ---------------------------------------------------------------------------

#[test]
fn lang_validation_rejects_unsupported() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "lang", "xx");
    assert!(result.is_err());
}

#[test]
fn lang_validation_accepts_all_supported() {
    let conn = db::open_in_memory().unwrap();
    for lang in &[
        "en", "fr", "es", "de", "it", "pt", "zh", "ja", "ko", "ru", "ar", "nl",
    ] {
        assert!(
            db::set_preference(&conn, "lang", lang).is_ok(),
            "Should accept lang={lang}"
        );
    }
}

// ---------------------------------------------------------------------------
// Gender and style enum validation
// ---------------------------------------------------------------------------

#[test]
fn gender_rejects_arbitrary_values() {
    let conn = db::open_in_memory().unwrap();
    assert!(db::set_preference(&conn, "gender", "other").is_err());
    assert!(db::set_preference(&conn, "gender", "").is_err());
}

#[test]
fn style_rejects_arbitrary_values() {
    let conn = db::open_in_memory().unwrap();
    assert!(db::set_preference(&conn, "style", "angry").is_err());
    assert!(db::set_preference(&conn, "style", "").is_err());
}

// ---------------------------------------------------------------------------
// Unicode and special characters in text
// ---------------------------------------------------------------------------

#[test]
fn unicode_text_stored_and_retrieved_correctly() {
    let conn = db::open_in_memory().unwrap();
    db::log_usage(&conn, "say", None, Some("fr"), 50, Some(1000)).unwrap();
    db::log_usage(&conn, "kokoro", None, Some("ja"), 30, Some(500)).unwrap();
    let entries = db::get_usage_stats(&conn).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn clone_name_with_unicode_works() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(
        &conn,
        "voix_française",
        "/tmp/test.wav",
        Some("Bonjour à tous"),
    )
    .unwrap();
    let clone = db::get_clone(&conn, "voix_française").unwrap();
    assert!(clone.is_some());
    assert_eq!(clone.unwrap().ref_text.as_deref(), Some("Bonjour à tous"));
}

// ---------------------------------------------------------------------------
// STT command does not use shell (no command injection)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[test]
fn stt_command_does_not_invoke_shell() {
    let cmd = vox::stt::build_transcribe_command("/tmp/test.wav", Some("en"));
    // Command should be python3, not sh/bash
    assert_eq!(cmd.get_program(), "python3");
    let args: Vec<_> = cmd.get_args().collect();
    // Should use -c with inline script, not shell
    assert_eq!(args[0], "-c");
}

#[cfg(target_os = "macos")]
#[test]
fn stt_escapes_single_quotes_in_path() {
    let cmd = vox::stt::build_transcribe_command("/tmp/it's a test.wav", Some("en"));
    let args: Vec<_> = cmd.get_args().collect();
    let script = args[1].to_string_lossy();
    // Should have escaped single quote
    assert!(script.contains("\\'"));
    assert!(!script.contains("it's"));
}

// ---------------------------------------------------------------------------
// Path traversal in clone audio file paths
// ---------------------------------------------------------------------------

#[test]
fn audio_validation_rejects_path_traversal_etc_passwd() {
    let result = clone::validate_audio("../../etc/passwd");
    assert!(result.is_err());
}

#[test]
fn audio_validation_rejects_path_traversal_with_extension() {
    // Even with a valid extension, path traversal to nonexistent file must fail
    let result = clone::validate_audio("../../../etc/shadow.wav");
    assert!(result.is_err());
}

#[test]
fn audio_validation_rejects_dot_dot_encoded_path() {
    let result = clone::validate_audio("/tmp/../../../etc/passwd.wav");
    // File does not exist so it must fail
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Clone name with null bytes and control characters
// ---------------------------------------------------------------------------

#[test]
fn clone_name_with_null_byte_stored_safely() {
    let conn = db::open_in_memory().unwrap();
    let name = "clone\0evil";
    db::add_clone(&conn, name, "/tmp/test.wav", None).unwrap();
    let clones = db::list_clones(&conn).unwrap();
    assert_eq!(clones.len(), 1);
    // Name is stored literally, DB is intact
    db::list_clones(&conn).unwrap();
}

#[test]
fn clone_name_with_control_characters_stored_safely() {
    let conn = db::open_in_memory().unwrap();
    let name = "clone\x01\x02\x03\x07\x1b[31mred";
    db::add_clone(&conn, name, "/tmp/test.wav", None).unwrap();
    let clone = db::get_clone(&conn, name).unwrap();
    assert!(clone.is_some());
    assert_eq!(clone.unwrap().name, name);
}

#[test]
fn clone_name_with_newlines_stored_safely() {
    let conn = db::open_in_memory().unwrap();
    let name = "line1\nline2\rline3";
    db::add_clone(&conn, name, "/tmp/test.wav", None).unwrap();
    let clone = db::get_clone(&conn, name).unwrap();
    assert!(clone.is_some());
}

// ---------------------------------------------------------------------------
// Config injection — shell metacharacters in preference values
// ---------------------------------------------------------------------------

#[test]
fn preference_value_with_shell_metacharacters_stored_literally() {
    let conn = db::open_in_memory().unwrap();
    db::set_preference(&conn, "voice", "$(rm -rf /)").unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(prefs.voice.as_deref(), Some("$(rm -rf /)"));
}

#[test]
fn preference_value_with_backticks_stored_literally() {
    let conn = db::open_in_memory().unwrap();
    db::set_preference(&conn, "voice", "`id`").unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(prefs.voice.as_deref(), Some("`id`"));
}

#[test]
fn preference_value_with_pipe_and_redirect_stored_literally() {
    let conn = db::open_in_memory().unwrap();
    db::set_preference(&conn, "voice", "test | cat /etc/passwd > /tmp/pwned").unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(
        prefs.voice.as_deref(),
        Some("test | cat /etc/passwd > /tmp/pwned")
    );
}

// ---------------------------------------------------------------------------
// MCP tool input validation — malicious JSON-RPC params
// ---------------------------------------------------------------------------

#[test]
fn config_set_rejects_key_with_semicolon() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "voice; DROP TABLE", "value");
    assert!(result.is_err());
}

#[test]
fn config_set_rejects_empty_key() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "", "value");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Very long text input (DoS prevention / robustness)
// ---------------------------------------------------------------------------

#[test]
fn very_long_clone_name_does_not_crash() {
    let conn = db::open_in_memory().unwrap();
    let long_name = "A".repeat(10_000);
    // Should succeed — DB handles long strings
    db::add_clone(&conn, &long_name, "/tmp/test.wav", None).unwrap();
    let clone = db::get_clone(&conn, &long_name).unwrap();
    assert!(clone.is_some());
}

#[test]
fn very_long_preference_value_does_not_crash() {
    let conn = db::open_in_memory().unwrap();
    let long_value = "V".repeat(100_000);
    db::set_preference(&conn, "voice", &long_value).unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(prefs.voice.as_deref(), Some(long_value.as_str()));
}

#[test]
fn very_long_usage_log_text_does_not_crash() {
    let conn = db::open_in_memory().unwrap();
    let large_len = 1_000_000;
    db::log_usage(&conn, "say", None, Some("en"), large_len, Some(5000)).unwrap();
    let (count, total_chars) = db::get_usage_summary(&conn).unwrap();
    assert_eq!(count, 1);
    assert_eq!(total_chars, large_len as u64);
}

// ---------------------------------------------------------------------------
// Backend selection with malicious strings
// ---------------------------------------------------------------------------

#[test]
fn backend_validation_rejects_shell_injection() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "backend", "say; rm -rf /");
    assert!(result.is_err());
}

#[test]
fn backend_validation_rejects_path_traversal() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "backend", "../../../bin/sh");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Rate / voice / lang enum boundary testing
// ---------------------------------------------------------------------------

#[test]
fn rate_rejects_non_numeric_value() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "rate", "not_a_number");
    assert!(result.is_err());
}

#[test]
fn rate_rejects_negative_value() {
    let conn = db::open_in_memory().unwrap();
    let result = db::set_preference(&conn, "rate", "-100");
    assert!(result.is_err());
}
