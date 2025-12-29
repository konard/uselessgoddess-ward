use std::{
  fs,
  io::{BufReader, Read},
  path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::{WardError, WardResult};

pub const WARD_DIR: &str = ".ward";
pub const MANIFEST_FILE: &str = "manifest.toml";
pub const KEYS_DIR: &str = "keys";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
  pub uuid: String,
  pub label: String,
  pub checksum: String,
  #[serde(default)]
  pub created_at: Option<String>,
}

impl Manifest {
  #[must_use]
  pub fn new(label: impl Into<String>) -> Self {
    Self {
      uuid: generate_uuid(),
      label: label.into(),
      checksum: String::new(),
      created_at: Some(current_timestamp()),
    }
  }

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

  pub fn update_checksum(&mut self, ward_dir: &Path) -> WardResult<()> {
    let keys_dir = ward_dir.join(KEYS_DIR);
    self.checksum = compute_directory_checksum(&keys_dir)?;
    Ok(())
  }
}

#[derive(Debug, Clone)]
pub struct WardDrive {
  pub mount_path: PathBuf,
  pub ward_dir: PathBuf,
  pub keys_dir: PathBuf,
  pub manifest: Manifest,
}

impl WardDrive {
  pub fn detect(mount_path: &Path) -> WardResult<Self> {
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

  pub fn init(mount_path: &Path, label: &str) -> WardResult<Self> {
    let mount_path = fs::canonicalize(mount_path)?;
    let ward_dir = mount_path.join(WARD_DIR);
    let keys_dir = ward_dir.join(KEYS_DIR);
    fs::create_dir_all(&keys_dir)?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let perms = fs::Permissions::from_mode(0o700);
      fs::set_permissions(&ward_dir, perms.clone())?;
      fs::set_permissions(&keys_dir, perms)?;
    }
    let mut manifest = Manifest::new(label);
    manifest.update_checksum(&ward_dir)?;
    manifest.save(&ward_dir)?;
    Ok(Self { mount_path, ward_dir, keys_dir, manifest })
  }
}

fn generate_uuid() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};
  let timestamp =
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
  let random_bits: u64 = {
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

fn current_timestamp() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};
  let duration =
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
  format!("{}", duration.as_secs())
}

fn compute_directory_checksum(dir: &Path) -> WardResult<String> {
  use std::collections::BTreeSet;
  if !dir.exists() {
    return Ok(String::new());
  }
  let mut files: BTreeSet<PathBuf> = BTreeSet::new();
  collect_files(dir, &mut files)?;
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
    let relative = file_path.strip_prefix(dir).unwrap_or(&file_path);
    for byte in relative.to_string_lossy().bytes() {
      file_hash = file_hash.wrapping_add(u64::from(byte)).wrapping_mul(31);
    }
    combined_hash ^= file_hash;
  }
  Ok(format!("{combined_hash:016x}"))
}

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
    let drive = WardDrive::init(mount_path, "Test").unwrap();
    assert_eq!(drive.manifest.label, "Test");
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
