// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cleaner;
mod cloud;
mod disk;
mod dupes;
mod history;
mod paths;
mod schedule;
mod settings;
mod state;
mod updater;

use cleaner::{CleanReport, ItemId, ScanEnv, ScanItem};
use cloud::{CloudScan, CloudStatus, Location};
use disk::{Analysis, Drive};
use dupes::DupGroup;
use history::Kind;
use paths::{user_dir, Guard};
use settings::Settings;
use state::{AppState, ScanOptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tauri::async_runtime::spawn_blocking;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

#[derive(Default)]
struct Shared {
    app: Mutex<AppState>,
    /// Duplicate groups from the last PC scan, for "Bersihkan sekarang".
    pc_dupes: Mutex<Vec<DupGroup>>,
    cloud: Mutex<HashMap<Location, cloud::Scanned>>,
    analyses: Mutex<HashMap<String, Analysis>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

type CmdResult<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> CmdResult<T> {
    spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// "5,36 GB" / "842 MB", for notifications.
fn format_bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB).replace('.', ",")
    } else {
        format!("{:.0} MB", b / MB)
    }
}

/// The user's choices, turned into what a scan or clean needs.
fn scan_env(state: &AppState) -> ScanEnv {
    let system = disk::system_drive();
    let system_included = state.drive_included(&system);
    let mut dup_roots: Vec<PathBuf> = Vec::new();
    if system_included {
        dup_roots.extend(
            ["Desktop", "Documents", "Downloads", "Pictures", "Videos", "Music"].iter().filter_map(|d| user_dir(d)),
        );
    }
    for drive in disk::drives().into_iter().filter(|d| !d.system && state.drive_included(&d.letter)) {
        dup_roots.push(disk::root_of(&drive.letter));
    }
    dup_roots.retain(|p| p.is_dir());
    ScanEnv {
        options: state.scan_options.clone(),
        guard: Guard::new(&state.skipped_folders),
        system_drive: system_included,
        dup_roots,
        bin: history::protected_bin(state),
        recent: state.settings.recent_window(),
        use_recycle_bin: state.settings.use_recycle_bin,
    }
}

fn cloud_root(settings: &Settings, location: Location) -> Option<String> {
    match location {
        Location::GoogleDrive => settings.google_drive_root.clone(),
        Location::IcloudDrive => settings.icloud_drive_root.clone(),
        Location::Pc => None,
    }
}

// ── State and settings ──────────────────────────────────────

#[tauri::command]
fn get_state(shared: State<'_, Shared>) -> AppState {
    // Re-read the file: a scheduled clean may have run since the app opened.
    let mut s = lock(&shared.app);
    *s = AppState::load();
    if history::purge_expired(&mut s) {
        let _ = s.save();
    }
    s.clone()
}

#[tauri::command]
fn set_selection(id: ItemId, checked: bool, shared: State<'_, Shared>) -> CmdResult<()> {
    let mut s = lock(&shared.app);
    s.selection.insert(id, checked);
    s.save()
}

#[tauri::command]
fn set_scan_options(options: ScanOptions, shared: State<'_, Shared>) -> CmdResult<AppState> {
    let mut s = lock(&shared.app);
    s.scan_options = options;
    s.save()?;
    Ok(s.clone())
}

#[tauri::command]
fn set_drive_included(letter: String, included: bool, shared: State<'_, Shared>) -> CmdResult<AppState> {
    let mut s = lock(&shared.app);
    s.excluded_drives.retain(|d| !d.eq_ignore_ascii_case(&letter));
    if !included {
        s.excluded_drives.push(letter);
    }
    s.save()?;
    Ok(s.clone())
}

