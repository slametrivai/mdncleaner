//! Duplicate finder. Files are compared by content, never by name: first by
//! size, then by a fingerprint of the first and last 64 KB, then by a full
//! BLAKE3 hash. The newest copy of each group is always kept.

use crate::paths::Guard;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Smaller files are not worth a review; they barely free any space.
pub const MIN_SIZE: u64 = 256 * 1024;
const EDGE: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
}

#[derive(Clone, Debug)]
pub struct DupGroup {
    pub id: String,
    pub size: u64,
    /// Newest first: `files[0]` is the copy that is kept.
    pub files: Vec<Candidate>,
}

impl DupGroup {
    pub fn reclaim(&self) -> u64 {
        self.size * (self.files.len() as u64).saturating_sub(1)
    }
}

/// Cloud placeholders that are not downloaded. Reading one would download it.
pub fn is_online_only(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const OFFLINE: u32 = 0x1000;
        const RECALL_ON_OPEN: u32 = 0x4_0000;
        const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
        meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

/// Personal files under `dir` that are big enough to compare, in parallel.
pub fn collect(dir: &Path, guard: &Guard) -> Vec<Candidate> {
    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if !guard.allows_user_file(&path) {
            continue;
        }
        if ft.is_dir() {
            subdirs.push(path);
        } else if let Ok(meta) = entry.metadata() {
            if meta.len() >= MIN_SIZE && !is_online_only(&meta) {
                files.push(Candidate { path, size: meta.len(), modified: meta.modified().unwrap_or(UNIX_EPOCH) });
            }
        }
    }
    let nested: Vec<Candidate> = subdirs.par_iter().flat_map_iter(|d| collect(d, guard)).collect();
    files.extend(nested);
    files
}

fn read_up_to(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

fn edge_hash(path: &Path, size: u64) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; EDGE as usize];
    let n = read_up_to(&mut file, &mut buf).ok()?;
    hasher.update(&buf[..n]);
    if size > EDGE * 2 {
        file.seek(SeekFrom::Start(size - EDGE)).ok()?;
        let n = read_up_to(&mut file, &mut buf).ok()?;
        hasher.update(&buf[..n]);
    }
    Some(*hasher.finalize().as_bytes())
}

fn full_hash(path: &Path, _size: u64) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(&buf[..n]);
            }
            Err(_) => return None,
        }
    }
    Some(*hasher.finalize().as_bytes())
}

/// Splits a group of same-size files into subgroups with equal hashes.
fn split(group: Vec<Candidate>, hash: fn(&Path, u64) -> Option<[u8; 32]>) -> Vec<([u8; 32], Vec<Candidate>)> {
    let mut by_hash: HashMap<[u8; 32], Vec<Candidate>> = HashMap::new();
    for c in group {
        if let Some(h) = hash(&c.path, c.size) {
            by_hash.entry(h).or_default().push(c);
        }
    }
    by_hash.into_iter().filter(|(_, v)| v.len() > 1).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn find(candidates: Vec<Candidate>) -> Vec<DupGroup> {
    let mut by_size: HashMap<u64, Vec<Candidate>> = HashMap::new();
    for c in candidates.into_iter().filter(|c| c.size >= MIN_SIZE) {
        by_size.entry(c.size).or_default().push(c);
    }
    let same_size: Vec<Vec<Candidate>> = by_size.into_values().filter(|g| g.len() > 1).collect();

    let same_edges: Vec<Vec<Candidate>> = same_size
        .into_par_iter()
        .flat_map_iter(|g| split(g, edge_hash).into_iter().map(|(_, v)| v))
        .collect();

    let mut groups: Vec<DupGroup> = same_edges
        .into_par_iter()
        .flat_map_iter(|g| split(g, full_hash))
        .map(|(hash, mut files)| {
            files.sort_by(|a, b| b.modified.cmp(&a.modified));
            DupGroup { id: hex(&hash[..8]), size: files[0].size, files }
        })
        .collect();

    groups.sort_by(|a, b| b.reclaim().cmp(&a.reclaim()));
    groups
}

/// The copies to delete from a group: every file but the newest, and only
/// while it still has the size it had when it was compared.
pub fn extra_copies(group: &DupGroup) -> impl Iterator<Item = &Candidate> {
    group
        .files
        .iter()
        .skip(1)
        .filter(move |c| fs::metadata(&c.path).map(|m| m.len() == group.size).unwrap_or(false))
}
