//! What the app remembers between runs, stored as JSON in %APPDATA%.
//!
//! The path is built by hand instead of through Tauri's path resolver so the
//! headless `--auto-clean` run, which starts no Tauri app, reads the same file.

use crate::cleaner::ItemId;
use crate::history::Entry;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const IDENTIFIER: &str = "com.mdn.cleaner";

/// "Yang dicari di PC ini".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanOptions {
    pub caches: bool,
    pub recycle_bin: bool,
    pub duplicates: bool,
    pub large_files: bool,
    pub old_downloads: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self { caches: true, recycle_bin: true, duplicates: true, large_files: true, old_downloads: false }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppState {
    pub auto_clean: bool,
    pub selection: HashMap<ItemId, bool>,
    pub scan_options: ScanOptions,
    /// Drive letters ("D:") whose "Ikut discan" is off.
    pub excluded_drives: Vec<String>,
    /// "Folder yang dilewati".
    pub skipped_folders: Vec<String>,
    /// Newest first.
    pub history: Vec<Entry>,
    pub settings: Settings,
}

pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join(IDENTIFIER))
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Writes then renames, so a crash mid-write never leaves a broken file.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

impl AppState {
    fn path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("state.json"))
    }

    pub fn load() -> Self {
        Self::path().and_then(|p| read_json(&p)).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("Folder AppData tidak ditemukan")?;
        write_json(&path, self)
    }

    pub fn drive_included(&self, letter: &str) -> bool {
        !self.excluded_drives.iter().any(|d| d.eq_ignore_ascii_case(letter))
    }
}