fn pick_folder(app: &AppHandle, title: &str) -> Option<String> {
    app.dialog()
        .file()
        .set_title(title)
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn add_skipped_folder(app: AppHandle) -> CmdResult<AppState> {
    let handle = app.clone();
    let picked = blocking(move || pick_folder(&handle, "Pilih folder yang dilewati")).await?;
    let shared = app.state::<Shared>();
    let mut s = lock(&shared.app);
    if let Some(path) = picked {
        if !s.skipped_folders.iter().any(|f| f.eq_ignore_ascii_case(&path)) {
            s.skipped_folders.push(path);
            s.save()?;
        }
    }
    Ok(s.clone())
}

#[tauri::command]
fn remove_skipped_folder(path: String, shared: State<'_, Shared>) -> CmdResult<AppState> {
    let mut s = lock(&shared.app);
    s.skipped_folders.retain(|f| f != &path);
    s.save()?;
    Ok(s.clone())
}

#[tauri::command]
fn set_auto_clean(enabled: bool, shared: State<'_, Shared>) -> CmdResult<AppState> {
    let mut s = lock(&shared.app);
    schedule::set_enabled(enabled, &s.settings)?;
    s.auto_clean = enabled;
    s.save()?;
    Ok(s.clone())
}

/// Saves "Pengaturan" and applies what reaches outside the app: the Run key
/// for start-up and the scheduled task.
#[tauri::command]
fn set_settings(mut settings: Settings, shared: State<'_, Shared>) -> CmdResult<AppState> {
    let mut s = lock(&shared.app);
    let old = s.settings.clone();
    settings.last_reminded_ms = old.last_reminded_ms;
    settings.last_update_check_ms = old.last_update_check_ms;
    if settings.start_with_windows != old.start_with_windows {
        settings::set_start_with_windows(settings.start_with_windows)?;
    }
    if s.auto_clean && (settings.schedule != old.schedule || settings.only_on_ac != old.only_on_ac) {
        schedule::set_enabled(true, &settings)?;
    }
    s.settings = settings;
    s.save()?;
    Ok(s.clone())
}

#[tauri::command]
async fn pick_cloud_folder(location: Location, app: AppHandle) -> CmdResult<AppState> {
    let handle = app.clone();
    let title = match location {
        Location::IcloudDrive => "Pilih folder iCloud Drive",
        _ => "Pilih folder Google Drive",
    };
    let picked = blocking(move || pick_folder(&handle, title)).await?;
    let shared = app.state::<Shared>();
    let mut s = lock(&shared.app);
    if let Some(path) = picked {
        match location {
            Location::GoogleDrive => s.settings.google_drive_root = Some(path),
            Location::IcloudDrive => s.settings.icloud_drive_root = Some(path),
            Location::Pc => {}
        }
        lock(&shared.cloud).remove(&location);
        s.save()?;
    }
    Ok(s.clone())
}

/// "Kembalikan ke pengaturan awal". History and files stay.
#[tauri::command]
fn reset_settings(shared: State<'_, Shared>) -> CmdResult<AppState> {
    let mut s = lock(&shared.app);
    settings::set_start_with_windows(false)?;
    schedule::set_enabled(false, &s.settings)?;
    s.auto_clean = false;
    s.settings = Settings::default();
    s.scan_options = ScanOptions::default();
    s.excluded_drives.clear();
    s.skipped_folders.clear();
    s.selection.clear();
    s.save()?;
    Ok(s.clone())
}

/// A native confirmation, for "Tanya dulu sebelum menghapus".
#[tauri::command]
async fn confirm(title: String, message: String, ok: String, app: AppHandle) -> CmdResult<bool> {
    blocking(move || {
        app.dialog()
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(ok, "Batal".into()))
            .blocking_show()
    })
    .await
}

// ── Pembaruan ───────────────────────────────────────────────

/// None means MDN Cleaner is up to date.
#[tauri::command]
async fn check_updates(app: AppHandle) -> CmdResult<Option<updater::UpdateInfo>> {
    let result = updater::check(&app).await?;
    let shared = app.state::<Shared>();
    let mut s = lock(&shared.app);
    s.settings.last_update_check_ms = state::now_ms();
    let _ = s.save();
    Ok(result)
}

#[tauri::command]
fn download_update(app: AppHandle) -> CmdResult<()> {
    updater::start_download(&app)
}

#[tauri::command]
fn cancel_update(app: AppHandle) {
    updater::cancel_download(&app);
}

#[tauri::command]
fn install_update(app: AppHandle) -> CmdResult<()> {
    updater::install_now(&app)
}

#[tauri::command]
fn install_update_on_exit(enabled: bool, app: AppHandle) {
    updater::set_install_on_exit(&app, enabled);
}

// ── Pembersih ───────────────────────────────────────────────

