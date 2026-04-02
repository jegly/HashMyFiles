use crate::hasher::HashAlgorithm;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const DB_MAGIC: &str = "# hashmyfiles";
pub const DB_VERSION: &str = "1.0";

/// One entry in the hash database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashEntry {
    pub hash: String,
    pub path: PathBuf,
}

/// The full in-memory database
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HashDatabase {
    pub algorithm: HashAlgorithm,
    pub created_at: DateTime<Local>,
    pub source_path: Option<PathBuf>,
    pub entries: Vec<HashEntry>,
}

impl HashDatabase {
    pub fn new(algorithm: HashAlgorithm) -> Self {
        Self {
            algorithm,
            created_at: Local::now(),
            source_path: None,
            entries: Vec::new(),
        }
    }

    /// Build a path→hash lookup map for fast verification
    pub fn as_map(&self) -> HashMap<PathBuf, &str> {
        self.entries
            .iter()
            .map(|e| (e.path.clone(), e.hash.as_str()))
            .collect()
    }

    /// Save to .txt with metadata header
    pub fn save_txt(&self, path: &Path) -> Result<()> {
        let mut f = File::create(path)
            .with_context(|| format!("Cannot create file: {}", path.display()))?;

        // Header block
        writeln!(f, "{} v{}", DB_MAGIC, DB_VERSION)?;
        writeln!(f, "# algorithm={}", self.algorithm.as_str())?;
        writeln!(
            f,
            "# created={}",
            self.created_at.format("%Y-%m-%dT%H:%M:%S%z")
        )?;
        writeln!(f, "# entries={}", self.entries.len())?;
        writeln!(f, "#")?;

        for entry in &self.entries {
            writeln!(f, "{}  {}", entry.hash, entry.path.display())?;
        }
        Ok(())
    }

    /// Load from .txt — parses header for algorithm
    pub fn load_txt(path: &Path) -> Result<Self> {
        let f = File::open(path)
            .with_context(|| format!("Cannot open database: {}", path.display()))?;
        let reader = BufReader::new(f);

        let mut algorithm: Option<HashAlgorithm> = None;
        let mut created_at: Option<DateTime<Local>> = None;
        let mut entries = Vec::new();
        let mut found_magic = false;

        for (line_no, line_result) in reader.lines().enumerate() {
            let line = line_result?;

            if line.starts_with('#') {
                // Parse header lines
                if line.starts_with(DB_MAGIC) {
                    found_magic = true;
                } else if let Some(rest) = line.strip_prefix("# algorithm=") {
                    algorithm = HashAlgorithm::from_str(rest.trim());
                } else if let Some(rest) = line.strip_prefix("# created=") {
                    if let Ok(dt) = DateTime::parse_from_str(rest.trim(), "%Y-%m-%dT%H:%M:%S%z") {
                        created_at = Some(dt.with_timezone(&Local));
                    }
                }
                continue;
            }

            if line.trim().is_empty() {
                continue;
            }

            // Parse data lines: "hash  /path/to/file"
            // Split on two-or-more spaces
            let mut parts = line.splitn(2, "  ");
            let hash = match parts.next() {
                Some(h) => h.trim().to_string(),
                None => {
                    eprintln!("Warning: malformed line {} — skipping", line_no + 1);
                    continue;
                }
            };
            let path_str = match parts.next() {
                Some(p) => p.trim(),
                None => {
                    eprintln!("Warning: missing path on line {} — skipping", line_no + 1);
                    continue;
                }
            };

            // Auto-detect algorithm from hash length if not in header
            if algorithm.is_none() {
                algorithm = HashAlgorithm::detect_from_hash(&hash);
            }

            entries.push(HashEntry {
                hash,
                path: PathBuf::from(path_str),
            });
        }

        if !found_magic && entries.is_empty() {
            bail!("File does not appear to be a hashmyfiles database");
        }

        Ok(HashDatabase {
            algorithm: algorithm.unwrap_or(HashAlgorithm::Sha256),
            created_at: created_at.unwrap_or_else(Local::now),
            source_path: Some(path.to_path_buf()),
            entries,
        })
    }

    /// Export as JSON
    pub fn export_json(&self, path: &Path) -> Result<()> {
        #[derive(Serialize)]
        struct JsonDb<'a> {
            version: &'static str,
            algorithm: &'a str,
            created: String,
            entries: Vec<JsonEntry<'a>>,
        }
        #[derive(Serialize)]
        struct JsonEntry<'a> {
            hash: &'a str,
            path: &'a str,
        }

        let data = JsonDb {
            version: DB_VERSION,
            algorithm: self.algorithm.as_str(),
            created: self.created_at.format("%Y-%m-%dT%H:%M:%S%z").to_string(),
            entries: self
                .entries
                .iter()
                .map(|e| JsonEntry {
                    hash: &e.hash,
                    path: e.path.to_str().unwrap_or(""),
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&data)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Export as CSV
    pub fn export_csv(&self, path: &Path) -> Result<()> {
        let mut wtr = csv::Writer::from_path(path)?;
        wtr.write_record(["hash", "path", "algorithm", "created"])?;
        let created = self.created_at.format("%Y-%m-%dT%H:%M:%S%z").to_string();
        let algo = self.algorithm.as_str();
        for entry in &self.entries {
            wtr.write_record(&[
                &entry.hash,
                entry.path.to_str().unwrap_or(""),
                algo,
                &created,
            ])?;
        }
        wtr.flush()?;
        Ok(())
    }
}

/// Generate default output filename: hash_YYYYMMDD_HHMMSS.txt
pub fn default_output_filename(algorithm: HashAlgorithm) -> String {
    let now = Local::now();
    format!(
        "hash_{}_{}.txt",
        algorithm.as_str(),
        now.format("%Y%m%d_%H%M%S")
    )
}
