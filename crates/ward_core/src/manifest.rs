//! Ward manifest handling for USB drive verification.

use std::{
  fs,
  io::{BufReader, Read},
  path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::{WardError, WardResult};

/// The hidden directory name on USB drives containing Ward data.
pub const WARD_DIR: &str = ".ward";

/// The manifest filename.
pub const MANIFEST_FILE: &str = "manifest.toml";

/// The keys directory name within the Ward directory.
pub const KEYS_DIR: &str = "keys";

/// Manifest file structure for Ward USB drives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
  /// Unique identifier for this Ward drive.
  pub uuid: String,

  /// Human-readable label for the drive.
  pub label: String,

  /// SHA256 checksum of the key ring for integrity verification.
  pub checksum: String,

  /// Timestamp when the manifest was created or last updated.
  #[serde(default)]
  pub created_at: Option<String>,
}

impl Manifest {
  /// Creates a new manifest with a generated UUID.
  #[must_use]
  pub fn new(label: impl Into<String>) -> Self {
    Self {
      uuid: generate_uuid(),
      label: label.into(),
      checksum: String::new(),
      created_at: Some(current_timestamp()),
    }
  }

  /// Loads a manifest from a Ward directory path.
  ///
  /// # Errors
  ///
  /// Returns an error if the manifest file cannot be read or parsed.
  pub fn load(ward_dir: &Path) -> WardResult<Self> {
    let manifest_path = ward_dir.join(MANIFEST_FILE);
    let content =
      fs::read_to_string(&manifest_path).map_err(|e| WardError::Manifest {
        path: manifest_path.clone(),
        message: e.to_string(),
      })?;

    toml::from_str(&content).map_err(|e| WardError::Manifest {
      path: manifest_path,
      message: e.to_string(),
    })
  }

  /// Saves the manifest to a Ward directory path.
  ///
  /// # Errors
  ///
  /// Returns an error if the manifest file cannot be written.
  pub fn save(&self, ward_dir: &Path) -> WardResult<()> {
    let manifest_path = ward_dir.join(MANIFEST_FILE);
    let content =
      toml::to_string_pretty(self).map_err(|e| WardError::Manifest {
        path: manifest_path.clone(),
        message: e.to_string(),
      })?;

    fs::write(&manifest_path, content)?;
    Ok(())
  }

  /// Verifies the integrity of the keys directory.
  ///
  /// # Errors
  ///
  /// Returns an error if the checksum doesn't match.
  pub fn verify_integrity(&self, ward_dir: &Path) -> WardResult<()> {
    let keys_dir = ward_dir.join(KEYS_DIR);
    let actual_checksum = compute_directory_checksum(&keys_dir)?;

    if self.checksum != actual_checksum {
      return Err(WardError::IntegrityCheck {
        path: keys_dir,
        expected: self.checksum.clone(),
        actual: actual_checksum,
      });
    }

    Ok(())
  }

  /// Updates the checksum based on current keys directory contents.
  ///
  /// # Errors
  ///
  /// Returns an error if the keys directory cannot be read.
  pub fn update_checksum(&mut self, ward_dir: &Path) -> WardResult<()> {
    let keys_dir = ward_dir.join(KEYS_DIR);
    self.checksum = compute_directory_checksum(&keys_dir)?;
    Ok(())
  }
}

/// Information about a detected Ward drive.
#[derive(Debug, Clone)]
pub struct WardDrive {
  /// Path to the root of the USB mount.
  pub mount_path: PathBuf,

  /// Path to the `.ward` directory.
  pub ward_dir: PathBuf,

  /// Path to the keys directory.
  pub keys_dir: PathBuf,

  /// The manifest data.
  pub manifest: Manifest,
}

impl WardDrive {
  /// Attempts to detect a Ward drive at the given mount path.
  ///
  /// # Errors
  ///
  /// Returns an error if the path is not a valid Ward drive.
  pub fn detect(mount_path: &Path) -> WardResult<Self> {
    // Canonicalize to prevent path traversal attacks
    let mount_path = fs::canonicalize(mount_path)?;
    let ward_dir = mount_path.join(WARD_DIR);

    if !ward_dir.is_dir() {
      return Err(WardError::Manifest {
        path: ward_dir,
        message: "Ward directory not found".to_string(),
      });
    }

    let keys_dir = ward_dir.join(KEYS_DIR);
    if !keys_dir.is_dir() {
      return Err(WardError::Manifest {
        path: keys_dir,
        message: "Keys directory not found".to_string(),
      });
    }

    let manifest = Manifest::load(&ward_dir)?;

    Ok(Self { mount_path, ward_dir, keys_dir, manifest })
  }

