//! Notch: a portable Dynamic Island-style overlay for Windows 10/11 with a
//! screenshot shelf, clipboard history and a quick RU/EN translator.

mod clip;
mod net;
mod overlay;
mod state;
mod win;

use notch_core::settings::{self, Settings, SETTINGS_FILE};
use notch_core::shelf::{ShelfItem, Shelf};
use notch_core::text_history::{TextEntry, TextHistory};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use state::{lock, App, Shared, COMPACT_H, COMPACT_W};

const AUTOSTART_NAME: &str = "Notch";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    settings: Settings,
    texts: Vec<TextEntry>,
    shelf: Vec<ShelfItem>,
    compact: (f64, f64),
    data_dir: String,
    autostart: bool,
    expanded: bool,
}

fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("notch.exe"))
}

fn autostart_command() -> String {
    format!("\"{}\"", exe_path().display())
}

fn autostart_enabled() -> bool {
    win::autostart_get(AUTOSTART_NAME).is_some_and(|v| v.eq_ignore_ascii_case(&autostart_command()))
}

#[tauri::command]
fn snapshot(state: State<'_, Shared>) -> Snapshot {
    Snapshot {
        settings: lock(&state.settings).clone(),
        texts: lock(&state.texts).entries().to_vec(),
        shelf: lock(&state.shelf).items().to_vec(),
        compact: (COMPACT_W, COMPACT_H),
        data_dir: state.data_dir.display().to_string(),
        autostart: autostart_enabled(),
        expanded: lock(&state.hover).is_expanded(),
    }
}

/// The UI reports the logical size of the open island so the hitbox matches it.
#[tauri::command]
fn set_island_size(width: f64, height: f64, state: State<'_, Shared>) {
    let clamp = |v: f64, max: f64| if v.is_finite() { v.clamp(40.0, max) } else { max };
    *lock(&state.expanded_size) = (clamp(width, state::STAGE_W), clamp(height, state::STAGE_H));
}

#[tauri::command]
fn close_island(app: AppHandle) {
    state::close_island(&app);
}

#[tauri::command]
fn open_island(app: AppHandle, tab: Option<String>) {
    let tab = match tab.as_deref() {
        Some("shelf") => Some("shelf"),
        Some("clipboard") => Some("clipboard"),
        Some("translate") => Some("translate"),
        _ => None,
    };
    state::open_island(&app, tab, 4_000);
}

/// Puts a history entry back on the clipboard (and moves it to the top).
#[tauri::command]
fn copy_entry(id: u64, close: bool, app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    let text = lock(&state.texts).get(id).map(|e| e.text.clone()).ok_or("запись не найдена")?;
    clip::write_text(&state, &text)?;
    lock(&state.texts).push(&text, state::now_ms());
    state.persist_texts();
    state::emit_texts(&app);
    if close {
        state::close_island(&app);
    }
    Ok(())
}

/// Copies arbitrary text (e.g. a translation) and records it in the history.
#[tauri::command]
fn copy_text(text: String, app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    clip::write_text(&state, &text)?;
    if lock(&state.texts).push(&text, state::now_ms()) {
        state.persist_texts();
        state::emit_texts(&app);
    }
    Ok(())
}

#[tauri::command]
fn remove_entry(id: u64, app: AppHandle, state: State<'_, Shared>) {
    lock(&state.texts).remove(id);
    state.persist_texts();
    state::emit_texts(&app);
}

#[tauri::command]
fn clear_entries(app: AppHandle, state: State<'_, Shared>) {
    lock(&state.texts).clear();
    state.persist_texts();
    state::emit_texts(&app);
}

#[tauri::command]
fn shelf_copy(id: String, state: State<'_, Shared>) -> Result<(), String> {
    let (w, h, rgba) = lock(&state.shelf).load_rgba(&id)?;
    clip::write_image(&state, w, h, rgba)
}

#[tauri::command]
fn shelf_remove(id: String, app: AppHandle, state: State<'_, Shared>) {
    lock(&state.shelf).remove(&id);
    state::emit_shelf(&app);
}

#[tauri::command]
fn shelf_clear(app: AppHandle, state: State<'_, Shared>) {
    lock(&state.shelf).clear();
    state::emit_shelf(&app);
}

#[tauri::command]
fn open_data_folder(state: State<'_, Shared>) -> Result<(), String> {
    open_in_explorer(&state.data_dir)
}

fn open_in_explorer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(not(windows))]
    let program = "xdg-open";
    std::process::Command::new(program).arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// A screenshot drag-out started (true) or finished (false).
#[tauri::command]
fn set_dragging(dragging: bool, state: State<'_, Shared>) {
    lock(&state.hover).set_dragging(dragging);
}

#[tauri::command]
fn begin_input(app: AppHandle) {
    state::begin_input(&app);
}

#[tauri::command]
fn end_input(app: AppHandle) {
    state::end_input(&app, true);
}

