//! Shared application state and the helpers every module uses to change it.

use notch_core::hover::{HoverMachine, Mode, Timing, Transition};
use notch_core::settings::{Settings, HISTORY_FILE};
use notch_core::shelf::Shelf;
use notch_core::text_history::{TextEntry, TextHistory};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use crate::win;

/// Logical size of the transparent overlay window (the stage).
pub const STAGE_W: f64 = 540.0;
pub const STAGE_H: f64 = 280.0;
/// Logical size of the collapsed capsule (also its hitbox).
pub const COMPACT_W: f64 = 180.0;
pub const COMPACT_H: f64 = 32.0;
/// Logical hitbox of the thin strip (the visible strip is a bit narrower).
pub const STRIP_W: f64 = 180.0;
pub const STRIP_H: f64 = 6.0;
/// Default logical size of the open island until the UI reports its own.
pub const EXPANDED_W: f64 = 520.0;
pub const EXPANDED_H: f64 = 260.0;

pub type Shared = Arc<App>;

pub struct App {
    pub data_dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub texts: Mutex<TextHistory>,
    pub shelf: Mutex<Shelf>,
    pub hover: Mutex<HoverMachine>,
    pub expanded_size: Mutex<(f64, f64)>,
    /// Raw HWND of the overlay (0 until the window exists / off Windows).
    pub hwnd: AtomicIsize,
    /// Clipboard updates before this moment (ms) were made by us; skip them.
    pub ignore_clipboard_until: AtomicU64,
    /// Window that had the foreground before the translator took focus.
    pub prev_foreground: AtomicIsize,
    pub input_mode: AtomicBool,
    pub hidden_for_fullscreen: AtomicBool,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Locks a mutex, recovering the data if a panicking thread poisoned it.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Serialize, Clone)]
pub struct IslandEvent {
    pub mode: Mode,
    /// Tab to show when opening (`shelf`, `clipboard`, `translate`).
    pub tab: Option<&'static str>,
    /// Put the caret into the translator field (hotkey).
    pub focus: bool,
}

impl App {
    pub fn new(data_dir: PathBuf, settings: Settings, texts: TextHistory, shelf: Shelf) -> Self {
        Self {
            data_dir,
            settings: Mutex::new(settings),
            texts: Mutex::new(texts),
            shelf: Mutex::new(shelf),
            hover: Mutex::new(HoverMachine::new(Timing::default())),
            expanded_size: Mutex::new((EXPANDED_W, EXPANDED_H)),
            hwnd: AtomicIsize::new(0),
            ignore_clipboard_until: AtomicU64::new(0),
            prev_foreground: AtomicIsize::new(0),
            input_mode: AtomicBool::new(false),
            hidden_for_fullscreen: AtomicBool::new(false),
        }
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd.load(Ordering::Relaxed)
    }

    pub fn history_path(&self) -> PathBuf {
        self.data_dir.join(HISTORY_FILE)
    }

    /// Writes the text history to disk if the user asked for it, otherwise
    /// makes sure no history file is left behind.
    pub fn persist_texts(&self) {
        let path = self.history_path();
        if lock(&self.settings).persist_text_history {
            let entries: Vec<TextEntry> = lock(&self.texts).entries().to_vec();
            if let Ok(json) = serde_json::to_vec(&entries) {
                let _ = notch_core::settings::write_atomic(&path, &json);
            }
        } else if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Marks the next clipboard change as our own so it is not recorded again.
    pub fn mute_clipboard(&self, ms: u64) {
        self.ignore_clipboard_until.store(now_ms() + ms, Ordering::Relaxed);
    }
}

pub fn emit_texts(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let entries = lock(&shared.texts).entries().to_vec();
    let _ = app.emit("texts", entries);
}

pub fn emit_shelf(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let items = lock(&shared.shelf).items().to_vec();
    let _ = app.emit("shelf", items);
}

/// Tells the UI about a mode change decided by the hover machine.
pub fn announce(app: &AppHandle, transition: Transition) {
    let shared = app.state::<Shared>();
    if transition == Transition::Collapse && shared.input_mode.load(Ordering::Relaxed) {
        end_input(app, true);
    }
    let mode = lock(&shared.hover).mode();
    let _ = app.emit("island", IslandEvent { mode, tab: None, focus: false });
}

/// Opens the island from code (hotkey, tray, double copy). With `focus` the
/// translator field gets the keyboard right away.
pub fn open_island(app: &AppHandle, tab: Option<&'static str>, sticky_ms: u64, focus: bool) {
    let shared = app.state::<Shared>();
    if shared.hidden_for_fullscreen.swap(false, Ordering::Relaxed) {
        win::show_no_activate(shared.hwnd(), true);
    }
    // Announce even if it was already open: the UI may need to switch tabs.
    let _ = lock(&shared.hover).open(now_ms(), sticky_ms);
    if focus {
        begin_input(app);
    }
    let _ = app.emit("island", IslandEvent { mode: Mode::Open, tab, focus });
}

pub fn close_island(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let t = lock(&shared.hover).close(now_ms());
    if let Some(t) = t {
        announce(app, t);
    } else if shared.input_mode.load(Ordering::Relaxed) {
        end_input(app, true);
    }
}

/// New clip or screenshot: wake the strip up so the counter flash is visible.
pub fn poke(app: &AppHandle) {
    let shared = app.state::<Shared>();
    lock(&shared.hover).poke(now_ms());
}

/// Lets the translator's text field take keyboard focus: the overlay becomes
/// activatable for a moment and is pushed to the foreground.
pub fn begin_input(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let Some(window) = app.get_webview_window("main") else { return };
    if !shared.input_mode.swap(true, Ordering::Relaxed) {
        let fg = win::foreground_window();
        if fg != shared.hwnd() {
            shared.prev_foreground.store(fg, Ordering::Relaxed);
        }
    }
    lock(&shared.hover).set_input_lock(true);
    let _ = window.set_focusable(true);
    win::force_foreground(shared.hwnd());
    let _ = window.set_focus();
}

/// Returns to the no-activate mode. With `restore` the window that was active
/// before gets the focus back.
pub fn end_input(app: &AppHandle, restore: bool) {
    let shared = app.state::<Shared>();
    if !shared.input_mode.swap(false, Ordering::Relaxed) {
        return;
    }
    lock(&shared.hover).set_input_lock(false);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focusable(false);
    }
    let prev = shared.prev_foreground.swap(0, Ordering::Relaxed);
    if restore && prev != 0 {
        win::force_foreground(prev);
    }
    let _ = app.emit("input-mode", false);
}
