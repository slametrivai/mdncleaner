//! "Cek pembaruan": finds a newer release on GitHub, downloads it in the
//! background with progress events, and installs it now or when the app
//! closes. Releases are signed; the updater refuses an unsigned download.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const RELEASES_URL: &str = "https://github.com/slametrivai/mdncleaner/releases";

#[derive(Default)]
pub struct UpdateState {
    pending: Mutex<Option<Update>>,
    bytes: Mutex<Option<Vec<u8>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    /// "Nanti" on the ready dialog: install when the app exits.
    install_on_exit: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release date, milliseconds since the epoch.
    pub date_ms: Option<i64>,
    pub notes: Option<String>,
    pub release_url: String,
    /// Already downloaded and waiting to be installed.
    pub ready: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn friendly(error: impl std::fmt::Display) -> String {
    let text = error.to_string();
    let lower = text.to_lowercase();
    if lower.contains("dns") || lower.contains("connect") || lower.contains("timed out") || lower.contains("network") {
        "Tidak bisa terhubung ke GitHub. Periksa koneksi internet lalu coba lagi.".into()
    } else if lower.contains("signature") {
        "File pembaruan gagal diverifikasi, jadi tidak dipasang.".into()
    } else if lower.contains("404") || lower.contains("release json") {
        "Belum ada rilis yang bisa dipakai untuk pembaruan.".into()
    } else {
        format!("Pembaruan gagal: {text}")
    }
}

fn info(update: &Update, ready: bool) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        date_ms: update.date.map(|d| (d.unix_timestamp_nanos() / 1_000_000) as i64),
        notes: update.body.clone().filter(|b| !b.trim().is_empty()),
        release_url: format!("{RELEASES_URL}/tag/v{}", update.version),
        ready,
    }
}

/// None means this is the newest version.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater_builder().timeout(Duration::from_secs(20)).build().map_err(friendly)?;
    let found = updater.check().await.map_err(friendly)?;
    let state = app.state::<UpdateState>();
    let ready = lock(&state.bytes).is_some();
    match found {
        Some(update) => {
            let result = info(&update, ready);
            // A different version than the one already downloaded invalidates it.
            let same = lock(&state.pending).as_ref().map(|u| u.version == update.version).unwrap_or(false);
            if !same {
                *lock(&state.bytes) = None;
            }
            *lock(&state.pending) = Some(update);
            Ok(Some(UpdateInfo { ready: same && ready, ..result }))
        }
        None => Ok(None),
    }
}

/// Starts the download in the background. Emits `update-progress` while it
/// runs, then `update-ready` or `update-failed`.
pub fn start_download(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<UpdateState>();
    let update = lock(&state.pending).clone().ok_or("Cek pembaruan dulu")?;
    if lock(&state.task).as_ref().is_some_and(|t| !t.inner().is_finished()) {
        return Ok(());
    }
    let handle = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut downloaded = 0u64;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let progress_app = handle.clone();
        let result = update
            .download(
                |chunk, total| {
                    downloaded += chunk as u64;
                    // A few updates a second is plenty for a progress bar.
                    if last_emit.elapsed() >= Duration::from_millis(120) || Some(downloaded) == total {
                        last_emit = Instant::now();
                        let _ = progress_app.emit("update-progress", Progress { downloaded, total });
                    }
                },
                || {},
            )
            .await;
        let state = handle.state::<UpdateState>();
        match result {
            Ok(bytes) => {
                *lock(&state.bytes) = Some(bytes);
                let _ = handle.emit("update-ready", info(&update, true));
            }
            Err(error) => {
                let _ = handle.emit("update-failed", friendly(error));
            }
        }
    });
    *lock(&state.task) = Some(task);
    Ok(())
}

pub fn cancel_download(app: &AppHandle) {
    let state = app.state::<UpdateState>();
    let task = lock(&state.task).take();
    if let Some(task) = task {
        task.abort();
    }
}

/// Installs the downloaded update. On Windows this starts the installer and
/// closes the app; the installer opens MDN Cleaner again when it is done.
pub fn install_now(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<UpdateState>();
    let update = lock(&state.pending).clone().ok_or("Tidak ada pembaruan")?;
    let bytes = lock(&state.bytes).take().ok_or("Pembaruan belum selesai diunduh")?;
    update.install(bytes).map_err(friendly)
}

pub fn set_install_on_exit(app: &AppHandle, enabled: bool) {
    app.state::<UpdateState>().install_on_exit.store(enabled, Ordering::Relaxed);
}

/// Called as the app exits: installs a downloaded update the user postponed.
pub fn install_if_postponed(app: &AppHandle) {
    let state = app.state::<UpdateState>();
    let downloaded = lock(&state.bytes).is_some();
    if state.install_on_exit.load(Ordering::Relaxed) && downloaded {
        let _ = install_now(app);
    }
}
