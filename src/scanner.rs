use crate::database::{HashDatabase, HashEntry};
use crate::hasher::{hash_file, HashAlgorithm};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

/// Virtual/pseudo filesystems to skip by default when scanning /
/// These can cause hangs or infinite reads.
pub const VIRTUAL_MOUNT_POINTS: &[&str] = &[
    "/proc", "/sys", "/dev", "/run", "/tmp", "/var/run",
];

/// Progress update sent from scanner thread to UI via channel
#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub files_scanned: u64,
    pub bytes_processed: u64, // read by future progress UI
    pub current_file: String,
    pub errors: u64,
}

/// Final result returned when scan completes
#[derive(Debug)]
pub struct ScanResult {
    pub database: HashDatabase,
    pub errors: Vec<(PathBuf, String)>,
    pub skipped: u64,
}

pub struct ScanOptions {
    pub root: PathBuf,
    pub algorithm: HashAlgorithm,
    pub recursive: bool,
    pub exclude_patterns: Vec<String>,
    pub skip_virtual_fs: bool,
}

impl ScanOptions {
    fn matches_exclusion(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        for pattern in &self.exclude_patterns {
            let pat = pattern.trim();
            if pat.is_empty() {
                continue;
            }
            // Check filename glob
            if let Some(fname) = path.file_name() {
                let fname_str = fname.to_string_lossy();
                if glob_match(pat, &fname_str) {
                    return true;
                }
            }
            // Check directory name match (e.g. ".git")
            for component in path.components() {
                if glob_match(pat, &component.as_os_str().to_string_lossy()) {
                    return true;
                }
            }
            // Substring match on full path
            if path_str.contains(pat) {
                return true;
            }
        }
        false
    }

    fn is_virtual_fs(&self, path: &Path) -> bool {
        if !self.skip_virtual_fs {
            return false;
        }
        let path_str = path.to_string_lossy();
        VIRTUAL_MOUNT_POINTS
            .iter()
            .any(|&vfs| path_str == vfs || path_str.starts_with(&format!("{}/", vfs)))
    }
}

/// Simple glob: supports `*` wildcard only (sufficient for *.log, *.tmp)
pub fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(ext) = pattern.strip_prefix("*.") {
        return text.ends_with(&format!(".{}", ext));
    }
    pattern == text
}

/// Run the scan on a background thread, sending progress via callback.
/// Returns ScanResult when done (or when cancelled).
///
/// Uses rayon for parallel hashing. Progress updates are batched and sent
/// via the `progress_fn` callback — safe to call from a thread.
pub fn scan_files<F>(
    options: ScanOptions,
    cancelled: Arc<AtomicBool>,
    progress_fn: F,
) -> ScanResult
where
    F: Fn(ScanProgress) + Send + Sync + 'static,
{
    let progress_fn = Arc::new(progress_fn);

    // Phase 1: Collect all paths (single-threaded walk)
    let mut all_paths: Vec<PathBuf> = Vec::new();
    let mut skipped: u64 = 0;

    let walker = if options.recursive {
        WalkDir::new(&options.root).follow_links(false)
    } else {
        WalkDir::new(&options.root).max_depth(1).follow_links(false)
    };

    for entry in walker {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }

        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                eprintln!("Walk error: {}", err);
                skipped += 1;
                continue;
            }
        };

        let path = entry.path().to_path_buf();

        if options.is_virtual_fs(&path) {
            skipped += 1;
            continue;
        }

        if options.matches_exclusion(&path) {
            skipped += 1;
            continue;
        }

        if entry.file_type().is_file() {
            all_paths.push(path);
        }
    }

    let _total_files = all_paths.len() as u64;

    // Phase 2: Hash files in parallel with rayon
    let files_done = Arc::new(AtomicU64::new(0));
    let bytes_done = Arc::new(AtomicU64::new(0));
    let errors: Arc<Mutex<Vec<(PathBuf, String)>>> = Arc::new(Mutex::new(Vec::new()));

    let entries: Vec<Option<HashEntry>> = all_paths
        .par_iter()
        .map(|path| {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }

            let bytes_counter = bytes_done.clone();
            let progress_cb = {
                let bytes_counter = bytes_counter.clone();
                move |bytes: u64| {
                    bytes_counter.store(bytes, Ordering::Relaxed);
                }
            };

            let result = hash_file(path, options.algorithm, Some(&progress_cb));

            let done = files_done.fetch_add(1, Ordering::Relaxed) + 1;

            // Send progress update
            let current_file = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            progress_fn(ScanProgress {
                files_scanned: done,
                bytes_processed: bytes_done.load(Ordering::Relaxed),
                current_file,
                errors: errors.lock().map(|e| e.len() as u64).unwrap_or(0),
            });

            match result {
                Ok(hash) => Some(HashEntry {
                    hash,
                    path: path.clone(),
                }),
                Err(e) => {
                    if let Ok(mut errs) = errors.lock() {
                        errs.push((path.clone(), e.to_string()));
                    }
                    None
                }
            }
        })
        .collect();

    let final_entries: Vec<HashEntry> = entries.into_iter().flatten().collect();
    let final_errors = errors.lock().map(|e| e.clone()).unwrap_or_default();

    let mut db = HashDatabase::new(options.algorithm);
    db.entries = final_entries;

    ScanResult {
        database: db,
        errors: final_errors,
        skipped,
    }
}
