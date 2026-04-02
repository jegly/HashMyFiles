use crate::database::HashDatabase;
use crate::hasher::{hash_file, HashAlgorithm};
use crate::scanner::{glob_match, VIRTUAL_MOUNT_POINTS};
use anyhow::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileStatus {
    Unchanged,
    Modified,
    Missing,
    New,
}

impl FileStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            FileStatus::Unchanged => "✅",
            FileStatus::Modified => "⚠️",
            FileStatus::Missing => "❌",
            FileStatus::New => "🆕",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            FileStatus::Unchanged => "Unchanged",
            FileStatus::Modified => "Modified",
            FileStatus::Missing => "Missing",
            FileStatus::New => "New",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            FileStatus::Unchanged => "diff-unchanged",
            FileStatus::Modified => "diff-modified",
            FileStatus::Missing => "diff-missing",
            FileStatus::New => "diff-new",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    pub path: PathBuf,
    pub status: FileStatus,
    pub old_hash: Option<String>,
    pub new_hash: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct VerifyStats {
    pub total: usize,
    pub unchanged: usize,
    pub modified: usize,
    pub missing: usize,
    pub new_files: usize,
    pub errors: usize,
}

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub entries: Vec<DiffEntry>,
    pub stats: VerifyStats,
    pub errors: Vec<(PathBuf, String)>,
}

#[derive(Debug, Clone)]
pub struct VerifyProgress {
    pub files_checked: u64,
    pub current_file: String,
    pub errors: u64,
}

impl VerifyResult {
    pub fn export_txt(&self, path: &Path) -> Result<()> {
        use std::fmt::Write as FmtWrite;
        let mut out = String::new();
        writeln!(out, "# hashmyfiles verification report")?;
        writeln!(
            out,
            "# {}: {} unchanged, {} modified, {} missing, {} new",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
            self.stats.unchanged,
            self.stats.modified,
            self.stats.missing,
            self.stats.new_files
        )?;
        writeln!(out, "#")?;
        for e in &self.entries {
            writeln!(out, "{} [{}]  {}", e.status.icon(), e.status.label(), e.path.display())?;
            if let Some(ref old) = e.old_hash { writeln!(out, "  old: {}", old)?; }
            if let Some(ref new) = e.new_hash { writeln!(out, "  new: {}", new)?; }
        }
        std::fs::write(path, out)?;
        Ok(())
    }

