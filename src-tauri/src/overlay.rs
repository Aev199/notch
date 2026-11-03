//! The overlay loop: polls the cursor (Win32 `GetCursorPos`; every 16 ms near
//! the top of the screen, 50 ms elsewhere), drives the hover state machine,
//! owns the click-through style, shrinks the island to a strip over
//! fullscreen windows and hides it in exclusive fullscreen games.

use notch_core::hover::{Rect, Sample};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, PhysicalPosition};

use crate::state::{announce, lock, now_ms, Shared, COMPACT_H, COMPACT_W, STAGE_H, STAGE_W, STRIP_H, STRIP_W};
use crate::win;

/// Fullscreen check and window housekeeping run this often.
const SLOW_EVERY: Duration = Duration::from_millis(900);

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

/// Fast polling near the notch (open in ~1 frame), slow elsewhere.
const FAST: Duration = Duration::from_millis(16);
const SLOW: Duration = Duration::from_millis(50);
/// Height of the screen band (logical px) where the fast rate kicks in.
const NEAR_BAND: f64 = 120.0;

fn run(app: AppHandle) {
    let shared = app.state::<Shared>().inner().clone();
    let mut last_slow = Instant::now();
    let mut force_thin = false;
    let mut delay = FAST;

    loop {
        std::thread::sleep(delay);
        let hwnd = shared.hwnd();

        if last_slow.elapsed() >= SLOW_EVERY {
            last_slow = Instant::now();
            force_thin = housekeeping(&app, &shared, hwnd);
        }
        if shared.hidden_for_fullscreen.load(Ordering::Relaxed) {
            win::apply_styles(hwnd, true);
            delay = SLOW;
            continue;
        }

        let (Some(cursor), Some((x, y, w, h, scale))) = (win::cursor_pos(), win::window_rect(hwnd)) else {
            delay = SLOW;
            continue;
        };
        let stage = Rect { x, y, w, h };
        let (ew, eh) = *lock(&shared.expanded_size);
        let idle_thin_ms = {
            let s = lock(&shared.settings);
            s.idle_thin.then_some(s.idle_seconds * 1000)
        };
        let sample = Sample {
            now_ms: now_ms(),
            cursor,
            compact: Rect::top_center(&stage, COMPACT_W * scale, COMPACT_H * scale),
            strip: Rect::top_center(&stage, STRIP_W * scale, STRIP_H * scale),
            expanded: Rect::top_center(&stage, ew * scale, eh * scale),
            leave_margin: 14.0 * scale,
            left_button_down: win::left_button_down(),
            idle_thin_ms,
            force_thin,
        };
        let (transition, interactive, expanded) = {
            let mut machine = lock(&shared.hover);
            let t = machine.tick(&sample);
            (t, machine.interactive(), machine.is_expanded())
        };
        // Re-applied every tick: repairs any style rewrite done by Tauri.
        win::apply_styles(hwnd, !interactive);
        if let Some(t) = transition {
            announce(&app, t);
        }
        let near = expanded || (cursor.1 - y) < NEAR_BAND * scale;
        delay = if near { FAST } else { SLOW };
    }
}

/// Runs about once a second. Returns whether the island must stay a thin
/// strip because a fullscreen window is in front.
fn housekeeping(app: &AppHandle, shared: &Shared, hwnd: isize) -> bool {
    let hide_enabled = lock(&shared.settings).hide_in_fullscreen;
    let expanded = lock(&shared.hover).is_expanded();
    let exclusive = hide_enabled && !expanded && win::exclusive_fullscreen_active();
    let hidden = shared.hidden_for_fullscreen.load(Ordering::Relaxed);
    if exclusive && !hidden {
        shared.hidden_for_fullscreen.store(true, Ordering::Relaxed);
        win::show_no_activate(hwnd, false);
    } else if !exclusive && hidden {
        shared.hidden_for_fullscreen.store(false, Ordering::Relaxed);
        win::show_no_activate(hwnd, true);
    } else if !hidden && !win::is_visible(hwnd) {
        win::show_no_activate(hwnd, true);
    }
    if !shared.hidden_for_fullscreen.load(Ordering::Relaxed) {
        // Follows resolution / DPI / primary-monitor changes.
        place_window(app);
    }
    win::busy_fullscreen_active() || win::foreground_fills_monitor(hwnd)
}
