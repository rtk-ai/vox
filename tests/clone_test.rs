use std::io::Write;

use vox::clone;
use vox::db;

#[test]
fn test_validate_audio_missing_file() {
    assert!(clone::validate_audio("/nonexistent/path.wav").is_err());
}

#[test]
fn test_validate_audio_bad_extension() {
    let tmp = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    assert!(clone::validate_audio(&path).is_err());
}

#[test]
fn test_validate_audio_valid_wav() {
    let tmp = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    assert!(clone::validate_audio(&path).is_ok());
}

#[test]
fn test_validate_audio_valid_mp3() {
    let tmp = tempfile::Builder::new().suffix(".mp3").tempfile().unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    assert!(clone::validate_audio(&path).is_ok());
}

#[test]
fn test_validate_audio_valid_flac() {
    let tmp = tempfile::Builder::new().suffix(".flac").tempfile().unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    assert!(clone::validate_audio(&path).is_ok());
}

#[test]
fn test_resolve_voice_found() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(&conn, "patrick", "/p.wav", Some("hello")).unwrap();
    let result = clone::resolve_voice(&conn, "patrick").unwrap();
    assert!(result.is_some());
    let vc = result.unwrap();
    assert_eq!(vc.name, "patrick");
    assert_eq!(vc.ref_audio, "/p.wav");
}

#[test]
fn test_resolve_voice_not_found() {
    let conn = db::open_in_memory().unwrap();
    let result = clone::resolve_voice(&conn, "nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_validate_audio_no_extension() {
    let mut tmp = tempfile::Builder::new().tempfile().unwrap();
    write!(tmp, "data").unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    assert!(clone::validate_audio(&path).is_err());
}

// --- The backend a voice is spoken with ---

#[test]
fn explicit_backend_is_always_respected() {
    // Naming the default backend is still naming a backend.
    for (stored, lang) in [
        (None, Some("fr")),
        (Some("piper"), None),
        (Some("say"), None),
    ] {
        assert_eq!(
            clone::speak_backend(Some("pocket"), stored, lang, false, false),
            "pocket"
        );
    }
    // Even for a clone, and even when the named backend cannot clone.
    assert_eq!(
        clone::speak_backend(Some("pocket"), None, None, true, false),
        "pocket"
    );
    assert_eq!(
        clone::speak_backend(Some("piper"), Some("qwen-native"), None, true, true),
        "piper"
    );
}

#[test]
fn without_a_flag_the_preference_then_the_language_decide() {
    assert_eq!(
        clone::speak_backend(None, None, None, false, false),
        "pocket"
    );
    assert_eq!(
        clone::speak_backend(None, None, Some("fr"), false, false),
        "piper"
    );
    assert_eq!(
        clone::speak_backend(None, Some("qwen-native"), Some("fr"), false, false),
        "qwen-native"
    );
}

#[test]
fn a_clone_goes_to_pocket_only_when_pocket_can_clone() {
    // The default backend, with and without the gated weights.
    assert_eq!(clone::speak_backend(None, None, None, true, true), "pocket");
    assert_eq!(
        clone::speak_backend(None, None, None, true, false),
        "qwen-native"
    );
    // A stored pocket preference follows the same rule.
    assert_eq!(
        clone::speak_backend(None, Some("pocket"), None, true, false),
        "qwen-native"
    );
    // Backends that cannot clone at all hand over to qwen-native, and the
    // token does not pull a French clone onto the English-only pocket.
    for can in [true, false] {
        assert_eq!(
            clone::speak_backend(None, Some("piper"), None, true, can),
            "qwen-native"
        );
        assert_eq!(
            clone::speak_backend(None, None, Some("fr"), true, can),
            "qwen-native"
        );
        assert_eq!(
            clone::speak_backend(None, Some("qwen-native"), None, true, can),
            "qwen-native"
        );
    }
}

#[test]
fn only_pocket_and_qwen_native_can_clone() {
    assert!(clone::can_clone("pocket"));
    assert!(clone::can_clone("qwen-native"));
    for backend in ["piper", "say", "kokoro"] {
        assert!(!clone::can_clone(backend), "{backend}");
    }
}

#[test]
fn test_validate_audio_rejects_m4a() {
    // No decoder for it in this build: refusing the name beats failing later.
    let tmp = tempfile::Builder::new().suffix(".m4a").tempfile().unwrap();
    let err = clone::validate_audio(&tmp.path().to_string_lossy()).unwrap_err();
    assert!(err.to_string().contains("Unsupported audio format: .m4a"));
}

// --- The name of a new clone ---

/// The reference is a file named after the clone, so a name is checked before
/// `clone add` writes and before `clone record` opens the microphone.
#[test]
fn a_new_clone_needs_a_free_plain_name() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(&conn, "me", "/clones/me.wav", None).unwrap();

    let free = clone::new_reference_path(&conn, "other").unwrap();
    assert!(free.is_absolute());
    assert!(free.ends_with("clones/other.wav"), "{}", free.display());

    let taken = clone::new_reference_path(&conn, "me").unwrap_err();
    assert!(taken.to_string().contains("already exists"), "{taken}");

    for name in ["../me", "a/b", "..", ""] {
        let err = clone::new_reference_path(&conn, name).unwrap_err();
        assert!(
            err.to_string().contains("Invalid clone name"),
            "{name}: {err}"
        );
    }
}

/// `Me.wav` and `me.wav` are one file on a case-insensitive file system:
/// adding `Me` replaced the reference of `me`, and both clones stayed listed.
#[test]
fn a_name_that_differs_only_by_case_is_taken() {
    let conn = db::open_in_memory().unwrap();
    db::add_clone(&conn, "me", "/clones/me.wav", None).unwrap();

    for name in ["Me", "ME"] {
        let err = clone::new_reference_path(&conn, name).unwrap_err();
        // The message names the clone that exists, the one to remove.
        assert!(err.to_string().contains("'me' already exists"), "{err}");
    }
}
