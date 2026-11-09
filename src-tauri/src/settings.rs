//! "Pengaturan": the user's preferences, and the Windows features some of
//! them switch on (start with Windows, notifications, power state).

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Schedule {
    Daily,
    #[default]
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutoLocations {
    pub pc: bool,
    pub google_drive: bool,
    pub icloud_drive: bool,
}

impl Default for AutoLocations {
    fn default() -> Self {
        Self { pc: true, google_drive: true, icloud_drive: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // Umum
    pub start_with_windows: bool,
    pub language: String,
    pub theme: Theme,
    // Pembersihan
    pub confirm_before_delete: bool,
    pub use_recycle_bin: bool,
    pub skip_recent_hours: u64,
    pub large_file_mb: u64,
    // Bersih otomatis (on/off lives in AppState::auto_clean)
    pub schedule: Schedule,
    pub auto_locations: AutoLocations,
    pub only_on_ac: bool,
    // Lokasi cloud
    pub google_drive_root: Option<String>,
    pub icloud_drive_root: Option<String>,
    pub check_cloud_on_start: bool,
    // Notifikasi
    pub remind_junk: bool,
    pub notify_after_auto: bool,
    // Pembaruan
    pub auto_update_check: bool,
    /// When the last "sampah menumpuk" reminder was shown.
    pub last_reminded_ms: u64,
    /// When "Cek pembaruan" last reached GitHub.
    pub last_update_check_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            language: "id".into(),
            theme: Theme::Light,
            confirm_before_delete: true,
            use_recycle_bin: true,
            skip_recent_hours: 24,
            large_file_mb: 500,
            schedule: Schedule::Weekly,
            auto_locations: AutoLocations::default(),
            only_on_ac: true,
            google_drive_root: None,
            icloud_drive_root: None,
            check_cloud_on_start: true,
            remind_junk: true,
            notify_after_auto: false,
            auto_update_check: true,
            last_reminded_ms: 0,
            last_update_check_ms: 0,
        }
    }
}

/// Reminder threshold for "Ingatkan kalau sampah menumpuk".
pub const REMIND_BYTES: u64 = 5 * 1024 * 1024 * 1024;

impl Settings {
    pub fn recent_window(&self) -> Duration {
        Duration::from_secs(self.skip_recent_hours.max(1) * 60 * 60)
    }

    pub fn large_file_bytes(&self) -> u64 {
        self.large_file_mb.max(1) * 1024 * 1024
    }
}

pub const MINIMIZED_ARG: &str = "--minimized";

/// Runs a Windows command-line tool without flashing a console window.
#[cfg(windows)]
pub(crate) fn run_windows_tool(program: &str, args: &[&str]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("{program} tidak bisa dijalankan: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// "Jalankan saat Windows menyala": a value under the user's Run key that
/// starts the app minimised to the tray.
#[cfg(windows)]
pub fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "MDN Cleaner";
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = format!("\"{}\" {MINIMIZED_ARG}", exe.display());
        run_windows_tool("reg", &["add", KEY, "/v", VALUE, "/t", "REG_SZ", "/d", &command, "/f"])
    } else {
        // Removing a value that is not there is fine.
        let _ = run_windows_tool("reg", &["delete", KEY, "/v", VALUE, "/f"]);
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn set_start_with_windows(_enabled: bool) -> Result<(), String> {
    Err("Hanya tersedia di Windows".into())
}

/// A Windows toast. Installed builds show as MDN Cleaner; dev builds borrow
/// PowerShell's identity, which Windows always accepts.
#[cfg(windows)]
pub fn notify(title: &str, body: &str) {
    use tauri_winrt_notification::Toast;
    let app_id = if cfg!(debug_assertions) { Toast::POWERSHELL_APP_ID } else { "com.mdn.cleaner" };
    let _ = Toast::new(app_id).title(title).text1(body).show();
}

#[cfg(not(windows))]
pub fn notify(_title: &str, _body: &str) {}

/// False only when the PC is known to run on battery.
#[cfg(windows)]
pub fn on_ac_power() -> bool {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return true;
    }
    status.ACLineStatus != 0
}

#[cfg(not(windows))]
pub fn on_ac_power() -> bool {
    true
}

/// Today's local date as (year, month, day).
#[cfg(windows)]
pub fn local_date() -> (u16, u16, u16) {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    (t.wYear, t.wMonth, t.wDay)
}

#[cfg(not(windows))]
pub fn local_date() -> (u16, u16, u16) {
    (2026, 1, 1)
}