#[tauri::command]
async fn scan(app: AppHandle) -> CmdResult<Vec<ScanItem>> {
    blocking(move || {
        let shared = app.state::<Shared>();
        let env = scan_env(&lock(&shared.app));
        let (items, dups) = cleaner::scan(&env);
        *lock(&shared.pc_dupes) = dups;
        items
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CleanOutcome {
    report: CleanReport,
    state: AppState,
}

#[tauri::command]
async fn clean(ids: Vec<ItemId>, app: AppHandle) -> CmdResult<CleanOutcome> {
    blocking(move || {
        let shared = app.state::<Shared>();
        let env = scan_env(&lock(&shared.app));
        let dups = lock(&shared.pc_dupes).clone();
        let report = cleaner::clean(&ids, &env, &dups);
        let mut s = lock(&shared.app);
        history::record(&mut s, Kind::Manual, vec![Location::Pc], &report, None);
        s.save()?;
        Ok(CleanOutcome { report, state: s.clone() })
    })
    .await?
}

// ── Disks ───────────────────────────────────────────────────

#[tauri::command]
async fn drives() -> CmdResult<Vec<Drive>> {
    blocking(disk::drives).await
}

fn analysis_file(letter: &str) -> Option<PathBuf> {
    state::data_dir().map(|d| d.join("analysis").join(format!("{}.json", letter.trim_end_matches(':'))))
}

#[tauri::command]
fn last_analysis(letter: String, shared: State<'_, Shared>) -> Option<Analysis> {
    let mut cache = lock(&shared.analyses);
    if let Some(a) = cache.get(&letter) {
        return Some(a.clone());
    }
    let loaded: Analysis = analysis_file(&letter).and_then(|p| state::read_json(&p))?;
    cache.insert(letter, loaded.clone());
    Some(loaded)
}

#[tauri::command]
async fn analyze(letter: String, app: AppHandle) -> CmdResult<Analysis> {
    blocking(move || {
        let shared = app.state::<Shared>();
        let (guard, large) = {
            let s = lock(&shared.app);
            (Guard::new(&s.skipped_folders), s.settings.large_file_bytes())
        };
        let analysis = disk::analyze(&letter, guard, large);
        if let Some(path) = analysis_file(&letter) {
            let _ = state::write_json(&path, &analysis);
        }
        lock(&shared.analyses).insert(letter, analysis.clone());
        analysis
    })
    .await
}

#[tauri::command]
fn analysis_progress() -> u64 {
    disk::FILES_SEEN.load(std::sync::atomic::Ordering::Relaxed)
}

// ── Cloud ───────────────────────────────────────────────────

#[tauri::command]
async fn cloud_status(location: Location, app: AppHandle) -> CmdResult<CloudStatus> {
    blocking(move || {
        let shared = app.state::<Shared>();
        let chosen = cloud_root(&lock(&shared.app).settings, location);
        cloud::status(location, chosen.as_deref())
    })
    .await
}

#[tauri::command]
async fn cloud_scan(location: Location, app: AppHandle) -> CmdResult<CloudScan> {
    blocking(move || {
        let shared = app.state::<Shared>();
        let (guard, chosen, large) = {
            let s = lock(&shared.app);
            (Guard::new(&s.skipped_folders), cloud_root(&s.settings, location), s.settings.large_file_bytes())
        };
        let scanned = cloud::scan(location, chosen.as_deref(), &guard, large)?;
        let view = scanned.view.clone();
        lock(&shared.cloud).insert(location, scanned);
        Ok(view)
    })
    .await?
}

#[tauri::command]
async fn cloud_delete(location: Location, dup_ids: Vec<String>, files: Vec<String>, app: AppHandle) -> CmdResult<CleanOutcome> {
    blocking(move || {
        let shared = app.state::<Shared>();
        let report = {
            let scans = lock(&shared.cloud);
            let scanned = scans.get(&location).ok_or("Scan dulu sebelum menghapus")?;
            cloud::delete(scanned, &dup_ids, &files)
        };
        let kind = if dup_ids.is_empty() { Kind::LargeFiles } else { Kind::Duplicates };
        let mut s = lock(&shared.app);
        history::record(&mut s, kind, vec![location], &report, Some(location));
        s.save()?;
        Ok(CleanOutcome { report, state: s.clone() })
    })
    .await?
}

// ── Riwayat ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RestoreOutcome {
    restored: usize,
    state: AppState,
}

#[tauri::command]
async fn restore(id: String, app: AppHandle) -> CmdResult<RestoreOutcome> {
    blocking(move || {
        let shared = app.state::<Shared>();
        let mut s = lock(&shared.app);
        let result = history::restore(&mut s, &id);
        s.save()?;
        result.map(|restored| RestoreOutcome { restored, state: s.clone() })
    })
    .await?
}

#[tauri::command]
fn open_cloud_trash(location: Location, app: AppHandle) -> CmdResult<()> {
    app.opener().open_url(location.trash_url(), None::<&str>).map_err(|e| e.to_string())
}

// ── Shell ───────────────────────────────────────────────────

#[tauri::command]
fn reveal(path: String, app: AppHandle) -> CmdResult<()> {
    app.opener().reveal_item_in_dir(&path).map_err(|e| e.to_string())
}

/// Opens a web page, the Microsoft Store or the mail app. Nothing else.
#[tauri::command]
fn open_external(url: String, app: AppHandle) -> CmdResult<()> {
    const ALLOWED: [&str; 3] = ["https://", "mailto:", "ms-windows-store://"];
    if !ALLOWED.iter().any(|p| url.starts_with(p)) {
        return Err("Alamat ini tidak diizinkan".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

// ── Background work ─────────────────────────────────────────

/// "Ingatkan kalau sampah menumpuk": while the app runs (also from the
/// tray), check every few hours and remind at most once a day.
fn junk_reminder(app: AppHandle) {
    const DAY_MS: u64 = 24 * 60 * 60 * 1000;
    std::thread::sleep(Duration::from_secs(120));
    loop {
        let shared = app.state::<Shared>();
        let state = lock(&shared.app).clone();
        let due = state.settings.remind_junk && state::now_ms() >= state.settings.last_reminded_ms + DAY_MS;
        if due {
            // Duplicates take long to find and are never cleaned unasked; leave them out.
            let mut env = scan_env(&state);
            env.options.duplicates = false;
            let (items, _) = cleaner::scan(&env);
            let junk = cleaner::checked_bytes(&items, &state.selection);
            if junk >= settings::REMIND_BYTES {
                settings::notify(
                    "Sampah sudah menumpuk",
                    &format!("Ada {} sampah di PC ini. Buka MDN Cleaner untuk membersihkannya.", format_bytes(junk)),
                );
                let mut s = lock(&shared.app);
                s.settings.last_reminded_ms = state::now_ms();
                let _ = s.save();
            }
        }
        std::thread::sleep(Duration::from_secs(6 * 60 * 60));
    }
}

/// Started by Task Scheduler: clean this PC, check the cloud folders for
/// duplicates (never deleted unattended), record it and tell the user.
fn run_auto_clean() {
    let mut s = AppState::load();
    history::purge_expired(&mut s);
    let cfg = s.settings.clone();
    if cfg.only_on_ac && !settings::on_ac_power() {
        let _ = s.save();
        return;
    }

    let mut lines = Vec::new();
    if cfg.auto_locations.pc {
        let env = scan_env(&s);
        let report = cleaner::clean(&cleaner::saved_selection(&s.selection), &env, &[]);
        history::record(&mut s, Kind::Auto, vec![Location::Pc], &report, None);
        lines.push(format!("{} dibebaskan dari PC ini.", format_bytes(report.bytes)));
    }

    let guard = Guard::new(&s.skipped_folders);
    for (location, enabled, name) in [
        (Location::GoogleDrive, cfg.auto_locations.google_drive, "Google Drive"),
        (Location::IcloudDrive, cfg.auto_locations.icloud_drive, "iCloud Drive"),
    ] {
        if !enabled {
            continue;
        }
        let chosen = cloud_root(&cfg, location);
        if let Ok(scanned) = cloud::scan(location, chosen.as_deref(), &guard, cfg.large_file_bytes()) {
            if scanned.view.dup_bytes > 0 {
                lines.push(format!("{} duplikat di {name} siap dicek.", format_bytes(scanned.view.dup_bytes)));
            }
        }
    }
    let _ = s.save();

    if cfg.notify_after_auto && !lines.is_empty() {
        settings::notify("Bersih otomatis selesai", &lines.join(" "));
    }
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Buka MDN Cleaner", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("MDN Cleaner")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn main() {
    if std::env::args().any(|a| a == schedule::AUTO_CLEAN_ARG) {
        run_auto_clean();
        return;
    }
    let minimized = std::env::args().any(|a| a == settings::MINIMIZED_ARG);

    let shared = Shared { app: Mutex::new(AppState::load()), ..Default::default() };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(shared)
        .manage(updater::UpdateState::default())
        .setup(move |app| {
            build_tray(app)?;
            if !minimized {
                show_main(app.handle());
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || junk_reminder(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            // With "Jalankan saat Windows menyala" on, closing keeps the app in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let shared = window.state::<Shared>();
                if lock(&shared.app).settings.start_with_windows {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_selection,
            set_scan_options,
            set_drive_included,
            add_skipped_folder,
            remove_skipped_folder,
            set_auto_clean,
            set_settings,
            pick_cloud_folder,
            reset_settings,
            confirm,
            check_updates,
            download_update,
            cancel_update,
            install_update,
            install_update_on_exit,
            scan,
            clean,
            drives,
            last_analysis,
            analyze,
            analysis_progress,
            cloud_status,
            cloud_scan,
            cloud_delete,
            restore,
            open_cloud_trash,
            reveal,
            open_external
        ])
        .build(tauri::generate_context!())
        .expect("MDN Cleaner gagal dijalankan")
        .run(|app, event| {
            // "Nanti" on a downloaded update: install it as the app closes.
            if let RunEvent::Exit = event {
                updater::install_if_postponed(app);
            }
        });
}
