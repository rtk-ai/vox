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
