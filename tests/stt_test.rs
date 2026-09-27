//! STT module tests that do not require downloading a model.

use vox::stt;

/// Resolution order, in one test because these assertions share process-wide
/// environment variables and would race if split across parallel tests.
///
/// The default is `base`, chosen on measurements (see models.toml): with no
/// flag, no env var, no preference and no user models.toml the resolver has to
/// land on it, so the bundled TOML and the Rust fallback cannot drift apart.
/// And the env var has to beat a stored preference, otherwise `vox config set
/// stt_model` would permanently lock out `VOX_STT_MODEL=...`.
#[test]
fn model_resolution_order() {
    assert_eq!(stt::DEFAULT_MODEL, "openai/whisper-base");
    assert_eq!(stt::QUALITY_MODEL, "openai/whisper-large-v3-turbo");

    // An empty config dir: no user models.toml, no database, no preference.
    let empty = tempfile::tempdir().unwrap();
    // Safety: no other test in this binary reads these variables — the rest
    // resolve their model from an explicit override, which short-circuits.
    unsafe {
        std::env::set_var("VOX_CONFIG_DIR", empty.path());
        std::env::remove_var("VOX_DB_PATH");
        std::env::remove_var("VOX_STT_MODEL");
    }
    assert_eq!(stt::model_id(None), stt::DEFAULT_MODEL);

    unsafe { std::env::set_var("VOX_STT_MODEL", "openai/whisper-tiny") };
    assert_eq!(stt::model_id(None), "openai/whisper-tiny");
    assert_eq!(
        stt::model_id(Some("openai/whisper-small")),
        "openai/whisper-small",
        "an explicit flag must outrank the environment"
    );

    unsafe { std::env::remove_var("VOX_STT_MODEL") };
}

#[test]
fn model_override_wins() {
    assert_eq!(
        stt::model_id(Some("openai/whisper-small")),
        "openai/whisper-small"
    );
}

#[test]
fn language_tokens_follow_whisper_format() {
    assert_eq!(stt::language_token("fr"), "<|fr|>");
    assert_eq!(stt::language_token("EN"), "<|en|>");
}

#[test]
fn all_vox_languages_are_supported() {
    for l in [
        "en", "fr", "es", "de", "it", "pt", "zh", "ja", "ko", "ru", "ar", "nl",
    ] {
        assert!(stt::is_supported_language(l), "{l}");
    }
    assert!(!stt::is_supported_language("xx"));
}

#[test]
fn unsupported_language_is_rejected_without_model() {
    let err = stt::transcribe_samples(&vec![0.0f32; 16_000], Some("xx")).unwrap_err();
    assert!(err.to_string().contains("not supported"));
}

#[test]
fn tiny_input_returns_empty_without_model() {
    assert_eq!(stt::transcribe_samples(&[0.0; 10], None).unwrap(), "");
}