#[tauri::command]
async fn translate(text: String, state: State<'_, Shared>) -> Result<net::Translation, String> {
    let settings = lock(&state.settings).clone();
    net::translate(&settings, &text).await
}

/// Translates whatever text is on the clipboard now.
#[tauri::command]
fn clipboard_text() -> Option<String> {
    clip::read_text()
}

fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if hotkey.is_empty() {
        return Ok(());
    }
    let shortcut: Shortcut = hotkey.parse().map_err(|e| format!("не понимаю сочетание «{hotkey}»: {e}"))?;
    gs.register(shortcut).map_err(|e| format!("сочетание «{hotkey}» занято другой программой: {e}"))
}

#[tauri::command]
fn save_settings(new: Settings, app: AppHandle, state: State<'_, Shared>) -> Result<Settings, String> {
    let new = new.sanitized();
    let old = lock(&state.settings).clone();
    if new.hotkey != old.hotkey {
        if let Err(e) = register_hotkey(&app, &new.hotkey) {
            let _ = register_hotkey(&app, &old.hotkey);
            return Err(e);
        }
    }
    new.save(&state.data_dir.join(SETTINGS_FILE))?;
    lock(&state.texts).set_limit(new.text_limit);
    lock(&state.shelf).set_limit(new.shelf_limit);
    *lock(&state.settings) = new.clone();
    state.persist_texts();
    state::emit_texts(&app);
    state::emit_shelf(&app);
    Ok(new)
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<bool, String> {
    let command = autostart_command();
    win::autostart_set(AUTOSTART_NAME, enabled.then_some(command.as_str()))?;
    Ok(autostart_enabled())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Открыть челку", true, None::<&str>)?;
    let translate = MenuItem::with_id(app, "translate", "Перевести буфер", true, None::<&str>)?;
    let folder = MenuItem::with_id(app, "folder", "Папка программы", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &translate, &folder, &sep, &quit])?;
    let mut builder = TrayIconBuilder::with_id("notch")
        .tooltip("Notch")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => state::open_island(app, None, 4_000),
            "translate" => clip::request_translation(app, clip::read_text().unwrap_or_default()),
            "folder" => {
                let dir = app.state::<Shared>().data_dir.clone();
                let _ = open_in_explorer(&dir);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                state::open_island(tray.app_handle(), None, 4_000);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn load_state() -> App {
    let fallback = std::env::temp_dir().join("Notch");
    let data_dir = settings::data_dir(&exe_path(), &fallback);
    let settings = Settings::load(&data_dir.join(SETTINGS_FILE));
    let texts = if settings.persist_text_history {
        std::fs::read(data_dir.join(settings::HISTORY_FILE))
            .ok()
            .and_then(|raw| serde_json::from_slice::<Vec<TextEntry>>(&raw).ok())
            .map(|entries| TextHistory::restore(entries, settings.text_limit))
            .unwrap_or_else(|| TextHistory::new(settings.text_limit))
    } else {
        TextHistory::new(settings.text_limit)
    };
    let shelf_dir = settings::shelf_dir(&data_dir);
    let shelf = Shelf::open(&shelf_dir, settings.shelf_limit)
        .or_else(|_| Shelf::open(fallback.join("cache").join("shelf"), settings.shelf_limit))
        .expect("screenshot cache folder");
    App::new(data_dir, settings, texts, shelf)
}

pub fn run() {
    if !win::claim_single_instance("Local\\NotchIsland.SingleInstance") {
        return;
    }
    let shared: Shared = Arc::new(load_state());

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        clip::request_translation(app, clip::read_text().unwrap_or_default());
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_drag::init())
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            snapshot,
            set_island_size,
            close_island,
            open_island,
            copy_entry,
            copy_text,
            remove_entry,
            clear_entries,
            shelf_copy,
            shelf_remove,
            shelf_clear,
            open_data_folder,
            set_dragging,
            begin_input,
            end_input,
            translate,
            clipboard_text,
            save_settings,
            set_autostart,
            quit,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let window = app.get_webview_window("main").expect("main window");
            #[cfg(windows)]
            if let Ok(hwnd) = window.hwnd() {
                shared.hwnd.store(hwnd.0 as isize, Ordering::Relaxed);
            }
            overlay::place_window(&handle);
            win::ensure_tool_window(shared.hwnd());

            let hotkey = lock(&shared.settings).hotkey.clone();
            if let Err(e) = register_hotkey(&handle, &hotkey) {
                eprintln!("hotkey: {e}");
            }
            if let Err(e) = build_tray(&handle) {
                eprintln!("tray: {e}");
            }

            let blur_handle = handle.clone();
            window.on_window_event(move |event| match event {
                // Clicked somewhere else while typing: give up keyboard focus
                // (do not steal it back from where the user clicked).
                WindowEvent::Focused(false) => state::end_input(&blur_handle, false),
                WindowEvent::ScaleFactorChanged { .. } => overlay::place_window(&blur_handle),
                _ => {}
            });

            clip::spawn_listener(handle.clone());
            overlay::spawn(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Notch");
}
