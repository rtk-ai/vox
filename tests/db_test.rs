use vox::db;

#[test]
fn test_open_in_memory() {
    let conn = db::open_in_memory().unwrap();
    assert!(conn.is_autocommit());
}

// --- Preferences ---

#[test]
fn test_get_preferences_default() {
    let conn = db::open_in_memory().unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert!(prefs.backend.is_none());
    assert!(prefs.voice.is_none());
    assert!(prefs.lang.is_none());
    assert!(prefs.rate.is_none());
    assert!(prefs.gender.is_none());
    assert!(prefs.style.is_none());
    assert!(prefs.model.is_none());
}

#[test]
fn test_set_and_get_preference() {
    let conn = db::open_in_memory().unwrap();
    db::set_preference(&conn, "lang", "fr").unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(prefs.lang.as_deref(), Some("fr"));
    assert!(prefs.backend.is_none());
}

#[test]
fn test_set_multiple_preferences() {
    let conn = db::open_in_memory().unwrap();
    db::set_preference(&conn, "backend", "qwen-native").unwrap();
    db::set_preference(&conn, "lang", "en").unwrap();
    db::set_preference(&conn, "gender", "feminine").unwrap();
    db::set_preference(&conn, "style", "warm").unwrap();
    db::set_preference(&conn, "rate", "150").unwrap();

    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(prefs.backend.as_deref(), Some("qwen-native"));
    assert_eq!(prefs.lang.as_deref(), Some("en"));
    assert_eq!(prefs.gender.as_deref(), Some("feminine"));
    assert_eq!(prefs.style.as_deref(), Some("warm"));
    assert_eq!(prefs.rate, Some(150));
}

#[test]
fn test_set_preference_overwrites() {
    let conn = db::open_in_memory().unwrap();
    db::set_preference(&conn, "lang", "fr").unwrap();
    db::set_preference(&conn, "lang", "en").unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(prefs.lang.as_deref(), Some("en"));
}

#[test]
fn test_set_preference_invalid_key() {
    let conn = db::open_in_memory().unwrap();
    assert!(db::set_preference(&conn, "nonexistent", "value").is_err());
}

#[test]
fn test_set_preference_invalid_gender() {
    let conn = db::open_in_memory().unwrap();
    assert!(db::set_preference(&conn, "gender", "other").is_err());
}

#[test]
fn test_set_preference_invalid_style() {
    let conn = db::open_in_memory().unwrap();
    assert!(db::set_preference(&conn, "style", "angry").is_err());
}

#[test]
fn test_set_preference_invalid_lang() {
    let conn = db::open_in_memory().unwrap();
    assert!(db::set_preference(&conn, "lang", "xx").is_err());
}

#[test]
fn test_set_preference_invalid_rate() {
    let conn = db::open_in_memory().unwrap();
    assert!(db::set_preference(&conn, "rate", "abc").is_err());
}

#[test]
fn test_set_preference_invalid_backend() {
    let conn = db::open_in_memory().unwrap();
    assert!(db::set_preference(&conn, "backend", "nonexistent-backend").is_err());
}

#[test]
fn test_set_preference_accepts_piper() {
    let conn = db::open_in_memory().unwrap();
    db::set_preference(&conn, "backend", "piper").unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert_eq!(prefs.backend.as_deref(), Some("piper"));
}

#[test]
fn test_reset_preferences() {
    let conn = db::open_in_memory().unwrap();
    db::set_preference(&conn, "lang", "fr").unwrap();
    db::reset_preferences(&conn).unwrap();
    let prefs = db::get_preferences(&conn).unwrap();
    assert!(prefs.lang.is_none());
}

// --- Voice Clones ---

#[test]
fn test_add_and_get_clone() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(&conn, "patrick", "/path/to/audio.wav", Some("hello")).unwrap();
    let clone = db::get_clone(&conn, "patrick").unwrap().unwrap();
    assert_eq!(clone.name, "patrick");
    assert_eq!(clone.ref_audio, "/path/to/audio.wav");
    assert_eq!(clone.ref_text.as_deref(), Some("hello"));
    assert!(!clone.created_at.is_empty());
}

