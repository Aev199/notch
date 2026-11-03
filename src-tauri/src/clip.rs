//! Clipboard watcher: an event-driven listener (`WM_CLIPBOARDUPDATE` via
//! clipboard-master, no polling) that routes text to the history and images
//! to the screenshot shelf.

use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use notch_core::text_history::normalize;
use std::io;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{emit_shelf, emit_texts, lock, now_ms, open_island, Shared};
use crate::win;

/// Two copies of the same text within this window (ms) mean "translate it".
/// The lower bound filters apps that write the clipboard twice per copy.
const DOUBLE_COPY_MIN_MS: u64 = 60;
const DOUBLE_COPY_MAX_MS: u64 = 450;

struct Handler {
    app: AppHandle,
    last_text: Option<(String, u64)>,
}

impl ClipboardHandler for Handler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        self.handle();
        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, _error: io::Error) -> CallbackResult {
        CallbackResult::Next
    }
}

impl Handler {
    fn handle(&mut self) {
        let shared = self.app.state::<Shared>().inner().clone();
        let now = now_ms();
        if now < shared.ignore_clipboard_until.load(Ordering::Relaxed) {
            return;
        }
        if win::clipboard_marked_private() {
            return;
        }
        // The source app may still be rendering formats; give it a moment.
        std::thread::sleep(Duration::from_millis(40));
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return };

        if let Ok(text) = clipboard.get_text() {
            if !text.trim().is_empty() {
                self.on_text(&shared, &text, now);
                return;
            }
        }
        if !lock(&shared.settings).capture_images {
            return;
        }
        if let Ok(img) = clipboard.get_image() {
            let (w, h) = (img.width as u32, img.height as u32);
            let added = lock(&shared.shelf).add_rgba(w, h, img.bytes.into_owned(), now);
            match added {
                Ok(Some(_)) => {
                    emit_shelf(&self.app);
                    let _ = self.app.emit("captured", "shelf");
                }
                Ok(None) => {}
                Err(e) => eprintln!("shelf: {e}"),
            }
        }
    }

    fn on_text(&mut self, shared: &Shared, text: &str, now: u64) {
        let changed = lock(&shared.texts).push(text, now);
        if changed {
            shared.persist_texts();
            emit_texts(&self.app);
            let _ = self.app.emit("captured", "clipboard");
        }

        let normalized = normalize(text);
        let double = matches!(&self.last_text, Some((prev, at))
            if *prev == normalized && (DOUBLE_COPY_MIN_MS..=DOUBLE_COPY_MAX_MS).contains(&now.saturating_sub(*at)));
        if double && lock(&shared.settings).double_copy_translate {
            self.last_text = None;
            request_translation(&self.app, normalized);
        } else {
            self.last_text = Some((normalized, now));
        }
    }
}

/// Opens the island on the translator tab and hands it the text.
pub fn request_translation(app: &AppHandle, text: String) {
    open_island(app, Some("translate"), 8_000);
    let _ = app.emit("translate-request", text);
}

/// Current clipboard text, if any.
pub fn read_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok().map(|t| normalize(&t)).filter(|t| !t.is_empty())
}

pub fn write_text(shared: &Shared, text: &str) -> Result<(), String> {
    shared.mute_clipboard(700);
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())).map_err(|e| e.to_string())
}

pub fn write_image(shared: &Shared, width: u32, height: u32, rgba: Vec<u8>) -> Result<(), String> {
    lock(&shared.shelf).remember(notch_core::shelf::fingerprint(width, height, &rgba));
    shared.mute_clipboard(1500);
    let data = arboard::ImageData { width: width as usize, height: height as usize, bytes: rgba.into() };
    arboard::Clipboard::new().and_then(|mut c| c.set_image(data)).map_err(|e| e.to_string())
}

/// Starts the listener thread; restarts it if the hidden message window dies.
pub fn spawn_listener(app: AppHandle) {
    std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || loop {
            let handler = Handler { app: app.clone(), last_text: None };
            match Master::new(handler) {
                Ok(mut master) => {
                    if let Err(e) = master.run() {
                        eprintln!("clipboard listener stopped: {e}");
                    }
                }
                Err(e) => eprintln!("clipboard listener failed: {e}"),
            }
            std::thread::sleep(Duration::from_secs(2));
        })
        .expect("clipboard thread");
}
