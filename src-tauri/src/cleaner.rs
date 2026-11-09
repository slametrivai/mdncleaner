//! Scanning and cleaning of junk categories.
//!
//! Everything except the Recycle Bin itself is moved to the Recycle Bin rather
//! than deleted, so a clean can be undone from "Riwayat" for 7 days. Entries
//! that are in use (locked by a running program) or that need admin rights
//! are skipped.

use crate::dupes::{self, DupGroup};
use crate::paths::{env_path, user_dir, Guard};
use crate::state::ScanOptions;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemId {
    WindowsTemp,
    RecycleBin,
    ErrorLogs,
    ThumbnailCache,
    BrowserCache,
    BrowserCookies,
    SpotifyCache,
    DiscordCache,
    Duplicates,
    OldDownloads,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    Windows,
    Browser,
    Apps,
    Files,
}

struct ItemDef {
    id: ItemId,
    group: Group,
    label: &'static str,
    default_checked: bool,
}

/// Display order follows the design: Windows, Browser, Aplikasi, then the
/// personal-file checks from "PC ini".
const ITEMS: &[ItemDef] = &[
    ItemDef { id: ItemId::WindowsTemp, group: Group::Windows, label: "File sementara Windows", default_checked: true },
    ItemDef { id: ItemId::RecycleBin, group: Group::Windows, label: "Recycle Bin", default_checked: true },
    ItemDef { id: ItemId::ErrorLogs, group: Group::Windows, label: "Log dan dump error", default_checked: true },
    ItemDef { id: ItemId::ThumbnailCache, group: Group::Windows, label: "Cache thumbnail", default_checked: true },
    ItemDef { id: ItemId::BrowserCache, group: Group::Browser, label: "Cache Chrome dan Edge", default_checked: true },
    // Off by default: removing cookies signs the user out of every website.
    ItemDef { id: ItemId::BrowserCookies, group: Group::Browser, label: "Cookie browser", default_checked: false },
    ItemDef { id: ItemId::SpotifyCache, group: Group::Apps, label: "Cache Spotify", default_checked: true },
    ItemDef { id: ItemId::DiscordCache, group: Group::Apps, label: "Cache Discord", default_checked: true },
    ItemDef { id: ItemId::Duplicates, group: Group::Files, label: "File duplikat", default_checked: true },
    ItemDef { id: ItemId::OldDownloads, group: Group::Files, label: "Unduhan lama", default_checked: true },
];

// Temp entries touched recently may belong to a running installer; the
// user's "Lewati file yang baru dipakai" window (24 hours by default) covers them.
const OLD_DOWNLOAD_AGE: Duration = Duration::from_secs(90 * 24 * 60 * 60);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanItem {
    id: ItemId,
    group: Group,
    label: &'static str,
    bytes: u64,
    files: u64,
    default_checked: bool,
    note: Option<String>,
}

/// One file or folder moved to the Recycle Bin, remembered for "Pulihkan".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Moved {
    pub path: String,
    pub bytes: u64,
    #[serde(default)]
    pub files: u64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanReport {
    pub bytes: u64,
    pub files: u64,
    pub skipped: u64,
    #[serde(skip)]
    pub moved: Vec<Moved>,
}

/// Recycle Bin items that belong to cleans that can still be undone.
#[derive(Debug, Default)]
pub struct BinGuard {
    pub paths: HashSet<String>,
    pub bytes: u64,
    pub files: u64,
}

/// Everything a scan or clean needs to know about the user's choices.
pub struct ScanEnv {
    pub options: ScanOptions,
    pub guard: Guard,
    /// Off when the system drive's "Ikut discan" is off.
    pub system_drive: bool,
    /// Where to look for duplicates.
    pub dup_roots: Vec<PathBuf>,
    pub bin: BinGuard,
    /// "Lewati file yang baru dipakai".
    pub recent: Duration,
    /// "Kirim ke Recycle Bin dulu". Off means files are deleted for good.
    pub use_recycle_bin: bool,
}

enum Target {
    /// Every entry directly inside the folder.
    Contents { dir: PathBuf, min_age: Option<Duration> },
    /// Files directly inside the folder whose name passes the filter.
    Matching { dir: PathBuf, filter: fn(&str) -> bool },
    /// One file.
    File(PathBuf),
}

struct Entry {
    path: PathBuf,
    bytes: u64,
    files: u64,
}

fn contents(dir: PathBuf) -> Target {
    Target::Contents { dir, min_age: None }
}

fn is_thumb_cache(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    (name.starts_with("thumbcache_") || name.starts_with("iconcache_")) && name.ends_with(".db")
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect()
}

