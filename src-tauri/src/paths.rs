//! Path helpers and the guard that keeps the cleaner away from folders it
//! must never touch.

use std::path::{Path, PathBuf};

pub fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// A folder in the user's profile, such as "Downloads".
pub fn user_dir(name: &str) -> Option<PathBuf> {
    env_path("USERPROFILE").map(|p| p.join(name))
}

pub fn lower(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// True when `path` is `base` or lies inside it (case-insensitive, like NTFS).
pub fn is_under(path: &Path, base: &Path) -> bool {
    let (p, b) = (lower(path), lower(base));
    !b.is_empty() && (p == b || p.starts_with(&format!("{b}\\")))
}

/// Folders that belong to an app or a game: duplicates or big files in here
/// are part of the program, so they are never offered for deletion.
const APP_MANAGED: &[&str] = &[
    "steamapps",
    "epic games",
    "riot games",
    "xboxgames",
    "windowsapps",
    "node_modules",
    ".git",
];

fn system_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData"]
        .iter()
        .filter_map(|k| env_path(k))
        .collect();
    if let Some(appdata) = user_dir("AppData") {
        roots.push(appdata);
    }
    roots
}

#[derive(Clone, Debug)]
pub struct Guard {
    skipped: Vec<PathBuf>,
    protected: Vec<PathBuf>,
}

impl Guard {
    pub fn new(skipped: &[String]) -> Self {
        Self { skipped: skipped.iter().map(PathBuf::from).collect(), protected: system_roots() }
    }

    /// Inside a folder the user listed under "Folder yang dilewati".
    pub fn is_skipped(&self, path: &Path) -> bool {
        self.skipped.iter().any(|s| is_under(path, s))
    }

    /// Windows, Program Files, ProgramData, AppData, drive-level system
    /// folders ($Recycle.Bin, System Volume Information) and app folders.
    pub fn is_protected(&self, path: &Path) -> bool {
        if self.protected.iter().any(|p| is_under(path, p)) {
            return true;
        }
        path.components().any(|c| {
            let name = c.as_os_str().to_string_lossy().to_lowercase();
            name.starts_with('$') || name == "system volume information" || APP_MANAGED.contains(&name.as_str())
        })
    }

    /// May this personal file (a duplicate, a big file, an old download) be
    /// offered for deletion?
    pub fn allows_user_file(&self, path: &Path) -> bool {
        !self.is_skipped(path) && !self.is_protected(path)
    }
}
