//! Updates, the same way Papka does them: GitHub Releases when the app can
//! reach the internet; otherwise the browser downloads `notch.exe` and the
//! app finds it in the Downloads folder. Installing swaps the EXE file;
//! the previous one stays as `notch.old.exe` for rollback.

use notch_core::update::{self, Update};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{lock, now_ms, Shared};
use crate::{net, win};

/// Version without a stamp (local builds): never checks by itself.
const DEV_VERSION: &str = "0.1.0";
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const WATCH_MS: u64 = 20 * 60 * 1000;

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    Idle,
    Checking,
    UpToDate { checked_at: u64 },
    /// `from_file`: found in Downloads (no download needed).
    Available { update: Update, from_file: bool },
    Downloading { done: u64, total: Option<u64> },
    /// Installed; takes effect after a restart.
    Ready { version: String },
    /// GitHub not reachable (firewall lets only the browser out).
    Offline { error: String },
    /// The browser is downloading; Downloads is being watched.
    Watching { until: u64 },
    Failed { error: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    version: String,
    status: Status,
    has_previous: bool,
    browser_url: String,
}

static STATUS: OnceLock<Mutex<Status>> = OnceLock::new();

fn status() -> &'static Mutex<Status> {
    STATUS.get_or_init(|| Mutex::new(Status::Idle))
}

fn set(app: &AppHandle, s: Status) {
    *lock(status()) = s.clone();
    let _ = app.emit("update", s);
}

pub fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

fn info(app: &AppHandle) -> Info {
    Info {
        version: current_version(app),
        status: lock(status()).clone(),
        has_previous: exe().map(|p| update::previous_path(&p).exists()).unwrap_or(false),
        browser_url: update::browser_url(),
    }
}

/// A newer EXE in Downloads, if any (cheap: reads file properties only).
fn scan_downloads(app: &AppHandle) -> Option<Update> {
    let folder = win::downloads_dir()?;
    let current = current_version(app);
    let own = exe().ok();
    let version_of = |p: &Path| if Some(p) == own.as_deref() { None } else { win::exe_version(p) };
    let (path, version) = update::find_downloaded(&folder, &current, &version_of)?;
    let bytes = std::fs::read(&path).ok()?;
    Some(Update {
        version,
        notes: Vec::new(),
        sha256: update::sha256_hex(&bytes),
        source: path.to_string_lossy().into_owned(),
        page: folder.to_string_lossy().into_owned(),
    })
}

async fn check_online(app: &AppHandle) -> Result<Option<Update>, String> {
    let list = net::get_bytes(&update::releases_url(), None).await?;
    let Some(candidate) = update::pick_release(&list).map_err(|e| e.to_string())? else { return Ok(None) };
    if update::compare_versions(&candidate.version, &current_version(app)) != std::cmp::Ordering::Greater {
        return Ok(None);
    }
    let manifest = net::get_bytes(&candidate.manifest_url.clone(), None).await?;
    update::make_update(&current_version(app), candidate, &manifest).map_err(|e| e.to_string())
}

/// Downloads first, then GitHub. `manual`: show "up to date"/errors.
pub async fn check(app: &AppHandle, manual: bool) {
    if matches!(*lock(status()), Status::Ready { .. } | Status::Downloading { .. }) {
        return;
    }
    if let Some(found) = scan_downloads(app) {
        set(app, Status::Available { update: found, from_file: true });
        return;
    }
    set(app, Status::Checking);
    let result = check_online(app).await;
    {
        let shared = app.state::<Shared>();
        let mut settings = lock(&shared.settings);
        settings.last_update_check = now_ms();
        let _ = settings.save(&shared.data_dir.join(notch_core::settings::SETTINGS_FILE));
    }
    match result {
        Ok(Some(found)) => set(app, Status::Available { update: found, from_file: false }),
        Ok(None) => set(app, Status::UpToDate { checked_at: now_ms() }),
        Err(error) if manual => set(app, Status::Offline { error }),
        // Quiet in the background: no network is the normal state here.
        Err(_) => set(app, Status::Idle),
    }
}

