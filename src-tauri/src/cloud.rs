//! Google Drive (through Google Drive for desktop) and iCloud Drive (through
//! iCloud for Windows). MDN only reads the synced folder on this PC; it never
//! asks for a login. Deleting a file there sends it to that service's trash.

use crate::cleaner::CleanReport;
use crate::dupes::{self, Candidate, DupGroup};
use crate::paths::{env_path, Guard};
use crate::state::now_ms;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Location {
    Pc,
    GoogleDrive,
    IcloudDrive,
}

impl Location {
    pub fn trash_url(self) -> &'static str {
        match self {
            Location::GoogleDrive => "https://drive.google.com/drive/trash",
            Location::IcloudDrive => "https://www.icloud.com/iclouddrive/",
            Location::Pc => "",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStatus {
    pub location: Location,
    pub connected: bool,
    /// The app is installed even though no synced folder was found yet.
    pub installed: bool,
    pub root: Option<String>,
    /// The root was picked by hand rather than detected.
    pub manual: bool,
    /// Where MDN looked, for the "Sudah dicek di …" line.
    pub checked: Vec<String>,
}

fn candidates(location: Location) -> Vec<PathBuf> {
    let profile = env_path("USERPROFILE");
    match location {
        Location::GoogleDrive => {
            let mut list: Vec<PathBuf> = crate::disk::all_letters()
                .into_iter()
                .flat_map(|l| ["My Drive", "Drive Saya"].map(|name| PathBuf::from(format!("{l}\\{name}"))))
                .collect();
            if let Some(p) = &profile {
                list.push(p.join("My Drive"));
                list.push(p.join("Google Drive"));
            }
            list
        }
        Location::IcloudDrive => {
            profile.map(|p| vec![p.join("iCloudDrive"), p.join("iCloud Drive")]).unwrap_or_default()
        }
        Location::Pc => Vec::new(),
    }
}

fn installed(location: Location) -> bool {
    let local = env_path("LOCALAPPDATA");
    match location {
        Location::GoogleDrive => {
            local.as_ref().is_some_and(|l| l.join(r"Google\DriveFS").is_dir())
                || env_path("ProgramFiles").is_some_and(|p| p.join(r"Google\Drive File Stream").is_dir())
        }
        Location::IcloudDrive => local.as_ref().is_some_and(|l| {
            fs::read_dir(l.join("Packages"))
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("AppleInc.iCloud"))
        }),
        Location::Pc => true,
    }
}

/// Where the synced folder is. A folder the user picked under "Ubah folder"
/// wins over automatic detection.
pub fn status(location: Location, chosen: Option<&str>) -> CloudStatus {
    let list = candidates(location);
    let manual = chosen.map(PathBuf::from).filter(|p| p.is_dir());
    let root = manual.clone().or_else(|| list.iter().find(|p| p.is_dir()).cloned());
    let checked = match location {
        // Only the folder people know; drive letters are implied.
        Location::IcloudDrive => list.iter().take(1).map(|p| p.to_string_lossy().into_owned()).collect(),
        _ => Vec::new(),
    };
    CloudStatus {
        location,
        connected: root.is_some(),
        installed: installed(location),
        root: root.map(|r| r.to_string_lossy().into_owned()),
        manual: manual.is_some(),
        checked,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupView {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub reclaim: u64,
    pub copies: usize,
    pub folders: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub name: String,
    pub folder: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudScan {
    pub location: Location,
    pub root: String,
    pub at_ms: u64,
    pub total_bytes: u64,
    pub total_files: u64,
    pub local_bytes: u64,
    pub local_files: u64,
    pub online_files: u64,
    pub dup_groups: usize,
    pub dup_bytes: u64,
    pub large_bytes: u64,
    pub duplicates: Vec<DupView>,
    pub large: Vec<FileView>,
}

/// A scan plus what deleting needs: the exact groups and files it showed.
pub struct Scanned {
    pub view: CloudScan,
    groups: Vec<DupGroup>,
    large: HashSet<PathBuf>,
}

struct FileRec {
    path: PathBuf,
    size: u64,
    modified: std::time::SystemTime,
    online: bool,
}

fn walk(dir: &Path, guard: &Guard) -> Vec<FileRec> {
    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if guard.is_skipped(&path) {
            continue;
        }
        if ft.is_dir() {
            subdirs.push(path);
        } else if let Ok(meta) = entry.metadata() {
            files.push(FileRec {
                online: dupes::is_online_only(&meta),
                size: meta.len(),
                modified: meta.modified().unwrap_or(UNIX_EPOCH),
                path,
            });
        }
    }
    let nested: Vec<FileRec> = subdirs.par_iter().flat_map_iter(|d| walk(d, guard)).collect();
    files.extend(nested);
    files
}

/// "My Drive/Video" for G:\My Drive\Video\clip.mp4.
fn display_folder(base: &Path, file: &Path) -> String {
    let parent = file.parent().unwrap_or(file);
    parent.strip_prefix(base).unwrap_or(parent).to_string_lossy().replace('\\', "/")
}

pub fn scan(location: Location, chosen: Option<&str>, guard: &Guard, large_file: u64) -> Result<Scanned, String> {
    let root = status(location, chosen).root.map(PathBuf::from).ok_or("Lokasi ini belum terhubung")?;
    let base = root.parent().unwrap_or(&root).to_path_buf();
    let files = walk(&root, guard);

    let total_bytes = files.iter().map(|f| f.size).sum();
    let total_files = files.len() as u64;
    let (local, online): (Vec<&FileRec>, Vec<&FileRec>) = files.iter().partition(|f| !f.online);
    let local_bytes = local.iter().map(|f| f.size).sum();

    let groups = dupes::find(
        local.iter().map(|f| Candidate { path: f.path.clone(), size: f.size, modified: f.modified }).collect(),
    );

    let mut large: Vec<&FileRec> = files.iter().filter(|f| f.size >= large_file).collect();
    large.sort_by(|a, b| b.size.cmp(&a.size));

    let view = CloudScan {
        location,
        root: root.to_string_lossy().into_owned(),
        at_ms: now_ms(),
        total_bytes,
        total_files,
        local_bytes,
        local_files: local.len() as u64,
        online_files: online.len() as u64,
        dup_groups: groups.len(),
        dup_bytes: groups.iter().map(DupGroup::reclaim).sum(),
        large_bytes: large.iter().map(|f| f.size).sum(),
        duplicates: groups
            .iter()
            .map(|g| DupView {
                id: g.id.clone(),
                name: g.files[0].path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                size: g.size,
                reclaim: g.reclaim(),
                copies: g.files.len(),
                folders: g.files.iter().map(|c| display_folder(&base, &c.path)).collect(),
            })
            .collect(),
        large: large
            .iter()
            .map(|f| FileView {
                path: f.path.to_string_lossy().into_owned(),
                name: f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                folder: display_folder(&base, &f.path),
                bytes: f.size,
            })
            .collect(),
    };
    Ok(Scanned { view, large: large.iter().map(|f| f.path.clone()).collect(), groups })
}

/// Deletes the chosen duplicate copies (all but the newest of each group)
/// and the chosen big files. Only files the last scan showed are touched.
pub fn delete(scanned: &Scanned, dup_ids: &[String], files: &[String]) -> CleanReport {
    let mut report = CleanReport::default();
    let mut done: HashSet<PathBuf> = HashSet::new();
    let mut remove = |path: &Path, size: u64, report: &mut CleanReport| {
        if !done.insert(path.to_path_buf()) {
            return;
        }
        match fs::remove_file(path) {
            Ok(()) => {
                report.bytes += size;
                report.files += 1;
            }
            Err(_) => report.skipped += 1,
        }
    };

    let wanted: HashSet<&str> = dup_ids.iter().map(String::as_str).collect();
    for group in scanned.groups.iter().filter(|g| wanted.contains(g.id.as_str())) {
        for copy in dupes::extra_copies(group) {
            remove(&copy.path, group.size, &mut report);
        }
    }
    for file in files.iter().map(PathBuf::from).filter(|p| scanned.large.contains(p)) {
        if let Ok(meta) = fs::metadata(&file) {
            remove(&file, meta.len(), &mut report);
        }
    }
    report
}
