//! The overlay loop: polls the cursor (Win32 `GetCursorPos`, every 35 ms),
//! drives the hover state machine, toggles click-through and hides the island
//! while a fullscreen app runs.

use notch_core::hover::{Rect, Sample};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition};

use crate::state::{announce, lock, now_ms, Shared, COMPACT_H, COMPACT_W, STAGE_H, STAGE_W};
use crate::win;

const TICK: Duration = Duration::from_millis(35);
/// Fullscreen check and window housekeeping run every N ticks (~1 s).
const SLOW_EVERY: u32 = 28;

/// Puts the stage at the top centre of the primary monitor.
pub fn place_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let Ok(Some(monitor)) = window.primary_monitor() else { return };
    let scale = monitor.scale_factor();
    let (w, h) = ((STAGE_W * scale).round() as i32, (STAGE_H * scale).round() as i32);
    let pos = monitor.position();
    let size = monitor.size();
    let x = pos.x + (size.width as i32 - w) / 2;
    let target = PhysicalPosition::new(x, pos.y);
    let current_size = window.outer_size().ok();
    if current_size.map(|s| (s.width as i32, s.height as i32)) != Some((w, h)) {
        let _ = window.set_size(tauri::PhysicalSize::new(w as u32, h as u32));
    }
    if window.outer_position().ok() != Some(target) {
        let _ = window.set_position(target);
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("overlay".into())
        .spawn(move || run(app))
        .expect("overlay thread");
}

fn run(app: AppHandle) {
    let shared = app.state::<Shared>().inner().clone();
    let Some(window) = app.get_webview_window("main") else { return };
    let mut interactive = false;
    let _ = window.set_ignore_cursor_events(true);
    win::ensure_tool_window(shared.hwnd());
    let mut tick: u32 = 0;

    loop {
        std::thread::sleep(TICK);
        tick = tick.wrapping_add(1);
        let hwnd = shared.hwnd();

        if tick % SLOW_EVERY == 0 {
            housekeeping(&app, &shared, hwnd);
        }
        if shared.hidden_for_fullscreen.load(Ordering::Relaxed) {
            continue;
        }

        let (Some(cursor), Some((x, y, w, h, scale))) = (win::cursor_pos(), win::window_rect(hwnd)) else {
            continue;
        };
        let stage = Rect { x, y, w, h };
        let (ew, eh) = *lock(&shared.expanded_size);
        let sample = Sample {
            now_ms: now_ms(),
            cursor,
            compact: Rect::top_center(&stage, COMPACT_W * scale, COMPACT_H * scale),
            expanded: Rect::top_center(&stage, ew * scale, eh * scale),
            leave_margin: 14.0 * scale,
            left_button_down: win::left_button_down(),
        };
        let (transition, now_interactive) = {
            let mut machine = lock(&shared.hover);
            let t = machine.tick(&sample);
            (t, machine.interactive())
        };
        if now_interactive != interactive {
            interactive = now_interactive;
            let _ = window.set_ignore_cursor_events(!interactive);
            win::ensure_tool_window(hwnd);
        }
        if let Some(t) = transition {
            announce(&app, t, None);
        }
    }
}

fn housekeeping(app: &AppHandle, shared: &Shared, hwnd: isize) {
    win::ensure_tool_window(hwnd);
    let hide_enabled = lock(&shared.settings).hide_in_fullscreen;
    let expanded = lock(&shared.hover).is_expanded();
    let fullscreen = hide_enabled && !expanded && win::fullscreen_app_active();
    let hidden = shared.hidden_for_fullscreen.load(Ordering::Relaxed);
    if fullscreen && !hidden {
        shared.hidden_for_fullscreen.store(true, Ordering::Relaxed);
        win::show_no_activate(hwnd, false);
    } else if !fullscreen && hidden {
        shared.hidden_for_fullscreen.store(false, Ordering::Relaxed);
        win::show_no_activate(hwnd, true);
    } else if !hidden && !win::is_visible(hwnd) {
        win::show_no_activate(hwnd, true);
    }
    if !shared.hidden_for_fullscreen.load(Ordering::Relaxed) {
        // Follows resolution / DPI / primary-monitor changes.
        place_window(app);
    }
}
