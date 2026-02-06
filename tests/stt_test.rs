#![cfg(target_os = "macos")]

use std::ffi::OsStr;

use vox::stt;

#[test]
fn build_transcribe_command_basic() {
    let cmd = stt::build_transcribe_command("/tmp/audio.wav", None);
    let args: Vec<&OsStr> = cmd.get_args().collect();
    assert_eq!(cmd.get_program(), "python3");
    assert!(args.contains(&OsStr::new("-m")));
    assert!(args.contains(&OsStr::new("mlx_audio.stt.generate")));
    assert!(args.contains(&OsStr::new("--audio")));
    assert!(args.contains(&OsStr::new("/tmp/audio.wav")));
    assert!(args.contains(&OsStr::new("--format")));
    assert!(args.contains(&OsStr::new("txt")));
}

#[test]
fn build_transcribe_command_with_language() {
    let cmd = stt::build_transcribe_command("/tmp/audio.wav", Some("fr"));
    let args: Vec<&OsStr> = cmd.get_args().collect();
    assert!(args.contains(&OsStr::new("--language")));
    assert!(args.contains(&OsStr::new("fr")));
}

#[test]
fn build_transcribe_command_no_language_flag_when_none() {
    let cmd = stt::build_transcribe_command("/tmp/audio.wav", None);
    let args: Vec<&OsStr> = cmd.get_args().collect();
    assert!(!args.contains(&OsStr::new("--language")));
}

#[test]
fn build_transcribe_command_preserves_audio_path() {
    let cmd = stt::build_transcribe_command("/home/user/recording.wav", None);
    let args: Vec<&OsStr> = cmd.get_args().collect();
    assert!(args.contains(&OsStr::new("/home/user/recording.wav")));
}

#[test]
fn build_transcribe_command_arg_order() {
    let cmd = stt::build_transcribe_command("/tmp/a.wav", Some("en"));
    let args: Vec<&OsStr> = cmd.get_args().collect();
    // -m must come before mlx_audio.stt.generate
    let m_pos = args.iter().position(|a| *a == "-m").unwrap();
    let mod_pos = args
        .iter()
        .position(|a| *a == "mlx_audio.stt.generate")
        .unwrap();
    assert!(m_pos < mod_pos);
    // --audio must come before --language
    let audio_pos = args.iter().position(|a| *a == "--audio").unwrap();
    let lang_pos = args.iter().position(|a| *a == "--language").unwrap();
    assert!(audio_pos < lang_pos);
}