    pub fn export_json(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn export_csv(&self, path: &Path) -> Result<()> {
        let mut wtr = csv::Writer::from_path(path)?;
        wtr.write_record(["path", "status", "old_hash", "new_hash"])?;
        for e in &self.entries {
            wtr.write_record(&[
                e.path.to_str().unwrap_or(""),
                e.status.label(),
                e.old_hash.as_deref().unwrap_or(""),
                e.new_hash.as_deref().unwrap_or(""),
            ])?;
        }
        wtr.flush()?;
        Ok(())
    }
}

/// Walk `scan_root`, honouring `skip_virtual_fs` and `exclude_patterns`.
fn collect_disk_files(
    scan_root: &Path,
    skip_virtual_fs: bool,
    exclude_patterns: &[String],
    cancelled: &AtomicBool,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(scan_root).follow_links(false) {
        if cancelled.load(Ordering::Relaxed) { break; }
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        let path = entry.path();

        // Skip virtual FS mount points
        if skip_virtual_fs {
            let ps = path.to_string_lossy();
            if VIRTUAL_MOUNT_POINTS.iter().any(|&vfs| ps == vfs || ps.starts_with(&format!("{}/", vfs))) {
                continue;
            }
        }

        // Apply glob exclusions
        let excluded = exclude_patterns.iter().any(|pat| {
            let pat = pat.trim();
            if pat.is_empty() { return false; }
            if let Some(fname) = path.file_name() {
                if glob_match(pat, &fname.to_string_lossy()) { return true; }
            }
            for comp in path.components() {
                if glob_match(pat, &comp.as_os_str().to_string_lossy()) { return true; }
            }
            false
        });
        if excluded { continue; }

        if entry.file_type().is_file() {
            paths.push(path.to_path_buf());
        }
    }
    paths
}

pub fn verify_files<F>(
    database: &HashDatabase,
    scan_root: &Path,
    algorithm_override: Option<HashAlgorithm>,
    skip_virtual_fs: bool,
    exclude_patterns: Vec<String>,
    cancelled: Arc<AtomicBool>,
    progress_fn: F,
) -> VerifyResult
where
    F: Fn(VerifyProgress) + Send + Sync + 'static,
{
    let algorithm = algorithm_override.unwrap_or(database.algorithm);
    let progress_fn = Arc::new(progress_fn);

    let db_map: HashMap<PathBuf, String> = database.entries.iter()
        .map(|e| (e.path.clone(), e.hash.clone()))
        .collect();
    let db_paths: HashSet<PathBuf> = db_map.keys().cloned().collect();

    // Phase 1: collect disk files (respects exclusions + vfs skip)
    let disk_paths = collect_disk_files(
        scan_root, skip_virtual_fs, &exclude_patterns, &cancelled);

    let disk_set: HashSet<PathBuf> = disk_paths.iter().cloned().collect();
    let files_done = Arc::new(AtomicU64::new(0));
    let errors: Arc<Mutex<Vec<(PathBuf, String)>>> = Arc::new(Mutex::new(Vec::new()));

    // Phase 2: hash in parallel
    let mut diff_entries: Vec<DiffEntry> = disk_paths
        .par_iter()
        .filter_map(|path| {
            if cancelled.load(Ordering::Relaxed) { return None; }

            let result = hash_file(path, algorithm, None);
            let done = files_done.fetch_add(1, Ordering::Relaxed) + 1;
            let current_file = path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            progress_fn(VerifyProgress {
                files_checked: done,
                current_file,
                errors: errors.lock().map(|e| e.len() as u64).unwrap_or(0),
            });

            match result {
                Ok(new_hash) => {
                    let (status, old_hash) = match db_map.get(path) {
                        Some(old) if old == &new_hash => (FileStatus::Unchanged, Some(old.clone())),
                        Some(old) => (FileStatus::Modified, Some(old.clone())),
                        None => (FileStatus::New, None),
                    };
                    Some(DiffEntry { path: path.clone(), status, old_hash, new_hash: Some(new_hash) })
                }
                Err(e) => {
                    if let Ok(mut errs) = errors.lock() {
                        errs.push((path.clone(), e.to_string()));
                    }
                    None
                }
            }
        })
        .collect();

    // Phase 3: mark files in DB but missing from disk
    for db_path in &db_paths {
        if !disk_set.contains(db_path) {
            diff_entries.push(DiffEntry {
                path: db_path.clone(),
                status: FileStatus::Missing,
                old_hash: db_map.get(db_path).cloned(),
                new_hash: None,
            });
        }
    }

    // Sort by priority: Missing → Modified → New → Unchanged
    diff_entries.sort_by(|a, b| {
        let rank = |s: &FileStatus| match s {
            FileStatus::Missing => 0,
            FileStatus::Modified => 1,
            FileStatus::New => 2,
            FileStatus::Unchanged => 3,
        };
        rank(&a.status).cmp(&rank(&b.status)).then(a.path.cmp(&b.path))
    });

    let mut stats = VerifyStats::default();
    for e in &diff_entries {
        stats.total += 1;
        match e.status {
            FileStatus::Unchanged => stats.unchanged += 1,
            FileStatus::Modified  => stats.modified += 1,
            FileStatus::Missing   => stats.missing += 1,
            FileStatus::New       => stats.new_files += 1,
        }
    }

    let final_errors = errors.lock().map(|e| e.clone()).unwrap_or_default();
    stats.errors = final_errors.len();

    VerifyResult { entries: diff_entries, stats, errors: final_errors }
}
