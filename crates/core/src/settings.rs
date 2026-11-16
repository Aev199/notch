//! Portable settings: one JSON file next to the executable.

use crate::translate::Provider;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SETTINGS_FILE: &str = "notch.settings.json";
pub const HISTORY_FILE: &str = "notch.history.json";
/// Pinned clipboard entries: always saved (the user pinned them on purpose).
pub const PINS_FILE: &str = "notch.pins.json";
/// The file shelf: paths of the files put on it (the files stay where they are).
pub const FILES_FILE: &str = "notch.files.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub provider: Provider,
    pub deepl_key: String,
    /// OpenAI-compatible chat completions URL.
    pub llm_url: String,
    pub llm_model: String,
    pub llm_key: String,
    /// Global hotkey that translates the current clipboard text, e.g. `Ctrl+Alt+T`.
    /// Empty string disables it.
    pub hotkey: String,
    /// Copying the same text twice within a short interval (Ctrl+C, Ctrl+C) translates it.
    pub double_copy_translate: bool,
    pub shelf_limit: usize,
    pub text_limit: usize,
    /// Keep the text history in a JSON file next to the exe between runs.
    /// Off by default: the clipboard often holds passwords and tokens.
    pub persist_text_history: bool,
    /// Hide the island while a fullscreen app, game or presentation is active.
    pub hide_in_fullscreen: bool,
    /// Capture images from the clipboard to the shelf.
    pub capture_images: bool,
    /// Shrink the capsule to a thin strip after `idle_seconds` without activity.
    pub idle_thin: bool,
    pub idle_seconds: u64,
    /// Look for a new version once a day (GitHub; Downloads folder).
    pub auto_update: bool,
    /// Unix ms of the last online check (not shown in the UI).
    pub last_update_check: u64,
    /// PrintScreen opens Notch's own capture window (area, marks, text).
    /// Off for people who use another screenshot program.
    pub capture_printscreen: bool,
    /// Global hotkey that opens the clipboard history with the keyboard in
    /// the search field. Empty string disables it.
    pub history_hotkey: String,
    /// The "Файлы" tab: files dropped on the notch wait there.
    pub files_shelf: bool,
    /// Show the notch on the monitor the cursor is on (otherwise: primary).
    pub follow_cursor: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::Offline,
            deepl_key: String::new(),
            llm_url: "http://127.0.0.1:11434/v1/chat/completions".into(),
            llm_model: "qwen2.5:7b".into(),
            llm_key: String::new(),
            hotkey: "Ctrl+Alt+T".into(),
            double_copy_translate: true,
            shelf_limit: 30,
            text_limit: 30,
            persist_text_history: false,
            hide_in_fullscreen: true,
            capture_images: true,
            idle_thin: true,
            idle_seconds: 4,
            auto_update: true,
            last_update_check: 0,
            capture_printscreen: true,
            history_hotkey: "Ctrl+Alt+B".into(),
            files_shelf: true,
            follow_cursor: false,
        }
    }
}

impl Settings {
    /// Clamps numbers into sane ranges after loading or editing.
    pub fn sanitized(mut self) -> Self {
        self.shelf_limit = self.shelf_limit.clamp(1, 100);
        self.text_limit = self.text_limit.clamp(5, 100);
        self.idle_seconds = self.idle_seconds.clamp(1, 600);
        self.hotkey = self.hotkey.trim().to_string();
        self.history_hotkey = self.history_hotkey.trim().to_string();
        self.deepl_key = self.deepl_key.trim().to_string();
        self.llm_url = self.llm_url.trim().to_string();
        self
    }

    /// Reads the file; a missing or broken file gives the defaults (a broken
    /// file is kept aside as `.bak` so nothing typed by the user is lost).
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = fs::read_to_string(path) else { return Self::default() };
        match serde_json::from_str::<Settings>(&raw) {
            Ok(s) => s.sanitized(),
            Err(_) => {
                let _ = fs::rename(path, path.with_extension("json.bak"));
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(path, json.as_bytes())
    }
}

/// Writes through a temporary file so a crash never leaves half a JSON.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".notch-write-test");
    let ok = fs::write(&probe, b"ok").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// Folder for settings, history and the screenshot cache: the folder of the
/// executable when it is writable (portable mode), otherwise `fallback`
/// (e.g. `%TEMP%\Notch`) — for an exe started from a read-only place.
pub fn data_dir(exe: &Path, fallback: &Path) -> PathBuf {
    if let Some(dir) = exe.parent() {
        if is_writable(dir) {
            return dir.to_path_buf();
        }
    }
    let _ = fs::create_dir_all(fallback);
    fallback.to_path_buf()
}

/// Where the data goes when the folder of the executable is read-only:
/// `%LOCALAPPDATA%\Notch`. Earlier versions used `%TEMP%\Notch`, which Windows
/// cleans up by itself (settings and pinned entries vanished with it); data
/// found there moves over. Without `local_app_data`, or if the move fails,
/// the old place stays in use.
pub fn fallback_dir(local_app_data: Option<&Path>, temp: &Path) -> PathBuf {
    let old = temp.join("Notch");
    let Some(local) = local_app_data else { return old };
    let new = local.join("Notch");
    if old.is_dir() && !new.exists() && fs::rename(&old, &new).is_err() {
        return old;
    }
    new
}

pub fn shelf_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cache").join("shelf")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("notch-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip_and_partial_files() {
        let dir = temp("rt");
        let path = dir.join(SETTINGS_FILE);
        assert_eq!(Settings::load(&path), Settings::default());
        let s = Settings { provider: Provider::Deepl, deepl_key: " key:fx ".into(), ..Default::default() };
        s.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.provider, Provider::Deepl);
        assert_eq!(loaded.deepl_key, "key:fx");

        fs::write(&path, r#"{"shelfLimit": 500, "provider": "llm"}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.shelf_limit, 100);
        assert_eq!(loaded.provider, Provider::Llm);
        assert_eq!(loaded.hotkey, "Ctrl+Alt+T");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn broken_file_is_backed_up() {
        let dir = temp("broken");
        let path = dir.join(SETTINGS_FILE);
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert!(dir.join("notch.settings.json.bak").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn data_dir_prefers_exe_folder() {
        let dir = temp("data");
        let exe = dir.join("notch.exe");
        let fallback = dir.join("fallback");
        assert_eq!(data_dir(&exe, &fallback), dir);
        assert_eq!(shelf_dir(&dir), dir.join("cache").join("shelf"));
        assert_eq!(data_dir(Path::new("/proc/notch.exe"), &fallback), fallback);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fallback_moves_out_of_temp() {
        let dir = temp("fallback");
        let (temp_dir, local) = (dir.join("temp"), dir.join("local"));
        fs::create_dir_all(&local).unwrap();
        assert_eq!(fallback_dir(None, &temp_dir), temp_dir.join("Notch"));
        assert_eq!(fallback_dir(Some(&local), &temp_dir), local.join("Notch"));
        // Data left in %TEMP% by an older version comes along.
        fs::create_dir_all(temp_dir.join("Notch")).unwrap();
        fs::write(temp_dir.join("Notch").join(PINS_FILE), b"[]").unwrap();
        assert_eq!(fallback_dir(Some(&local), &temp_dir), local.join("Notch"));
        assert!(local.join("Notch").join(PINS_FILE).exists());
        assert!(!temp_dir.join("Notch").exists());
        // Both exist: the new place wins, the old one is left alone.
        fs::create_dir_all(temp_dir.join("Notch")).unwrap();
        assert_eq!(fallback_dir(Some(&local), &temp_dir), local.join("Notch"));
        assert!(temp_dir.join("Notch").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
