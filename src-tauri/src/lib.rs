//! Notch: a portable Dynamic Island-style overlay for Windows 10/11 with a
//! screenshot shelf, clipboard history and a quick RU/EN translator.

mod clip;
mod models;
mod net;
mod ocr;
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

use state::{lock, App, Shared};

const AUTOSTART_NAME: &str = "Notch";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    settings: Settings,
    texts: Vec<TextEntry>,
    shelf: Vec<ShelfItem>,
    data_dir: String,
    autostart: bool,
    mode: notch_core::hover::Mode,
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
        data_dir: state.data_dir.display().to_string(),
        autostart: autostart_enabled(),
        mode: lock(&state.hover).mode(),
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
    state::open_island(&app, tab, 4_000, false);
}

/// One part of an embedded offline model, gzip-compressed, as raw bytes.
#[tauri::command]
fn offline_model(pair: String, part: String) -> Result<tauri::ipc::Response, String> {
    models::get(&pair, &part)
        .map(|bytes| tauri::ipc::Response::new(bytes.to_vec()))
        .ok_or_else(|| format!("нет модели {pair}/{part}"))
}

/// Puts text on the clipboard and moves it to the top of the history.
fn put_text(text: &str, app: &AppHandle, state: &Shared) -> Result<(), String> {
    clip::write_text(state, text)?;
    lock(&state.texts).push(text, state::now_ms());
    state.persist_texts();
    state::emit_texts(app);
    Ok(())
}

fn entry_text(id: u64, state: &Shared) -> Result<String, String> {
    lock(&state.texts).get(id).map(|e| e.text.clone()).ok_or_else(|| "запись не найдена".into())
}

/// Puts a history entry back on the clipboard; `close` folds the island.
#[tauri::command]
fn copy_entry(id: u64, close: bool, app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    put_text(&entry_text(id, &state)?, &app, &state)?;
    if close {
        state::close_island(&app);
    }
    Ok(())
}

/// Pastes a history entry straight into the field the user was typing in.
#[tauri::command]
fn paste_entry(id: u64, app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    put_text(&entry_text(id, &state)?, &app, &state)?;
    state::paste_into_active(&app);
    Ok(())
}

/// Pastes arbitrary text (a translation) into the active field.
#[tauri::command]
fn paste_text(text: String, app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    put_text(&text, &app, &state)?;
    state::paste_into_active(&app);
    Ok(())
}

/// Copies arbitrary text (a translation) and records it in the history.
#[tauri::command]
fn copy_text(text: String, app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    put_text(&text, &app, &state)
}

#[tauri::command]
fn pin_entry(id: u64, pinned: bool, app: AppHandle, state: State<'_, Shared>) {
    lock(&state.texts).set_pinned(id, pinned);
    state.persist_texts();
    state::emit_texts(&app);
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

/// Full-size PNG of a screenshot for the preview (raw bytes).
#[tauri::command]
fn shelf_image(id: String, state: State<'_, Shared>) -> Result<tauri::ipc::Response, String> {
    let path = lock(&state.shelf).get(&id).map(|i| i.path.clone()).ok_or("скриншот не найден")?;
    std::fs::read(&path).map(tauri::ipc::Response::new).map_err(|e| e.to_string())
}

#[tauri::command]
async fn ocr_png(request: tauri::ipc::Request<'_>) -> Result<ocr::OcrText, String> {
    let tauri::ipc::InvokeBody::Raw(png) = request.body() else {
        return Err("нужна картинка PNG".into());
    };
    let (w, h, rgba) = notch_core::shelf::decode_png(png)?;
    run_ocr(w, h, rgba).await
}

/// WinRT calls block; run them on their own thread with their own apartment.
async fn run_ocr(w: u32, h: u32, rgba: Vec<u8>) -> Result<ocr::OcrText, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(ocr::recognize(w, h, rgba));
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|_| "распознавание прервано".to_string())?)
        .await
        .map_err(|e| e.to_string())?
}

