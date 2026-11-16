//! Notch's own screenshot tool. PrintScreen freezes the monitor under the
//! cursor and shows it in a borderless window covering that monitor; there
//! the user picks an area, draws on it, and sends it to the shelf and the
//! clipboard — or reads the text in it (OCR) and translates it.
//!
//! The window is built on the first use and only hidden afterwards, so the
//! next captures open at once.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::state::{lock, Shared};
use crate::{clip, state, win};

pub const LABEL: &str = "capture";

/// A capture is being prepared or shown: further presses do nothing.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static STARTED_AT: AtomicU64 = AtomicU64::new(0);
/// A capture that never reached the screen (the page failed) stops blocking
/// new ones after this long.
const STUCK_MS: u64 = 4_000;

/// PrintScreen was pressed. Runs on its own thread: building a window on the
/// main thread from a hotkey handler deadlocks on Windows (tauri#4121).
pub fn start(app: &AppHandle) {
    let now = state::now_ms();
    if ACTIVE.swap(true, Ordering::SeqCst) {
        let shown = app.get_webview_window(LABEL).is_some_and(|w| w.is_visible().unwrap_or(false));
        if shown || now.saturating_sub(STARTED_AT.load(Ordering::SeqCst)) < STUCK_MS {
            return;
        }
    }
    STARTED_AT.store(now, Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = open(&app) {
            eprintln!("capture: {e}");
            finish(&app);
        }
    });
}

fn open(app: &AppHandle) -> Result<(), String> {
    let shared = app.state::<Shared>().inner().clone();
    let screen = win::capture_cursor_monitor().ok_or("не удалось снять экран")?;
    *lock(&shared.capture) = Some(screen);
    if let Some(window) = app.get_webview_window(LABEL) {
        // The page is alive: it loads the new picture and asks to be shown.
        return window.emit("capture-open", ()).map_err(|e| e.to_string());
    }
    let url = WebviewUrl::App("index.html?capture=1".into());
    WebviewWindowBuilder::new(app, LABEL, url)
        .title("Notch — снимок экрана")
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn finish(app: &AppHandle) {
    *lock(&app.state::<Shared>().capture) = None;
    ACTIVE.store(false, Ordering::SeqCst);
}

/// The frozen screen for the page: 8 bytes of width and height (u32 LE),
/// then RGBA pixels.
#[tauri::command]
pub async fn capture_image(state: tauri::State<'_, Shared>) -> Result<tauri::ipc::Response, String> {
    let guard = lock(&state.capture);
    let screen = guard.as_ref().ok_or("нет снимка экрана")?;
    let mut out = Vec::with_capacity(8 + screen.rgba.len());
    out.extend_from_slice(&screen.width.to_le_bytes());
    out.extend_from_slice(&screen.height.to_le_bytes());
    out.extend_from_slice(&screen.rgba);
    Ok(tauri::ipc::Response::new(out))
}

/// The page has drawn the picture: put the window over the captured monitor.
#[tauri::command]
pub async fn capture_show(app: AppHandle, state: tauri::State<'_, Shared>) -> Result<(), String> {
    let (x, y, width, height) = {
        let guard = lock(&state.capture);
        let screen = guard.as_ref().ok_or("нет снимка экрана")?;
        (screen.x, screen.y, screen.width, screen.height)
    };
    let window = app.get_webview_window(LABEL).ok_or("окно снимка закрыто")?;
    // Position first: the size is then applied at that monitor's scale.
    let _ = window.set_position(PhysicalPosition::new(x, y));
    let _ = window.set_size(PhysicalSize::new(width, height));
    let _ = window.set_position(PhysicalPosition::new(x, y));
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    Ok(())
}

/// Done or cancelled: hide the window and forget the picture.
#[tauri::command]
pub async fn capture_close(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    finish(&app);
    Ok(())
}

/// Text read from the selected area: it goes to the clipboard and its
/// history; with `translate` the translator opens with it instead.
#[tauri::command]
pub async fn capture_text(text: String, translate: bool, app: AppHandle, state: tauri::State<'_, Shared>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    finish(&app);
    clip::write_text(&state, &text)?;
    lock(&state.texts).push(&text, state::now_ms());
    state.persist_texts();
    state::emit_texts(&app);
    if translate {
        clip::request_translation(&app, notch_core::text_history::normalize(&text), false);
    } else {
        state::open_island(&app, Some("clipboard"), 4_000, false);
    }
    Ok(())
}
