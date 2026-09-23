//! STT module tests that do not require downloading a model.

use vox::stt;

#[test]
fn default_model_is_multilingual_small() {
    assert_eq!(stt::DEFAULT_MODEL, "openai/whisper-small");
    assert_eq!(stt::QUALITY_MODEL, "openai/whisper-large-v3-turbo");
    assert_eq!(stt::model_id(None), stt::DEFAULT_MODEL);
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
