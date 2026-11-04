//! Thin Win32 layer. Every function has a no-op twin for other platforms so
//! the crate still type-checks outside Windows.

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW};
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::{AttachThreadInput, CreateMutexW, GetCurrentThreadId};
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL, VK_LBUTTON, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_V,
    };
    use windows_sys::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
        GetClassNameW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetForegroundWindow, SetWindowLongPtrW,
        SetWindowPos, ShowWindow, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, HWND_TOPMOST, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
        WS_EX_TRANSPARENT,
    };
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn hwnd(raw: isize) -> HWND {
        raw as HWND
    }

    /// Keeps the extended styles we own in place: always a tool window (no
    /// Alt+Tab), click-through (`WS_EX_TRANSPARENT | WS_EX_LAYERED`) on
    /// demand. Tauri rewrites the whole extended style whenever it changes
    /// something itself (focusable, focus...), so this runs on every overlay
    /// tick and repairs any such rewrite within one tick. All writes come from
    /// one thread, so there is no race between two writers any more.
    pub fn apply_styles(raw: isize, click_through: bool) {
        unsafe {
            let h = hwnd(raw);
            if h.is_null() {
                return;
            }
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let pass = (WS_EX_TRANSPARENT | WS_EX_LAYERED) as isize;
            let mut want = (ex | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);
            want = if click_through { want | pass } else { want & !pass };
            if want != ex {
                SetWindowLongPtrW(h, GWL_EXSTYLE, want);
                SetWindowPos(
                    h,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
        }
    }

    /// The foreground window covers the whole monitor our window is on
    /// (browser F11, borderless games and players, fullscreen video...).
    /// The desktop and the taskbar do not count.
    pub fn foreground_fills_monitor(ours: isize) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() || fg as isize == ours {
                return false;
            }
            let mut class = [0u16; 64];
            let len = GetClassNameW(fg, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
            let class = String::from_utf16_lossy(&class[..len]);
            if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
                return false;
            }
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if GetWindowRect(fg, &mut r) == 0 {
                return false;
            }
            let monitor = MonitorFromWindow(hwnd(ours), MONITOR_DEFAULTTOPRIMARY);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return false;
            }
            let m = info.rcMonitor;
            r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
        }
    }

    pub fn cursor_pos() -> Option<(f64, f64)> {
        let mut p = POINT { x: 0, y: 0 };
        (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x as f64, p.y as f64))
    }

    pub fn left_button_down() -> bool {
        (unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } as u16 & 0x8000) != 0
    }

    /// Window rectangle in physical pixels plus its scale factor.
    pub fn window_rect(raw: isize) -> Option<(f64, f64, f64, f64, f64)> {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe {
            if GetWindowRect(hwnd(raw), &mut r) == 0 {
                return None;
            }
            let dpi = GetDpiForWindow(hwnd(raw));
            let scale = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };
            Some((r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64, scale))
        }
    }

    fn notification_state() -> i32 {
        let mut state = 0;
        if unsafe { SHQueryUserNotificationState(&mut state) } < 0 {
            return 0;
        }
        state
    }

    /// An exclusive-fullscreen Direct3D game or presentation mode: the island
    /// is hidden completely.
    pub fn exclusive_fullscreen_active() -> bool {
        matches!(notification_state(), QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
    }

    /// Windows considers a fullscreen app "busy" (video, F11 browser...): the
    /// island shrinks to a strip.
    pub fn busy_fullscreen_active() -> bool {
        notification_state() == QUNS_BUSY
    }

    /// Puts the window back at the top of the topmost band: fullscreen
    /// players and other overlays push themselves above it.
    pub fn keep_topmost(raw: isize) {
        unsafe {
            SetWindowPos(hwnd(raw), HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    pub fn is_visible(raw: isize) -> bool {
        unsafe { IsWindowVisible(hwnd(raw)) != 0 }
    }

    /// Shows or hides the window without activating it.
    pub fn show_no_activate(raw: isize, show: bool) {
        unsafe {
            ShowWindow(hwnd(raw), if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }

    pub fn foreground_window() -> isize {
        unsafe { GetForegroundWindow() as isize }
    }

    /// Brings a window to the foreground even when Windows' foreground lock
    /// would normally refuse (we are a no-activate overlay, so we never "own"
    /// the last input). Uses the documented AttachThreadInput technique.
    pub fn force_foreground(raw: isize) {
        unsafe {
            let target = hwnd(raw);
            if IsWindow(target) == 0 {
                return;
            }
            if SetForegroundWindow(target) != 0 {
                return;
            }
            let fg = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            let me = GetCurrentThreadId();
            if fg_thread != 0 && fg_thread != me {
                AttachThreadInput(me, fg_thread, 1);
                BringWindowToTop(target);
                SetForegroundWindow(target);
                AttachThreadInput(me, fg_thread, 0);
            }
        }
    }

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 },
            },
        }
    }

    fn key_down(vk: VIRTUAL_KEY) -> bool {
        (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0
    }

    /// Presses Ctrl+V in whatever window has the keyboard focus. Modifiers the
    /// user may still hold (Alt, Shift, Win) are released first so the target
    /// sees a clean Ctrl+V.
    pub fn send_paste() -> bool {
        let mut inputs = Vec::new();
        for vk in [VK_MENU, VK_LMENU, VK_RMENU, VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_LWIN, VK_RWIN] {
            if key_down(vk) {
                inputs.push(key(vk, true));
            }
        }
        inputs.extend([key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)]);
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        sent as usize == inputs.len()
    }

    /// Password managers mark secrets with these formats; such clipboard
    /// content must not be recorded.
    pub fn clipboard_marked_private() -> bool {
        unsafe {
            for name in ["ExcludeClipboardContentFromMonitorProcessing", "Clipboard Viewer Ignore"] {
                let fmt = RegisterClipboardFormatW(wide(name).as_ptr());
                if fmt != 0 && IsClipboardFormatAvailable(fmt) != 0 {
                    return true;
                }
            }
            false
        }
    }

    /// Returns `false` when another copy of the app already runs in this session.
    /// The mutex handle is intentionally leaked: it lives as long as the process.
    pub fn claim_single_instance(name: &str) -> bool {
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, wide(name).as_ptr());
            !(handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS)
        }
    }

    pub fn autostart_get(name: &str) -> Option<String> {
        let key = wide(RUN_KEY);
        let value = wide(name);
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut c_void,
                &mut size,
            )
        };
        if rc != 0 {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..len.min(buf.len())]))
    }

    pub fn autostart_set(name: &str, command: Option<&str>) -> Result<(), String> {
        let key = wide(RUN_KEY);
        let value = wide(name);
        let rc = unsafe {
            match command {
                Some(cmd) => {
                    let data = wide(cmd);
                    RegSetKeyValueW(
                        HKEY_CURRENT_USER,
                        key.as_ptr(),
                        value.as_ptr(),
                        REG_SZ,
                        data.as_ptr() as *const c_void,
                        (data.len() * 2) as u32,
                    )
                }
                None => {
                    let rc = RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr());
                    if rc == 2 { 0 } else { rc } // ERROR_FILE_NOT_FOUND: already absent
                }
            }
        };
        if rc == 0 { Ok(()) } else { Err(format!("реестр: ошибка {rc}")) }
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
mod imp {
    pub fn apply_styles(_: isize, _: bool) {}
    pub fn foreground_fills_monitor(_: isize) -> bool {
        false
    }
    pub fn cursor_pos() -> Option<(f64, f64)> {
        None
    }
    pub fn left_button_down() -> bool {
        false
    }
    pub fn window_rect(_: isize) -> Option<(f64, f64, f64, f64, f64)> {
        None
    }
    pub fn exclusive_fullscreen_active() -> bool {
        false
    }
    pub fn busy_fullscreen_active() -> bool {
        false
    }
    pub fn keep_topmost(_: isize) {}
    pub fn is_visible(_: isize) -> bool {
        true
    }
    pub fn show_no_activate(_: isize, _: bool) {}
    pub fn foreground_window() -> isize {
        0
    }
    pub fn force_foreground(_: isize) {}
    pub fn clipboard_marked_private() -> bool {
        false
    }
    pub fn send_paste() -> bool {
        false
    }
    /// Returns `false` when another copy of the app already runs in this session.
    /// The mutex handle is intentionally leaked: it lives as long as the process.
    pub fn claim_single_instance(name: &str) -> bool {
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, wide(name).as_ptr());
            !(handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS)
        }
    }

    pub fn claim_single_instance(_: &str) -> bool {
        true
    }
    pub fn autostart_get(_: &str) -> Option<String> {
        None
    }
    pub fn autostart_set(_: &str, _: Option<&str>) -> Result<(), String> {
        Err("автозапуск есть только в Windows".into())
    }
}

pub use imp::*;