/// Opens (or re-targets) the annotation editor: a normal, focusable window
/// sized to most of the primary screen.
#[tauri::command]
fn open_editor(id: String, app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    use tauri::{Emitter, WebviewUrl, WebviewWindowBuilder};
    if lock(&state.shelf).get(&id).is_none() {
        return Err("скриншот не найден".into());
    }
    state::close_island(&app);
    if let Some(editor) = app.get_webview_window("editor") {
        let _ = editor.emit("editor-open", &id);
        let _ = editor.unminimize();
        let _ = editor.set_focus();
        return Ok(());
    }
    let (w, h) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let s = m.scale_factor();
            (m.size().width as f64 / s * 0.8, m.size().height as f64 / s * 0.8)
        })
        .unwrap_or((1200.0, 800.0));
    let url = WebviewUrl::App(format!("index.html?editor={id}").into());
    WebviewWindowBuilder::new(&app, "editor", url)
        .title("Notch — редактор")
        .inner_size(w.max(720.0), h.max(480.0))
        .min_inner_size(640.0, 420.0)
        .center()
        .focused(true)
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The editor's result arrives as raw PNG bytes. Header `x-notch-action`:
/// `save` puts it on the shelf (next to the original, in its group) and on
/// the clipboard; `copy` only puts it on the clipboard.
#[tauri::command]
fn editor_result(request: tauri::ipc::Request<'_>, app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(png) = request.body() else {
        return Err("редактор прислал не картинку".into());
    };
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let encoded = notch_core::shelf::encode_png(png)?;
    let (w, h) = (encoded.width, encoded.height);
    if header("x-notch-action") == "save" {
        let source = header("x-notch-source");
        let mut shelf = lock(&state.shelf);
        let group = shelf.get(&source).and_then(|i| i.group.clone());
        let item = shelf.insert(encoded, state::now_ms())?;
        if let Some(group) = group {
            let _ = shelf.group(&[item.id.clone()], &group, state::now_ms());
        }
        drop(shelf);
        state::emit_shelf(&app);
    }
    let rgba = image_rgba(png)?;
    clip::write_image(&state, w, h, rgba)
}

fn image_rgba(png: &[u8]) -> Result<Vec<u8>, String> {
    notch_core::shelf::decode_png(png).map(|(_, _, rgba)| rgba)
}

#[tauri::command]
fn shelf_remove(id: String, app: AppHandle, state: State<'_, Shared>) {
    lock(&state.shelf).remove(&id);
    state::emit_shelf(&app);
}

/// Clears the loose screenshots; groups stay unless `everything`.
#[tauri::command]
fn shelf_clear(everything: bool, app: AppHandle, state: State<'_, Shared>) {
    lock(&state.shelf).clear(everything);
    state::emit_shelf(&app);
}

/// Drops screenshots or groups (`ids`) onto a screenshot or group (`target`).
#[tauri::command]
fn shelf_group(ids: Vec<String>, target: String, app: AppHandle, state: State<'_, Shared>) -> Result<String, String> {
    let gid = lock(&state.shelf).group(&ids, &target, state::now_ms())?;
    state::emit_shelf(&app);
    Ok(gid)
}

#[tauri::command]
fn shelf_ungroup(id: String, app: AppHandle, state: State<'_, Shared>) {
    lock(&state.shelf).ungroup(&id);
    state::emit_shelf(&app);
}

#[tauri::command]
fn shelf_remove_group(group: String, app: AppHandle, state: State<'_, Shared>) {
    lock(&state.shelf).remove_group(&group);
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
            "open" => state::open_island(app, None, 4_000, false),
            "translate" => clip::request_translation(app, clip::read_text().unwrap_or_default(), true),
            "folder" => {
                let dir = app.state::<Shared>().data_dir.clone();
                let _ = open_in_explorer(&dir);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                state::open_island(tray.app_handle(), None, 4_000, false);
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
    let read = |file: &str| -> Vec<TextEntry> {
        std::fs::read(data_dir.join(file)).ok().and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default()
    };
    let mut entries = read(settings::PINS_FILE);
    for entry in &mut entries {
        entry.pinned = true;
    }
    if settings.persist_text_history {
        entries.extend(read(settings::HISTORY_FILE));
    }
    let texts = TextHistory::restore(entries, settings.text_limit);
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
                        clip::request_translation(app, clip::read_text().unwrap_or_default(), true);
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
            offline_model,
            copy_entry,
            copy_text,
            remove_entry,
            clear_entries,
            shelf_copy,
            shelf_image,
            open_editor,
            ocr_png,
            editor_result,
            shelf_remove,
            pin_entry,
            shelf_clear,
            shelf_group,
            shelf_ungroup,
            shelf_remove_group,
            paste_entry,
            paste_text,
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
            win::apply_styles(shared.hwnd(), true);

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
