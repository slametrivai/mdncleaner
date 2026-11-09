//! Fixed drives and "Analisis disk": what fills a drive, by type and by
//! folder, plus big files that have not been used for a long time.

use crate::dupes::is_online_only;
use crate::paths::{env_path, is_under, lower, user_dir, Guard};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Files seen by the running analysis, for the progress line.
pub static FILES_SEEN: AtomicU64 = AtomicU64::new(0);


const RARELY_USED: Duration = Duration::from_secs(90 * 24 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    /// "C:"
    pub letter: String,
    pub label: String,
    pub total: u64,
    pub free: u64,
    /// None when the drive does not report it (USB enclosures, RAID).
    pub ssd: Option<bool>,
    pub system: bool,
}

pub fn system_drive() -> String {
    std::env::var("SystemDrive").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "C:".into()).to_uppercase()
}

pub fn root_of(letter: &str) -> PathBuf {
    PathBuf::from(format!("{letter}\\"))
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Ioctl::{
        PropertyStandardQuery, StorageDeviceSeekPenaltyProperty, DEVICE_SEEK_PENALTY_DESCRIPTOR,
        IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_PROPERTY_QUERY,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const DRIVE_FIXED: u32 = 3;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn letters() -> Vec<String> {
        let mask = unsafe { GetLogicalDrives() };
        (0..26u8).filter(|i| mask & (1 << i) != 0).map(|i| format!("{}:", (b'A' + i) as char)).collect()
    }

    pub fn is_fixed(letter: &str) -> bool {
        unsafe { GetDriveTypeW(wide(&format!("{letter}\\")).as_ptr()) == DRIVE_FIXED }
    }

    pub fn label(letter: &str) -> String {
        let root = wide(&format!("{letter}\\"));
        let mut buf = [0u16; 261];
        let ok = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                0,
            )
        };
        if ok == 0 {
            return String::new();
        }
        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    /// (total, free) in bytes.
    pub fn space(letter: &str) -> (u64, u64) {
        let root = wide(&format!("{letter}\\"));
        let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
        let ok = unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut available, &mut total, &mut free) };
        if ok == 0 {
            (0, 0)
        } else {
            (total, free)
        }
    }

    /// SSDs report no seek penalty. Needs no admin rights.
    pub fn is_ssd(letter: &str) -> Option<bool> {
        let device = wide(&format!(r"\\.\{letter}"));
        let handle = unsafe {
            CreateFileW(device.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut())
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let query = STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceSeekPenaltyProperty,
            QueryType: PropertyStandardQuery,
            AdditionalParameters: [0],
        };
        let mut out = DEVICE_SEEK_PENALTY_DESCRIPTOR { Version: 0, Size: 0, IncursSeekPenalty: 0 };
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &query as *const _ as *const c_void,
                std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                &mut out as *mut _ as *mut c_void,
                std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
                &mut returned,
                null_mut(),
            )
        };
        unsafe { CloseHandle(handle) };
        (ok != 0).then_some(out.IncursSeekPenalty == 0)
    }
}

#[cfg(not(windows))]
mod win {
    pub fn letters() -> Vec<String> {
        Vec::new()
    }
    pub fn is_fixed(_: &str) -> bool {
        false
    }
    pub fn label(_: &str) -> String {
        String::new()
    }
    pub fn space(_: &str) -> (u64, u64) {
        (0, 0)
    }
    pub fn is_ssd(_: &str) -> Option<bool> {
        None
    }
}

pub use win::letters as all_letters;

/// Fixed drives, system drive first. Google Drive's virtual drive is a cloud
/// location, not a disk, so it is left out.
pub fn drives() -> Vec<Drive> {
    let system = system_drive();
    let mut list: Vec<Drive> = win::letters()
        .into_iter()
        .filter(|l| win::is_fixed(l))
        .filter_map(|letter| {
            let label = win::label(&letter);
            if label.eq_ignore_ascii_case("Google Drive") {
                return None;
            }
            let (total, free) = win::space(&letter);
            let is_system = letter == system;
            let label = match label.trim() {
                "" if is_system => "Windows".to_string(),
                "" => "Disk lokal".to_string(),
                l => l.to_string(),
            };
            Some(Drive { ssd: win::is_ssd(&letter), system: is_system, letter, label, total, free })
        })
        .collect();
    list.sort_by_key(|d| (!d.system, d.letter.clone()));
    list
}

pub fn space(letter: &str) -> (u64, u64) {
    win::space(letter)
}

