//! Clipboard watcher: an event-driven listener (`WM_CLIPBOARDUPDATE` via
//! clipboard-master, no polling) that routes text to the history and images
//! to the screenshot shelf.

use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use notch_core::clipboard::take_image;
use notch_core::shelf;
use notch_core::text_history::normalize;
use std::io;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{emit_shelf, emit_texts, lock, now_ms, open_island, poke, Shared};
use crate::win;

/// Two copies of the same text within this window (ms) mean "translate it".
/// The lower bound filters apps that write the clipboard twice per copy.
const DOUBLE_COPY_MIN_MS: u64 = 60;
const DOUBLE_COPY_MAX_MS: u64 = 450;

/// Office programs also fill the clipboard for Format Painter ("Формат по
/// образцу"), which is not a copy the user wants to keep.
const OFFICE: [&str; 4] = ["excel.exe", "winword.exe", "powerpnt.exe", "onenote.exe"];
/// A copy chord this recent counts as "the user pressed Ctrl+C".
const KEYS_RECENT_MS: u64 = 1_500;
/// A right click this recent counts as "copied from the context menu".
const MENU_RECENT_MS: u64 = 10_000;

struct Handler {
    app: AppHandle,
    /// Last text with its time; only copies made with the keyboard.
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
        let keyboard = win::copy_keys_down()
            || now.saturating_sub(shared.last_copy_keys.load(Ordering::Relaxed)) < KEYS_RECENT_MS;
        let context_menu = now.saturating_sub(shared.last_right_click.load(Ordering::Relaxed)) < MENU_RECENT_MS;
        if !keyboard && !context_menu {
            if let Some(owner) = win::clipboard_owner_exe() {
                if OFFICE.contains(&owner.as_str()) {
                    return;
                }
            }
        }
        // The source app may still be rendering formats; give it a moment.
        std::thread::sleep(Duration::from_millis(40));
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return };

        if let Ok(text) = clipboard.get_text() {
            if !text.trim().is_empty() {
                self.on_text(&shared, &text, now, keyboard);
                return;
            }
        }
        if !lock(&shared.settings).capture_images {
            return;
        }
        // Screenshots and copied pictures go to the shelf; previews of copied
        // objects (AutoCAD entities, Excel cells, Office shapes) do not —
        // see notch_core::clipboard. Unreadable clipboard: skip, not guess.
        let Some(formats) = win::clipboard_formats() else { return };
        let formats: Vec<(u32, &str)> = formats.iter().map(|(id, name)| (*id, name.as_str())).collect();
        if !take_image(win::clipboard_owner_exe().as_deref(), &formats) {
            return;
        }
        if let Ok(img) = clipboard.get_image() {
            let (w, h) = (img.width as u32, img.height as u32);
            let rgba = img.bytes.into_owned();
            if lock(&shared.shelf).is_duplicate(shelf::fingerprint(w, h, &rgba)) {
                return;
            }
            // PNG encoding takes a while on big screens: do it without the lock
            // so the UI stays responsive.
            let added = shelf::encode(w, h, rgba).and_then(|enc| lock(&shared.shelf).insert(enc, now));
            match added {
                Ok(_) => {
                    poke(&self.app);
                    emit_shelf(&self.app);
                    let _ = self.app.emit("captured", "shelf");
                }
                Err(e) => eprintln!("shelf: {e}"),
            }
        }
    }

    fn on_text(&mut self, shared: &Shared, text: &str, now: u64, keyboard: bool) {
        let changed = lock(&shared.texts).push(text, now);
        if changed {
            poke(&self.app);
            shared.persist_texts();
            emit_texts(&self.app);
            let _ = self.app.emit("captured", "clipboard");
        }

        // Double Ctrl+C: both copies must come from the keyboard, so double
        // clicks on toolbar buttons (Format Painter) never translate.
        if !keyboard {
            self.last_text = None;
            return;
        }
        let normalized = normalize(text);
        let double = matches!(&self.last_text, Some((prev, at))
            if *prev == normalized && (DOUBLE_COPY_MIN_MS..=DOUBLE_COPY_MAX_MS).contains(&now.saturating_sub(*at)));
        if double && lock(&shared.settings).double_copy_translate {
            self.last_text = None;
            request_translation(&self.app, normalized, false);
        } else {
            self.last_text = Some((normalized, now));
        }
    }
}

#[derive(serde::Serialize, Clone)]
struct TranslateRequest {
    text: String,
    focus: bool,
}

/// Opens the island on the translator tab and hands it the text. With
/// `focus` (hotkey) the caret goes into the source field; the double Ctrl+C
/// gesture leaves the keyboard where the user is working.
pub fn request_translation(app: &AppHandle, text: String, focus: bool) {
    let _ = app.emit("translate-request", TranslateRequest { text, focus });
    open_island(app, Some("translate"), 8_000, focus);
}

/// Puts the clipboard picture on the shelf whatever its source (the shelf's
/// "take from clipboard" button for what the automatic rule skipped).
pub fn image_to_shelf(app: &AppHandle) -> Result<(), String> {
    let shared = app.state::<Shared>().inner().clone();
    let img = arboard::Clipboard::new()
        .and_then(|mut c| c.get_image())
        .map_err(|_| "в буфере обмена нет картинки".to_string())?;
    let (w, h) = (img.width as u32, img.height as u32);
    let encoded = shelf::encode(w, h, img.bytes.into_owned())?;
    lock(&shared.shelf).insert(encoded, now_ms())?;
    emit_shelf(app);
    Ok(())
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
    lock(&shared.shelf).remember(shelf::fingerprint(width, height, &rgba));
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