fn dir_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Chromium profile folders: Default, Profile 1, Profile 2, Guest Profile …
fn chromium_profiles(user_data: &Path) -> Vec<PathBuf> {
    subdirs(user_data)
        .into_iter()
        .filter(|p| {
            let name = dir_name(p);
            name == "Default" || name.starts_with("Profile ") || name == "Guest Profile"
        })
        .collect()
}

fn enabled(id: ItemId, env: &ScanEnv) -> bool {
    let o = &env.options;
    match id {
        ItemId::RecycleBin => env.system_drive && o.recycle_bin,
        ItemId::Duplicates => o.duplicates && !env.dup_roots.is_empty(),
        ItemId::OldDownloads => env.system_drive && o.old_downloads,
        _ => env.system_drive && o.caches,
    }
}

/// The folders an item cleans. `None` means the item does not apply to this
/// PC (for example the app is not installed), so it is left out of the scan.
fn targets(id: ItemId) -> Option<Vec<Target>> {
    let local = env_path("LOCALAPPDATA");
    let roaming = env_path("APPDATA");
    let windir = env_path("SystemRoot").or_else(|| env_path("windir"));
    let program_data = env_path("ProgramData");

    match id {
        ItemId::RecycleBin | ItemId::Duplicates => Some(Vec::new()),
        ItemId::WindowsTemp => {
            let mut t = Vec::new();
            // %TEMP% is only trusted when it really is a temp folder, so a
            // misconfigured variable can never point the cleaner at real data.
            if let Some(tmp) = env_path("TEMP").filter(|p| {
                matches!(dir_name(p).to_ascii_lowercase().as_str(), "temp" | "tmp")
            }) {
                t.push(Target::Contents { dir: tmp, min_age: None });
            }
            if let Some(w) = &windir {
                t.push(Target::Contents { dir: w.join("Temp"), min_age: None });
            }
            Some(t)
        }
        ItemId::ErrorLogs => {
            let mut t = Vec::new();
            if let Some(l) = &local {
                t.push(contents(l.join("CrashDumps")));
                t.push(contents(l.join(r"Microsoft\Windows\WER\ReportArchive")));
                t.push(contents(l.join(r"Microsoft\Windows\WER\ReportQueue")));
            }
            if let Some(pd) = &program_data {
                t.push(contents(pd.join(r"Microsoft\Windows\WER\ReportArchive")));
                t.push(contents(pd.join(r"Microsoft\Windows\WER\ReportQueue")));
            }
            if let Some(w) = &windir {
                t.push(contents(w.join("Minidump")));
                t.push(Target::File(w.join("MEMORY.DMP")));
            }
            Some(t)
        }
        ItemId::ThumbnailCache => local.map(|l| {
            vec![Target::Matching { dir: l.join(r"Microsoft\Windows\Explorer"), filter: is_thumb_cache }]
        }),
        ItemId::BrowserCache | ItemId::BrowserCookies => {
            let local = local?;
            let roots: Vec<PathBuf> = [r"Google\Chrome\User Data", r"Microsoft\Edge\User Data"]
                .iter()
                .map(|p| local.join(p))
                .filter(|p| p.is_dir())
                .collect();
            if roots.is_empty() {
                return None;
            }
            let mut t = Vec::new();
            for profile in roots.iter().flat_map(|r| chromium_profiles(r)) {
                if id == ItemId::BrowserCache {
                    for sub in [r"Cache\Cache_Data", "Code Cache", "GPUCache"] {
                        t.push(contents(profile.join(sub)));
                    }
                } else {
                    for file in [r"Network\Cookies", r"Network\Cookies-journal"] {
                        t.push(Target::File(profile.join(file)));
                    }
                }
            }
            Some(t)
        }
        ItemId::SpotifyCache => {
            let local = local?;
            let mut t = Vec::new();
            let mut installed = false;
            let desktop = local.join("Spotify");
            if desktop.is_dir() {
                installed = true;
                t.push(contents(desktop.join("Storage")));
                t.push(contents(desktop.join("Data")));
            }
            // Microsoft Store build.
            for pkg in subdirs(&local.join("Packages"))
                .into_iter()
                .filter(|p| dir_name(p).starts_with("SpotifyAB.SpotifyMusic_"))
            {
                installed = true;
                t.push(contents(pkg.join(r"LocalCache\Spotify\Data")));
            }
            installed.then_some(t)
        }
        ItemId::DiscordCache => {
            let discord = roaming?.join("discord");
            if !discord.is_dir() {
                return None;
            }
            Some(
                [r"Cache\Cache_Data", "Code Cache", "GPUCache"]
                    .into_iter()
                    .map(|sub| contents(discord.join(sub)))
                    .collect(),
            )
        }
        ItemId::OldDownloads => {
            user_dir("Downloads").map(|d| vec![Target::Contents { dir: d, min_age: Some(OLD_DOWNLOAD_AGE) }])
        }
    }
}

