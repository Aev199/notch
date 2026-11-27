//! Notch's own screenshot tool. PrintScreen freezes the monitor under the
//! cursor and shows it in a borderless window covering that monitor; there
//! the user picks an area, draws on it, and sends it to the shelf and the
//! clipboard — or reads the text in it (OCR) and translates it.
//!
//! The window is built on the first use and only hidden afterwards, so the
//! next captures open at once. It never appears through a plain show: it is
//! shown *cloaked* (see `win::cloak`), the page draws the new picture, and
//! only then the cloak comes off. So there is no show/hide animation and no
//! frame of the previous capture flashing by.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::state::{lock, Shared};
use crate::{clip, state, win};

pub const LABEL: &str = "capture";

/// A capture is being prepared or shown: further presses do nothing.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The capture is uncloaked: the user is working in it.
static ON_SCREEN: AtomicBool = AtomicBool::new(false);
static STARTED_AT: AtomicU64 = AtomicU64::new(0);
/// A capture that never reached the screen (the page failed) stops blocking
/// new ones after this long.
const STUCK_MS: u64 = 4_000;

/// PrintScreen was pressed. Runs on its own thread: building a window on the
/// main thread from a hotkey handler deadlocks on Windows (tauri#4121).
pub fn start(app: &AppHandle) {
    let now = state::now_ms();
    if ACTIVE.swap(true, Ordering::SeqCst) {
        // On screen, or still being prepared: one capture at a time. A
        // preparation that never finished stops blocking after a while.
        let preparing = now.saturating_sub(STARTED_AT.load(Ordering::SeqCst)) < STUCK_MS;
        if ON_SCREEN.load(Ordering::SeqCst) || preparing {
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
    // Whatever was in front when PrintScreen was pressed: the drawing or
    // document the screenshot is about.
    let (app_name, window) = win::window_source(win::foreground_window());
    *lock(&shared.capture_source) = notch_core::papka::Source { app: app_name, window };
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
        .focused(false)
        // Cloaking does not guarantee that WebView2 has submitted its first
        // frame. Keep both the native window and the webview transparent so
        // an empty frame cannot cover the monitor with a solid colour.
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    // The page prepares the window through capture_image. Doing it here
    // races the page on first use: a late prepare could cloak it again.
    Ok(())
}

pub fn hwnd_of(window: &WebviewWindow) -> isize {
    #[cfg(windows)]
    {
        window.hwnd().map(|h| h.0 as isize).unwrap_or(0)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        0
    }
}

/// Puts the window over the captured monitor, shown but cloaked: the page
/// renders there unseen and uncloaks it with `capture_show`.
fn prepare(window: &WebviewWindow, (x, y, width, height): (i32, i32, u32, u32)) -> Result<(), String> {
    let hwnd = hwnd_of(window);
    win::no_transitions(hwnd);
    win::cloak(hwnd, true);
    // Position first: the size is then applied at that monitor's scale.
    window.set_position(PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
    window.set_size(PhysicalSize::new(width, height)).map_err(|e| e.to_string())?;
    window.set_position(PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
    // Tauri's show also activates the window. Until the picture is ready,
    // leave the foreground app alone (including its active-window colours).
    #[cfg(windows)]
    win::show_no_activate(hwnd, true);
    #[cfg(not(windows))]
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

/// Off the screen at once (cloak: no hide animation), then really hidden so
/// it takes no input and gives the focus back.
fn dismiss(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        win::cloak(hwnd_of(&window), true);
        let _ = window.hide();
    }
    finish(app);
}

fn finish(app: &AppHandle) {
    ON_SCREEN.store(false, Ordering::SeqCst);
    *lock(&app.state::<Shared>().capture) = None;
    ACTIVE.store(false, Ordering::SeqCst);
}

/// The frozen screen for the page: 8 bytes of width and height (u32 LE),
/// then RGBA pixels.
#[tauri::command]
pub async fn capture_image(app: AppHandle, state: tauri::State<'_, Shared>) -> Result<tauri::ipc::Response, String> {
    let rect = {
        let guard = lock(&state.capture);
        let screen = guard.as_ref().ok_or("нет снимка экрана")?;
        (screen.x, screen.y, screen.width, screen.height)
    };
    let window = app.get_webview_window(LABEL).ok_or("окно снимка закрыто")?;
    prepare(&window, rect)?;
    let guard = lock(&state.capture);
    let screen = guard.as_ref().ok_or("нет снимка экрана")?;
    let mut out = Vec::with_capacity(8 + screen.rgba.len());
    out.extend_from_slice(&screen.width.to_le_bytes());
    out.extend_from_slice(&screen.height.to_le_bytes());
    out.extend_from_slice(&screen.rgba);
    Ok(tauri::ipc::Response::new(out))
}

/// The page has drawn and presented the new picture: take the cloak off.
#[tauri::command]
pub async fn capture_show(app: AppHandle, state: tauri::State<'_, Shared>) -> Result<(), String> {
    if lock(&state.capture).is_none() {
        return Err("нет снимка экрана".into());
    }
    let window = app.get_webview_window(LABEL).ok_or("окно снимка закрыто")?;
    let _ = window.set_always_on_top(true);
    win::flush_compositor();
    win::cloak(hwnd_of(&window), false);
    ON_SCREEN.store(true, Ordering::SeqCst);
    let _ = window.set_focus();
    Ok(())
}

/// Done or cancelled: hide the window and forget the picture.
#[tauri::command]
pub async fn capture_close(app: AppHandle) -> Result<(), String> {
    dismiss(&app);
    Ok(())
}

/// Text read from the selected area: it goes to the clipboard and its
/// history; with `translate` the translator opens with it instead.
#[tauri::command]
pub async fn capture_text(text: String, translate: bool, app: AppHandle, state: tauri::State<'_, Shared>) -> Result<(), String> {
    dismiss(&app);
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
