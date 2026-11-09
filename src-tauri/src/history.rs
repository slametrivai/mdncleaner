//! "Riwayat": every clean, what it freed and how long it can be undone.
//!
//! Files from the PC go to the Recycle Bin. MDN keeps them there for 7 days,
//! during which "Pulihkan" puts them back, and then deletes them for good.
//! Files from Google Drive or iCloud go to that service's own trash, which
//! keeps them for 30 days.

use crate::cleaner::{BinGuard, CleanReport, Moved};
use crate::cloud::Location;
use crate::paths::lower;
use crate::state::{data_dir, now_ms, read_json, write_json, AppState};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
pub const RECYCLE_DAYS: u64 = 7;
const KEEP_DAYS: u64 = 90;
const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Manual,
    Auto,
    Duplicates,
    LargeFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub at_ms: u64,
    pub kind: Kind,
    pub locations: Vec<Location>,
    pub bytes: u64,
    pub files: u64,
    /// Bytes moved to the Recycle Bin by this clean.
    pub recycle_bytes: u64,
    /// Bytes sent to a cloud trash by this clean.
    pub cloud_bytes: u64,
    pub cloud: Option<Location>,
    #[serde(default)]
    pub purged: bool,
    #[serde(default)]
    pub restored: bool,
}

impl Entry {
    fn recycle_valid(&self, now: u64) -> bool {
        self.recycle_bytes > 0 && !self.purged && !self.restored && now < self.at_ms + RECYCLE_DAYS * DAY_MS
    }
}

fn paths_file(id: &str) -> Option<PathBuf> {
    data_dir().map(|d| d.join("history").join(format!("{id}.json")))
}

fn load_moved(id: &str) -> Vec<Moved> {
    paths_file(id).and_then(|p| read_json(&p)).unwrap_or_default()
}

pub fn record(state: &mut AppState, kind: Kind, locations: Vec<Location>, report: &CleanReport, cloud: Option<Location>) {
    if report.bytes == 0 && report.files == 0 {
        return;
    }
    let at_ms = now_ms();
    let id = at_ms.to_string();
    if !report.moved.is_empty() {
        if let Some(path) = paths_file(&id) {
            let _ = write_json(&path, &report.moved);
        }
    }
    state.history.insert(
        0,
        Entry {
            id,
            at_ms,
            kind,
            locations,
            bytes: report.bytes,
            files: report.files,
            recycle_bytes: report.moved.iter().map(|m| m.bytes).sum(),
            cloud_bytes: if cloud.is_some() { report.bytes } else { 0 },
            cloud,
            purged: false,
            restored: false,
        },
    );

    // Forget old entries, together with their list of moved files.
    let cutoff = at_ms.saturating_sub(KEEP_DAYS * DAY_MS);
    let mut kept = Vec::new();
    for (i, entry) in state.history.drain(..).enumerate() {
        if i < MAX_ENTRIES && entry.at_ms >= cutoff {
            kept.push(entry);
        } else if let Some(path) = paths_file(&entry.id) {
            let _ = std::fs::remove_file(path);
        }
    }
    state.history = kept;
}

/// Recycle Bin items that still belong to an undoable clean. Emptying the
/// Recycle Bin leaves these alone, so "Pulihkan" keeps working for 7 days.
pub fn protected_bin(state: &AppState) -> BinGuard {
    let now = now_ms();
    let mut guard = BinGuard::default();
    for entry in state.history.iter().filter(|e| e.recycle_valid(now)) {
        for moved in load_moved(&entry.id) {
            guard.bytes += moved.bytes;
            guard.files += moved.files;
            guard.paths.insert(lower(std::path::Path::new(&moved.path)));
        }
    }
    guard
}

/// Recycle Bin items that a clean put there.
fn bin_items(entry: &Entry, moved: &[Moved], items: &[trash::TrashItem]) -> Vec<trash::TrashItem> {
    let wanted: HashSet<String> = moved.iter().map(|m| lower(std::path::Path::new(&m.path))).collect();
    let since_s = (entry.at_ms / 1000) as i64 - 60;
    items
        .iter()
        .filter(|item| item.time_deleted >= since_s && wanted.contains(&lower(&item.original_path())))
        .cloned()
        .collect()
}

/// Permanently deletes what earlier cleans left in the Recycle Bin once
/// their 7 days are over. Returns whether anything changed.
pub fn purge_expired(state: &mut AppState) -> bool {
    let now = now_ms();
    let expired: Vec<usize> = state
        .history
        .iter()
        .enumerate()
        .filter(|(_, e)| e.recycle_bytes > 0 && !e.purged && !e.restored && !e.recycle_valid(now))
        .map(|(i, _)| i)
        .collect();
    if expired.is_empty() {
        return false;
    }
    let items = trash::os_limited::list().unwrap_or_default();
    for i in expired {
        let entry = &mut state.history[i];
        let doomed = bin_items(entry, &load_moved(&entry.id), &items);
        if !doomed.is_empty() {
            let _ = trash::os_limited::purge_all(doomed);
        }
        entry.purged = true;
    }
    true
}

/// "Pulihkan": puts a clean's files back where they were.
pub fn restore(state: &mut AppState, id: &str) -> Result<usize, String> {
    let now = now_ms();
    let entry = state.history.iter_mut().find(|e| e.id == id).ok_or("Riwayat tidak ditemukan")?;
    if !entry.recycle_valid(now) {
        return Err("Masa simpan di Recycle Bin sudah habis".into());
    }
    let items = trash::os_limited::list().map_err(|e| e.to_string())?;
    let found = bin_items(entry, &load_moved(&entry.id), &items);
    if found.is_empty() {
        entry.restored = true;
        return Err("File-nya sudah tidak ada di Recycle Bin".into());
    }
    let count = found.len();
    trash::os_limited::restore_all(found)
        .map_err(|e| format!("Sebagian file tidak bisa dipulihkan karena sudah ada file dengan nama yang sama ({e})"))?;
    entry.restored = true;
    Ok(count)
}