pub async fn install(app: &AppHandle) -> Result<(), String> {
    let Status::Available { update: found, .. } = lock(status()).clone() else {
        return Err("нечего устанавливать".into());
    };
    let bytes = if found.source.starts_with("https://") {
        set(app, Status::Downloading { done: 0, total: None });
        let progress_app = app.clone();
        let mut last = 0u64;
        let progress = move |done: u64, total: Option<u64>| {
            // About ten updates per download is plenty.
            if done.saturating_sub(last) > 4 << 20 || Some(done) == total {
                last = done;
                set(&progress_app, Status::Downloading { done, total });
            }
        };
        match net::get_bytes(&found.source, Some(Box::new(progress))).await {
            Ok(b) => b,
            Err(error) => {
                set(app, Status::Offline { error });
                return Err("не удалось скачать".into());
            }
        }
    } else {
        std::fs::read(&found.source).map_err(|e| e.to_string())?
    };
    let target = exe()?;
    match update::install(&target, &bytes, &found.sha256) {
        Ok(()) => {
            set(app, Status::Ready { version: found.version });
            Ok(())
        }
        Err(e) => {
            let error = if e.kind() == std::io::ErrorKind::PermissionDenied {
                "нет прав на запись в папку программы — перенесите notch.exe в свою папку".to_string()
            } else {
                e.to_string()
            };
            set(app, Status::Failed { error: error.clone() });
            Err(error)
        }
    }
}

/// Opens the download in the browser and watches Downloads for it.
pub fn via_browser(app: &AppHandle) {
    win::shell_open(&update::browser_url());
    set(app, Status::Watching { until: now_ms() + WATCH_MS });
}

/// Starts the (new) EXE and quits; the new one waits for this one to go.
pub fn restart(app: &AppHandle) -> Result<(), String> {
    let path = exe()?;
    std::process::Command::new(&path).arg(AFTER_UPDATE).spawn().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

pub fn rollback(app: &AppHandle) -> Result<(), String> {
    update::rollback(&exe()?).map_err(|e| e.to_string())?;
    restart(app)
}

pub const AFTER_UPDATE: &str = "--after-update";

/// Background schedule: Downloads hourly (every 5 s while the browser
/// downloads), GitHub once a day.
pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(90));
            let mut last_scan = 0u64;
            loop {
                let now = now_ms();
                let (auto, last_check) = {
                    let shared = app.state::<Shared>();
                    let s = lock(&shared.settings);
                    (s.auto_update, s.last_update_check)
                };
                let watching = matches!(*lock(status()), Status::Watching { until } if until > now);
                if matches!(*lock(status()), Status::Watching { until } if until <= now) {
                    set(&app, Status::Idle);
                }
                if watching || (auto && now.saturating_sub(last_scan) > 60 * 60 * 1000) {
                    last_scan = now;
                    if let Some(found) = scan_downloads(&app) {
                        if !matches!(*lock(status()), Status::Available { .. } | Status::Ready { .. } | Status::Downloading { .. }) {
                            set(&app, Status::Available { update: found, from_file: true });
                        }
                    }
                }
                let stamped = current_version(&app) != DEV_VERSION;
                if auto && stamped && !watching && now.saturating_sub(last_check) > DAY_MS {
                    tauri::async_runtime::block_on(check(&app, false));
                }
                std::thread::sleep(Duration::from_secs(if watching { 5 } else { 60 }));
            }
        })
        .expect("updates thread");
}

#[tauri::command]
pub fn update_info(app: AppHandle) -> Info {
    info(&app)
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<(), String> {
    check(&app, true).await;
    Ok(())
}

#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    install(&app).await
}

#[tauri::command]
pub fn update_browser(app: AppHandle) {
    via_browser(&app);
}

#[tauri::command]
pub fn update_restart(app: AppHandle) -> Result<(), String> {
    restart(&app)
}

#[tauri::command]
pub fn update_rollback(app: AppHandle) -> Result<(), String> {
    rollback(&app)
}
