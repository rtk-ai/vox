use vox::backend::TtsBackend;
use vox::backend::pocket::PocketBackend;

#[test]
fn test_pocket_name() {
    let backend = PocketBackend;
    assert_eq!(backend.name(), "pocket");
}

#[test]
fn test_pocket_list_voices() {
    let backend = PocketBackend;
    let voices = backend.list_voices().unwrap();
    assert_eq!(voices.len(), 8);
    assert!(voices.contains(&"alba".to_string()));
}

#[test]
fn test_pocket_is_available() {
    let backend = PocketBackend;
    assert!(backend.is_available());
}

#[test]
fn test_pocket_in_supported_backends() {
    assert!(vox::backend::supported_backends().contains(&"pocket"));
}

#[test]
fn test_pocket_get_backend() {
    let backend = vox::backend::get_backend("pocket").unwrap();
    assert_eq!(backend.name(), "pocket");
}

#[test]
fn test_default_backend_for_lang() {
    use vox::config::default_backend_for_lang;
    assert_eq!(default_backend_for_lang(None), "pocket");
    assert_eq!(default_backend_for_lang(Some("en")), "pocket");
    assert_eq!(default_backend_for_lang(Some("fr")), "piper");
    assert_eq!(default_backend_for_lang(Some("de")), "piper");
}