// ── Analysis ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeSize {
    pub kind: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSize {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    pub path: String,
    pub name: String,
    pub folder: String,
    pub bytes: u64,
    pub last_used_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub drive: String,
    pub at_ms: u64,
    pub total: u64,
    pub free: u64,
    pub files: u64,
    pub types: Vec<TypeSize>,
    pub folders: Vec<FolderSize>,
    pub large_files: Vec<LargeFile>,
}

const KINDS: [&str; 7] = ["game", "apps", "video", "photo", "docs", "system", "other"];
const GAME: usize = 0;
const APPS: usize = 1;
const VIDEO: usize = 2;
const PHOTO: usize = 3;
const DOCS: usize = 4;
const SYSTEM: usize = 5;
const OTHER: usize = 6;

const GAME_DIRS: &[&str] = &[
    "steamapps",
    "epic games",
    "riot games",
    "xboxgames",
    "gog games",
    "ea games",
    "ubisoft game launcher",
];
const VIDEO_EXT: &[&str] = &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "3gp", "ts", "vob"];
const PHOTO_EXT: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "heic", "heif", "webp", "tif", "tiff", "raw", "cr2", "cr3", "nef", "arw", "dng", "psd",
];
const DOCS_EXT: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "odt", "ods", "odp", "csv", "md", "epub",
];
const APP_EXT: &[&str] = &["exe", "dll", "msi", "sys", "pak", "dat", "bin"];

#[derive(Clone, Copy, PartialEq)]
enum Zone {
    None,
    System,
    Game,
    Apps,
}

struct Ctx {
    windows: Option<PathBuf>,
    apps_roots: Vec<PathBuf>,
    guard: Guard,
    now: SystemTime,
    /// "Batas file besar".
    large_file: u64,
}

#[derive(Default)]
struct Acc {
    kinds: [u64; 7],
    bytes: u64,
    files: u64,
    dirs: Vec<(PathBuf, u64)>,
    large: Vec<LargeFile>,
}

fn merge(mut a: Acc, b: Acc) -> Acc {
    for i in 0..KINDS.len() {
        a.kinds[i] += b.kinds[i];
    }
    a.bytes += b.bytes;
    a.files += b.files;
    a.dirs.extend(b.dirs);
    a.large.extend(b.large);
    a
}

fn child_zone(ctx: &Ctx, parent: Zone, path: &Path, name: &str) -> Zone {
    match parent {
        Zone::System | Zone::Game => parent,
        _ if GAME_DIRS.contains(&name) => Zone::Game,
        Zone::Apps => Zone::Apps,
        Zone::None => {
            if ctx.windows.as_ref().is_some_and(|w| is_under(path, w))
                || name.starts_with('$')
                || name == "system volume information"
                || name == "recovery"
            {
                Zone::System
            } else if ctx.apps_roots.iter().any(|r| is_under(path, r)) {
                Zone::Apps
            } else {
                Zone::None
            }
        }
    }
}