/// Refuses drive roots and other shallow paths, whatever the environment says.
fn is_safe_dir(dir: &Path) -> bool {
    dir.is_absolute() && dir.components().count() >= 3
}

fn is_recent(meta: &fs::Metadata, min_age: Duration) -> bool {
    meta.modified()
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .map(|age| age < min_age)
        .unwrap_or(true)
}

/// Size and file count of a file or folder, without following links.
fn measure(path: &Path) -> Option<Entry> {
    let meta = fs::symlink_metadata(path).ok()?;
    let (bytes, files) = if meta.file_type().is_symlink() {
        (0, 1)
    } else if meta.is_dir() {
        let (mut bytes, mut files) = (0, 0);
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_symlink() {
                    files += 1;
                } else if ft.is_dir() {
                    stack.push(entry.path());
                } else if let Ok(m) = entry.metadata() {
                    bytes += m.len();
                    files += 1;
                }
            }
        }
        (bytes, files)
    } else {
        (meta.len(), 1)
    };
    Some(Entry { path: path.to_path_buf(), bytes, files })
}

/// Entries a target would clean. Folder contents touched within the user's
/// "Lewati file yang baru dipakai" window are left alone.
fn collect(target: &Target, env: &ScanEnv) -> Vec<Entry> {
    let entries: Vec<Entry> = match target {
        Target::Contents { dir, min_age } => {
            if !is_safe_dir(dir) {
                return Vec::new();
            }
            let window = min_age.unwrap_or_default().max(env.recent);
            fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| window.is_zero() || e.metadata().map(|m| !is_recent(&m, window)).unwrap_or(false))
                .filter_map(|e| measure(&e.path()))
                .collect()
        }
        Target::Matching { dir, filter } => {
            if !is_safe_dir(dir) {
                return Vec::new();
            }
            fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter(|e| filter(&e.file_name().to_string_lossy()))
                .filter_map(|e| measure(&e.path()))
                .collect()
        }
        Target::File(path) => measure(path).into_iter().collect(),
    };
    entries.into_iter().filter(|e| !env.guard.is_skipped(&e.path)).collect()
}

fn recycle_bin_size(env: &ScanEnv) -> (u64, u64) {
    let (bytes, files) = recycle_bin::query();
    (bytes.saturating_sub(env.bin.bytes), files.saturating_sub(env.bin.files))
}

fn scan_item(def: &ItemDef, env: &ScanEnv, dups: &[DupGroup]) -> Option<ScanItem> {
    if !enabled(def.id, env) {
        return None;
    }
    let mut note = None;
    let (bytes, files) = match def.id {
        ItemId::RecycleBin => recycle_bin_size(env),
        ItemId::Duplicates => {
            note = Some(format!("{} grup", dups.len()));
            (dups.iter().map(DupGroup::reclaim).sum(), dups.iter().map(|g| g.files.len() as u64 - 1).sum())
        }
        id => targets(id)?
            .iter()
            .flat_map(|t| collect(t, env))
            .fold((0, 0), |(b, f), e| (b + e.bytes, f + e.files)),
    };
    Some(ScanItem {
        id: def.id,
        group: def.group,
        label: def.label,
        bytes,
        files,
        default_checked: def.default_checked,
        note,
    })
}

pub fn find_duplicates(env: &ScanEnv) -> Vec<DupGroup> {
    if !enabled(ItemId::Duplicates, env) {
        return Vec::new();
    }
    let candidates = env.dup_roots.iter().flat_map(|root| dupes::collect(root, &env.guard)).collect();
    dupes::find(candidates)
}

