use anyhow::{Context, Result};
use sha2::{Digest, Sha256, Sha512};
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const CHUNK_SIZE: usize = 8 * 1024 * 1024; // 8MB chunks

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HashAlgorithm {
    Sha256,
    Sha512,
    Blake3,
}

impl HashAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            HashAlgorithm::Sha256 => "sha256",
            HashAlgorithm::Sha512 => "sha512",
            HashAlgorithm::Blake3 => "blake3",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sha256" => Some(HashAlgorithm::Sha256),
            "sha512" => Some(HashAlgorithm::Sha512),
            "blake3" => Some(HashAlgorithm::Blake3),
            _ => None,
        }
    }

    /// Detect algorithm from hash string length (sha256 and blake3 are
    /// both 64 chars, so we default to sha256 for ambiguous cases)
    pub fn detect_from_hash(hash: &str) -> Option<Self> {
        match hash.len() {
            64 => Some(HashAlgorithm::Sha256), // also blake3 - prefer sha256
            128 => Some(HashAlgorithm::Sha512),
            _ => None,
        }
    }
}

impl fmt::Display for HashAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub fn hash_file(
    path: &Path,
    algorithm: HashAlgorithm,
    progress_cb: Option<&dyn Fn(u64)>,
) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let mut reader = BufReader::with_capacity(CHUNK_SIZE, file);
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut bytes_read_total: u64 = 0;

    match algorithm {
        HashAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            loop {
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
                bytes_read_total += n as u64;
                if let Some(cb) = progress_cb {
                    cb(bytes_read_total);
                }
            }
            Ok(hex::encode(hasher.finalize()))
        }
        HashAlgorithm::Sha512 => {
            let mut hasher = Sha512::new();
            loop {
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
                bytes_read_total += n as u64;
                if let Some(cb) = progress_cb {
                    cb(bytes_read_total);
                }
            }
            Ok(hex::encode(hasher.finalize()))
        }
        HashAlgorithm::Blake3 => {
            let mut hasher = blake3::Hasher::new();
            loop {
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
                bytes_read_total += n as u64;
                if let Some(cb) = progress_cb {
                    cb(bytes_read_total);
                }
            }
            Ok(hasher.finalize().to_hex().to_string())
        }
    }
}
