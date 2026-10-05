//! vox — local text-to-speech and speech-to-text for AI coding agents.
//!
//! Speaking goes through one of the backends in [`backend`]: `pocket` (the
//! default for English), `piper` (the default for other languages),
//! `qwen-native` (voice cloning), `kokoro` (behind the `kokoro` feature) and
//! `say` (macOS). Listening is Whisper, in [`stt`]. Everything is Rust: no
//! Python at build or run time. [`mcp`] exposes 14 tools over stdio, and
//! [`init`] configures 14 AI tools to use them.

pub mod accel;
pub mod audio;
pub mod backend;
#[cfg(target_os = "macos")]
pub mod chat;
pub mod clone;
pub mod config;
pub mod daemon;
pub mod db;
pub mod init;
pub mod input;
pub mod lang;
pub mod levels;
pub mod mcp;
pub mod mic;
pub mod pack;
pub mod stt;
pub mod timing;
pub mod tui;