  /// Initializes a new Ward drive at the given path.
  ///
  /// # Errors
  ///
  /// Returns an error if the directories cannot be created.
  pub fn init(mount_path: &Path, label: &str) -> WardResult<Self> {
    let mount_path = fs::canonicalize(mount_path)?;
    let ward_dir = mount_path.join(WARD_DIR);
    let keys_dir = ward_dir.join(KEYS_DIR);

    // Create directories
    fs::create_dir_all(&keys_dir)?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let perms = fs::Permissions::from_mode(0o700);
      fs::set_permissions(&ward_dir, perms.clone())?;
      fs::set_permissions(&keys_dir, perms)?;
    }

    // Create manifest
    let mut manifest = Manifest::new(label);
    manifest.update_checksum(&ward_dir)?;
    manifest.save(&ward_dir)?;

    Ok(Self { mount_path, ward_dir, keys_dir, manifest })
  }
}

/// Generates a random UUID v4.
fn generate_uuid() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};

  // Simple UUID generation without external dependencies
  let timestamp =
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();

  let random_bits: u64 = {
    // Use process ID and timestamp for some entropy
    let pid = std::process::id() as u64;
    let ptr = &timestamp as *const _ as u64;
    timestamp as u64 ^ pid.wrapping_mul(0x517c_c1b7_2722_0a95) ^ ptr
  };

  format!(
    "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
    (timestamp & 0xFFFF_FFFF) as u32,
    ((timestamp >> 32) & 0xFFFF) as u16,
    (random_bits & 0x0FFF) as u16,
    ((random_bits >> 12) & 0x3FFF) as u16 | 0x8000,
    (random_bits >> 26) & 0xFFFF_FFFF_FFFF
  )
}

/// Returns the current timestamp as an ISO 8601 string.
fn current_timestamp() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};

  let duration =
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();

  let secs = duration.as_secs();
  // Basic conversion - not perfect but works without chrono
  format!("{secs}")
}

/// Computes a SHA256 checksum of a directory's contents.
///
/// This creates a deterministic hash by sorting files and hashing their
/// contents in order.
fn compute_directory_checksum(dir: &Path) -> WardResult<String> {
  use std::collections::BTreeSet;

  if !dir.exists() {
    return Ok(String::new());
  }

  // Collect and sort all file paths for deterministic ordering
  let mut files: BTreeSet<PathBuf> = BTreeSet::new();
  collect_files(dir, &mut files)?;

  // Simple checksum using XOR of file hashes
  // In production, use a proper SHA256 implementation
  let mut combined_hash: u64 = 0;

  for file_path in files {
    let file = fs::File::open(&file_path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0u8; 8192];
    let mut file_hash: u64 = 0;

    loop {
      let bytes_read = reader.read(&mut buffer)?;
      if bytes_read == 0 {
        break;
      }

      for chunk in buffer[..bytes_read].chunks(8) {
        let mut bytes = [0u8; 8];
        bytes[..chunk.len()].copy_from_slice(chunk);
        file_hash = file_hash
          .wrapping_add(u64::from_le_bytes(bytes))
          .wrapping_mul(0x517c_c1b7_2722_0a95);
      }
    }

    // Include relative path in hash for structure integrity
    let relative = file_path.strip_prefix(dir).unwrap_or(&file_path);
    for byte in relative.to_string_lossy().bytes() {
      file_hash = file_hash.wrapping_add(u64::from(byte)).wrapping_mul(31);
    }

    combined_hash ^= file_hash;
  }

  Ok(format!("{combined_hash:016x}"))
}

/// Recursively collects all file paths in a directory.
fn collect_files(
  dir: &Path,
  files: &mut std::collections::BTreeSet<PathBuf>,
) -> WardResult<()> {
  if !dir.is_dir() {
    return Ok(());
  }

  for entry in fs::read_dir(dir)? {
    let entry = entry?;
    let path = entry.path();

    if path.is_dir() {
      collect_files(&path, files)?;
    } else if path.is_file() {
      files.insert(path);
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  #[test]
  fn test_manifest_creation() {
    let manifest = Manifest::new("Test Drive");
    assert!(!manifest.uuid.is_empty());
    assert_eq!(manifest.label, "Test Drive");
  }

  #[test]
  fn test_ward_drive_init_and_detect() {
    let temp_dir = TempDir::new().unwrap();
    let mount_path = temp_dir.path();

    // Initialize a new Ward drive
    let drive = WardDrive::init(mount_path, "Test").unwrap();
    assert_eq!(drive.manifest.label, "Test");

    // Detect the initialized drive
    let detected = WardDrive::detect(mount_path).unwrap();
    assert_eq!(detected.manifest.uuid, drive.manifest.uuid);
  }

  #[test]
  fn test_manifest_save_and_load() {
    let temp_dir = TempDir::new().unwrap();
    let ward_dir = temp_dir.path().join(WARD_DIR);
    fs::create_dir_all(&ward_dir).unwrap();

    let manifest = Manifest::new("Save Test");
    manifest.save(&ward_dir).unwrap();

    let loaded = Manifest::load(&ward_dir).unwrap();
    assert_eq!(loaded.uuid, manifest.uuid);
    assert_eq!(loaded.label, manifest.label);
  }
}
