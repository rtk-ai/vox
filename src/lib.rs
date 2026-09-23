//! vox — Cross-platform TTS CLI with MCP server for AI assistants.
//!
//! Four backends: `say` (macOS native), `qwen` (MLX Python), `qwen-native` (pure Rust),
//! `kokoro` (pure Rust). Exposes 14 MCP tools over stdio for integration with
//! Claude Code, Cursor, VS Code, and 11 other AI tools.

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
pub mod mcp;
pub mod mic;
pub mod pack;
pub mod stt;
pub mod tui;