fn kind_of(zone: Zone, depth: usize, name: &str) -> usize {
    match zone {
        Zone::System => return SYSTEM,
        Zone::Game => return GAME,
        _ => {}
    }
    if depth == 0 && matches!(name, "pagefile.sys" | "hiberfil.sys" | "swapfile.sys") {
        return SYSTEM;
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if VIDEO_EXT.contains(&ext) {
        VIDEO
    } else if PHOTO_EXT.contains(&ext) {
        PHOTO
    } else if DOCS_EXT.contains(&ext) {
        DOCS
    } else if zone == Zone::Apps || APP_EXT.contains(&ext) {
        APPS
    } else {
        OTHER
    }
}

fn ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Opening or saving a file both count as using it.
fn last_used(meta: &fs::Metadata) -> SystemTime {
    let modified = meta.modified().unwrap_or(UNIX_EPOCH);
    let accessed = meta.accessed().unwrap_or(UNIX_EPOCH);
    modified.max(accessed)
}

fn walk(ctx: &Ctx, dir: &Path, depth: usize, zone: Zone) -> Acc {
    let mut acc = Acc::default();
    let mut subdirs = Vec::new();
    let Ok(read) = fs::read_dir(dir) else { return acc };
    for entry in read.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if ft.is_dir() {
            let path = entry.path();
            let z = child_zone(ctx, zone, &path, &name);
            subdirs.push((path, z));
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        // Placeholders of cloud files take no space on this disk.
        if is_online_only(&meta) {
            continue;
        }
        let size = meta.len();
        acc.files += 1;
        acc.bytes += size;
        acc.kinds[kind_of(zone, depth, &name)] += size;

        if size >= ctx.large_file && zone == Zone::None {
            let path = entry.path();
            let used = last_used(&meta);
            let idle = ctx.now.duration_since(used).unwrap_or_default();
            if idle >= RARELY_USED && ctx.guard.allows_user_file(&path) {
                acc.large.push(LargeFile {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    folder: dir.to_string_lossy().into_owned(),
                    path: path.to_string_lossy().into_owned(),
                    bytes: size,
                    last_used_ms: ms(used),
                });
            }
        }
    }
    FILES_SEEN.fetch_add(acc.files, Ordering::Relaxed);

    let nested = subdirs
        .into_par_iter()
        .map(|(path, z)| {
            let mut sub = walk(ctx, &path, depth + 1, z);
            // Folder sizes are kept only near the top, where they mean something.
            if depth + 1 <= 4 {
                sub.dirs.push((path, sub.bytes));
            }
            sub
        })
        .reduce(Acc::default, merge);
    merge(acc, nested)
}

/// Picks the folders that best explain where the space went: never a plain
/// container like C:\Users\<name>, and when one child holds most of a
/// folder, that child instead (Steam → steamapps).
fn pick_folders(dirs: &[(PathBuf, u64)], root: &Path, count: usize) -> Vec<FolderSize> {
    let root_depth = root.components().count();
    let depth = |p: &Path| p.components().count().saturating_sub(root_depth);
    let users = root.join("Users");
    let windows = root.join("Windows");

    let mut children: HashMap<String, Vec<(PathBuf, u64)>> = HashMap::new();
    for (path, size) in dirs {
        if let Some(parent) = path.parent() {
            children.entry(lower(parent)).or_default().push((path.clone(), *size));
        }
    }

    let is_container = |p: &Path| {
        let d = depth(p);
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        (d == 1 && name == "users") || (d == 2 && is_under(p, &users)) || (d == 3 && is_under(p, &users) && name == "appdata")
    };
    let is_excluded = |p: &Path| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        is_under(p, &windows) || (depth(p) == 1 && (name.starts_with('$') || name == "system volume information" || name == "recovery"))
    };
    let refine = |mut path: PathBuf, mut size: u64| {
        while depth(&path) < 3 {
            let biggest = children.get(&lower(&path)).and_then(|c| c.iter().max_by_key(|(_, s)| *s));
            match biggest {
                Some((child, child_size)) if *child_size > 0 && *child_size as f64 >= size as f64 * 0.6 => {
                    path = child.clone();
                    size = *child_size;
                }
                _ => break,
            }
        }
        (path, size)
    };

    let mut candidates: Vec<&(PathBuf, u64)> = dirs
        .iter()
        .filter(|(p, s)| *s > 0 && depth(p) >= 1 && !is_container(p) && !is_excluded(p))
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    let mut picked: Vec<(PathBuf, u64)> = Vec::new();
    for (path, size) in candidates {
        let (path, size) = refine(path.clone(), *size);
        if picked.iter().any(|(p, _)| is_under(p, &path) || is_under(&path, p)) {
            continue;
        }
        picked.push((path, size));
        if picked.len() == count {
            break;
        }
    }
    picked.sort_by(|a, b| b.1.cmp(&a.1));
    picked.into_iter().map(|(p, s)| FolderSize { path: p.to_string_lossy().into_owned(), bytes: s }).collect()
}

pub fn analyze(letter: &str, guard: Guard, large_file: u64) -> Analysis {
    FILES_SEEN.store(0, Ordering::Relaxed);
    let root = root_of(letter);
    let (total, free) = space(letter);
    let apps_roots = ["ProgramFiles", "ProgramFiles(x86)", "ProgramData"]
        .iter()
        .filter_map(|k| env_path(k))
        .chain(user_dir("AppData"))
        .collect();
    let ctx = Ctx { windows: env_path("SystemRoot"), apps_roots, guard, now: SystemTime::now(), large_file };
    let mut acc = walk(&ctx, &root, 0, Zone::None);

    // Space the walk could not read (restore points, locked system files)
    // belongs to Windows.
    let used = total.saturating_sub(free);
    acc.kinds[SYSTEM] += used.saturating_sub(acc.bytes);

    let mut types: Vec<TypeSize> =
        KINDS.iter().zip(acc.kinds).map(|(k, b)| TypeSize { kind: (*k).to_string(), bytes: b }).collect();
    types.sort_by(|a, b| b.bytes.cmp(&a.bytes));

    acc.large.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    acc.large.truncate(5);

    Analysis {
        drive: letter.to_string(),
        at_ms: ms(SystemTime::now()),
        total,
        free,
        files: acc.files,
        types,
        folders: pick_folders(&acc.dirs, &root, 6),
        large_files: acc.large,
    }
}