/// Measures every category that applies, one thread per category. Also
/// returns the duplicate groups, which a later clean needs.
pub fn scan(env: &ScanEnv) -> (Vec<ScanItem>, Vec<DupGroup>) {
    let dups = find_duplicates(env);
    let items = std::thread::scope(|s| {
        let handles: Vec<_> = ITEMS.iter().map(|def| s.spawn(|| scan_item(def, env, &dups))).collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    (items, dups)
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn delete_permanently(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            let _ = fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = fs::remove_file(path);
        }
        Err(_) => {}
    }
}

/// Moves entries to the Recycle Bin in batches (or deletes them for good when
/// the user turned the Recycle Bin off). A batch that fails, usually because
/// one entry is locked, is retried one entry at a time. An entry counts as
/// cleaned only once it is really gone from disk.
fn recycle(entries: Vec<Entry>, use_bin: bool, report: &mut CleanReport) {
    for chunk in entries.chunks(64) {
        if use_bin {
            let paths: Vec<&PathBuf> = chunk.iter().map(|e| &e.path).collect();
            if trash::delete_all(&paths).is_err() {
                for e in chunk.iter().filter(|e| exists(&e.path)) {
                    let _ = trash::delete(&e.path);
                }
            }
        } else {
            for e in chunk {
                delete_permanently(&e.path);
            }
        }
        for e in chunk {
            if exists(&e.path) {
                report.skipped += 1;
                continue;
            }
            report.bytes += e.bytes;
            report.files += e.files;
            if use_bin {
                report.moved.push(Moved { path: e.path.to_string_lossy().into_owned(), bytes: e.bytes, files: e.files });
            }
        }
    }
}

/// Empties the Recycle Bin, except what recent cleans put there.
fn empty_recycle_bin(env: &ScanEnv, report: &mut CleanReport) {
    let (bytes_before, files_before) = recycle_bin::query();
    if files_before == 0 {
        return;
    }
    if env.bin.paths.is_empty() {
        recycle_bin::empty();
    } else if let Ok(items) = trash::os_limited::list() {
        let doomed: Vec<_> = items
            .into_iter()
            .filter(|item| !env.bin.paths.contains(&crate::paths::lower(&item.original_path())))
            .collect();
        if !doomed.is_empty() {
            let _ = trash::os_limited::purge_all(doomed);
        }
    }
    let (bytes_after, files_after) = recycle_bin::query();
    report.bytes += bytes_before.saturating_sub(bytes_after);
    report.files += files_before.saturating_sub(files_after);
}

pub fn clean(ids: &[ItemId], env: &ScanEnv, dups: &[DupGroup]) -> CleanReport {
    let mut report = CleanReport::default();
    let wanted: Vec<ItemId> = ids.iter().copied().filter(|id| enabled(*id, env)).collect();

    // Empty the bin first, so the files moved there below stay recoverable.
    if wanted.contains(&ItemId::RecycleBin) {
        empty_recycle_bin(env, &mut report);
    }

    for id in &wanted {
        match id {
            ItemId::RecycleBin => {}
            ItemId::Duplicates => {
                let copies = dups
                    .iter()
                    .flat_map(|g| dupes::extra_copies(g))
                    .filter(|c| c.modified.elapsed().map(|age| age >= env.recent).unwrap_or(false))
                    .filter(|c| env.guard.allows_user_file(&c.path))
                    .map(|c| Entry { path: c.path.clone(), bytes: c.size, files: 1 })
                    .collect();
                recycle(copies, env.use_recycle_bin, &mut report);
            }
            _ => {
                for target in targets(*id).unwrap_or_default() {
                    recycle(collect(&target, env), env.use_recycle_bin, &mut report);
                }
            }
        }
    }
    report
}

/// Junk the user would clean right now: the ticked items' sizes.
pub fn checked_bytes(items: &[ScanItem], selection: &HashMap<ItemId, bool>) -> u64 {
    items
        .iter()
        .filter(|item| selection.get(&item.id).copied().unwrap_or(item.default_checked))
        .map(|item| item.bytes)
        .sum()
}

/// The items to clean when nobody is at the screen: the user's saved ticks,
/// falling back to each item's default. Duplicates are never removed
/// unattended.
pub fn saved_selection(selection: &HashMap<ItemId, bool>) -> Vec<ItemId> {
    ITEMS
        .iter()
        .filter(|def| def.id != ItemId::Duplicates)
        .filter(|def| selection.get(&def.id).copied().unwrap_or(def.default_checked))
        .map(|def| def.id)
        .collect()
}

#[cfg(windows)]
mod recycle_bin {
    use windows_sys::Win32::UI::Shell::{
        SHEmptyRecycleBinW, SHQueryRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI,
        SHERB_NOSOUND, SHQUERYRBINFO,
    };

    /// (bytes, items) across every drive's Recycle Bin.
    pub fn query() -> (u64, u64) {
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            i64Size: 0,
            i64NumItems: 0,
        };
        let hr = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) };
        if hr < 0 {
            return (0, 0);
        }
        (info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64)
    }

    pub fn empty() -> bool {
        let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
        let hr = unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), flags) };
        hr >= 0
    }
}

#[cfg(not(windows))]
mod recycle_bin {
    pub fn query() -> (u64, u64) {
        (0, 0)
    }
    pub fn empty() -> bool {
        false
    }
}