#[test]
fn test_add_clone_without_text() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(&conn, "test", "/path/to/audio.wav", None).unwrap();
    let clone = db::get_clone(&conn, "test").unwrap().unwrap();
    assert!(clone.ref_text.is_none());
}

#[test]
fn test_get_clone_not_found() {
    let conn = db::open_in_memory().unwrap();
    let clone = db::get_clone(&conn, "nonexistent").unwrap();
    assert!(clone.is_none());
}

#[test]
fn test_add_clone_duplicate_fails() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(&conn, "patrick", "/path/a.wav", None).unwrap();
    assert!(db::add_clone(&conn, "patrick", "/path/b.wav", None).is_err());
}

#[test]
fn test_list_clones() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(&conn, "bob", "/b.wav", None).unwrap();
    db::add_clone(&conn, "alice", "/a.wav", None).unwrap();
    let clones = db::list_clones(&conn).unwrap();
    assert_eq!(clones.len(), 2);
    assert_eq!(clones[0].name, "alice"); // sorted by name
    assert_eq!(clones[1].name, "bob");
}

#[test]
fn test_list_clones_empty() {
    let conn = db::open_in_memory().unwrap();
    let clones = db::list_clones(&conn).unwrap();
    assert!(clones.is_empty());
}

#[test]
fn test_remove_clone() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(&conn, "patrick", "/p.wav", None).unwrap();
    assert!(db::remove_clone(&conn, "patrick").unwrap());
    assert!(db::get_clone(&conn, "patrick").unwrap().is_none());
}

#[test]
fn test_remove_clone_not_found() {
    let conn = db::open_in_memory().unwrap();
    assert!(!db::remove_clone(&conn, "nonexistent").unwrap());
}

// --- Usage Log ---

#[test]
fn test_log_and_get_usage() {
    let conn = db::open_in_memory().unwrap();
    db::log_usage(&conn, "say", Some("Thomas"), Some("fr"), 42, Some(1200)).unwrap();
    let entries = db::get_usage_stats(&conn).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].backend, "say");
    assert_eq!(entries[0].voice.as_deref(), Some("Thomas"));
    assert_eq!(entries[0].lang.as_deref(), Some("fr"));
    assert_eq!(entries[0].text_len, 42);
    assert_eq!(entries[0].duration_ms, Some(1200));
}

#[test]
fn test_log_usage_minimal() {
    let conn = db::open_in_memory().unwrap();
    db::log_usage(&conn, "qwen", None, None, 10, None).unwrap();
    let entries = db::get_usage_stats(&conn).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].voice.is_none());
    assert!(entries[0].duration_ms.is_none());
}

#[test]
fn test_usage_summary() {
    let conn = db::open_in_memory().unwrap();
    db::log_usage(&conn, "say", None, None, 100, None).unwrap();
    db::log_usage(&conn, "qwen", None, None, 50, None).unwrap();
    let (count, total) = db::get_usage_summary(&conn).unwrap();
    assert_eq!(count, 2);
    assert_eq!(total, 150);
}

#[test]
fn test_usage_summary_empty() {
    let conn = db::open_in_memory().unwrap();
    let (count, total) = db::get_usage_summary(&conn).unwrap();
    assert_eq!(count, 0);
    assert_eq!(total, 0);
}

#[test]
fn test_usage_stats_ordered_desc() {
    let conn = db::open_in_memory().unwrap();
    db::log_usage(&conn, "say", None, None, 10, None).unwrap();
    db::log_usage(&conn, "qwen", None, None, 20, None).unwrap();
    let entries = db::get_usage_stats(&conn).unwrap();
    assert_eq!(entries[0].backend, "qwen"); // most recent first
    assert_eq!(entries[1].backend, "say");
}
