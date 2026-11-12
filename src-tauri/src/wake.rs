//! Repaints the transparent overlay after the system wakes up.
//!
//! After sleep, a locked session or the display turning off, Windows rebuilds
//! the GPU side and WebView2's transparent background may come back black:
//! a dark rectangle the size of the window stays on the desktop while the
//! island keeps working on top of it. Nothing tells WebView2 to fix itself,
//! so we listen for those moments and give the window a nudge that makes
//! WebView2 and DWM rebuild its surface.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalSize};

use crate::state::Shared;
use crate::win;

static SIGNAL: OnceLock<Mutex<Sender<&'static str>>> = OnceLock::new();

/// Something happened after which the window may need repainting.
pub fn notify(reason: &'static str) {
    if let Some(tx) = SIGNAL.get() {
        let _ = tx.lock().map(|tx| tx.send(reason));
    }
}

pub fn spawn(app: AppHandle) {
    let (tx, rx) = channel();
    let _ = SIGNAL.set(Mutex::new(tx));
    #[cfg(windows)]
    std::thread::Builder::new().name("wake-listener".into()).spawn(listener::run).expect("wake listener");
    std::thread::Builder::new().name("wake-repair".into()).spawn(move || repair_loop(app, rx)).expect("wake repair");
}

fn repair_loop(app: AppHandle, rx: Receiver<&'static str>) {
    while rx.recv().is_ok() {
        // Several messages come at once (resume + unlock + display on): one repair.
        while rx.recv_timeout(Duration::from_millis(400)).is_ok() {}
        // The GPU driver needs a moment after resume; repeat once more later.
        for wait in [1_500, 3_500] {
            std::thread::sleep(Duration::from_millis(wait));
            repair(&app);
        }
    }
}

/// Re-applies transparency and forces the window surface to be rebuilt.
/// Harmless when nothing was wrong (the window flickers for one frame).
pub fn repair(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let shared = app.state::<Shared>().inner().clone();
    let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    if let Ok(size) = window.outer_size() {
        let _ = window.set_size(PhysicalSize::new(size.width, size.height + 1));
        std::thread::sleep(Duration::from_millis(60));
        let _ = window.set_size(size);
    }
    if !shared.hidden_for_fullscreen.load(std::sync::atomic::Ordering::Relaxed) {
        let hwnd = shared.hwnd();
        win::show_no_activate(hwnd, false);
        std::thread::sleep(Duration::from_millis(40));
        win::show_no_activate(hwnd, true);
        win::keep_topmost(hwnd);
    }
    let _ = app.emit("repaint", ());
}

/// A hidden top-level window (power and session broadcasts do not reach
/// message-only windows) that turns system messages into `notify` calls.
#[cfg(windows)]
mod listener {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
    use windows_sys::Win32::System::RemoteDesktop::WTSRegisterSessionNotification;
    use windows_sys::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, MSG,
        PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_POWERSETTINGCHANGE, WM_DISPLAYCHANGE,
        WM_DWMCOMPOSITIONCHANGED, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
        WTS_SESSION_UNLOCK,
    };

    const NOTIFY_FOR_THIS_SESSION: u32 = 0;
    const DEVICE_NOTIFY_WINDOW_HANDLE: u32 = 0;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_POWERBROADCAST => {
                let event = wparam as u32;
                if event == PBT_APMRESUMEAUTOMATIC || event == PBT_APMRESUMESUSPEND {
                    super::notify("resume");
                } else if event == PBT_POWERSETTINGCHANGE && lparam != 0 {
                    let setting = &*(lparam as *const POWERBROADCAST_SETTING);
                    let g = setting.PowerSetting;
                    let d = GUID_CONSOLE_DISPLAY_STATE;
                    let display = g.data1 == d.data1 && g.data2 == d.data2 && g.data3 == d.data3 && g.data4 == d.data4;
                    // Data: 0 = off, 1 = on, 2 = dimmed.
                    if display && setting.DataLength >= 1 && setting.Data[0] == 1 {
                        super::notify("display on");
                    }
                }
                1
            }
            WM_WTSSESSION_CHANGE => {
                if wparam as u32 == WTS_SESSION_UNLOCK {
                    super::notify("unlock");
                }
                0
            }
            WM_DISPLAYCHANGE | WM_DWMCOMPOSITIONCHANGED => {
                super::notify("display change");
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    pub fn run() {
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide("NotchWakeListener");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            let title = wide("Notch wake listener");
            // Never shown: a tool window with zero size.
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return;
            }
            WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            RegisterPowerSettingNotification(hwnd, &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
