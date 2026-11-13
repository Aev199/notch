//! Platform-independent logic of the Notch overlay.
//!
//! Everything here is plain Rust without Tauri or Win32, so it can be unit-tested
//! on any OS. The Tauri shell (`src-tauri`) wires these pieces to the real
//! clipboard, window and network.

pub mod clipboard;
pub mod hover;
pub mod ocr;
pub mod settings;
pub mod shelf;
pub mod text_history;
pub mod translate;
pub mod update;
