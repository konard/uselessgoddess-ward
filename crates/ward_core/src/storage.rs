use std::{
  fs,
  path::{Path, PathBuf},
};

use crate::error::WardResult;

pub trait KeyStorage: Send + Sync {
  fn activate(&self, source_keys_dir: &Path) -> WardResult<PathBuf>;
  fn deactivate(&self) -> WardResult<()>;
  fn active_path(&self) -> Option<&Path>;
  fn is_active(&self) -> bool {
    self.active_path().is_some()
  }
}

#[cfg(target_os = "linux")]
pub mod linux {
  use super::*;
  use std::sync::Mutex;

  pub struct RamStorage {
    ram_base: PathBuf,
    active_path: Mutex<Option<PathBuf>>,
  }

  impl RamStorage {
    #[must_use]
    pub fn new() -> Self {
      let ram_base = Self::find_ram_path();
      Self { ram_base, active_path: Mutex::new(None) }
    }

    fn find_ram_path() -> PathBuf {
      if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime_dir);
        if path.exists() {
          return path;
        }
      }
      let uid = get_uid();
      let run_user = PathBuf::from(format!("/run/user/{uid}"));
      if run_user.exists() {
        return run_user;
      }
      PathBuf::from("/dev/shm")
    }

    fn secure_wipe(path: &Path) -> WardResult<()> {
      if !path.exists() {
        return Ok(());
      }
      if path.is_file() {
        let metadata = fs::metadata(path)?;
        let size = metadata.len() as usize;
        let zeros = vec![0u8; size];
        fs::write(path, &zeros)?;
        fs::remove_file(path)?;
      } else if path.is_dir() {
        for entry in fs::read_dir(path)? {
          let entry = entry?;
          Self::secure_wipe(&entry.path())?;
        }
        fs::remove_dir(path)?;
      }
      Ok(())
    }

    fn copy_dir_secure(src: &Path, dst: &Path) -> WardResult<()> {
      use std::os::unix::fs::PermissionsExt;
      fs::create_dir_all(dst)?;
      fs::set_permissions(dst, fs::Permissions::from_mode(0o700))?;
      for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
          Self::copy_dir_secure(&src_path, &dst_path)?;
        } else {
          fs::copy(&src_path, &dst_path)?;
          fs::set_permissions(&dst_path, fs::Permissions::from_mode(0o600))?;
        }
      }
      Ok(())
    }
  }

  impl Default for RamStorage {
    fn default() -> Self {
      Self::new()
    }
  }

  impl KeyStorage for RamStorage {
    fn activate(&self, source_keys_dir: &Path) -> WardResult<PathBuf> {
      let mut active = self.active_path.lock().unwrap();
      if let Some(ref path) = *active {
        tracing::info!("Deactivating existing storage at {:?}", path);
        Self::secure_wipe(path)?;
      }
      let storage_name = format!("ward-gpg-{}", std::process::id());
      let storage_path = self.ram_base.join(&storage_name);
      tracing::info!(
        "Copying keys from {:?} to RAM storage at {:?}",
        source_keys_dir,
        storage_path
      );
      Self::copy_dir_secure(source_keys_dir, &storage_path)?;
      *active = Some(storage_path.clone());
      Ok(storage_path)
    }

    fn deactivate(&self) -> WardResult<()> {
      let mut active = self.active_path.lock().unwrap();
      if let Some(ref path) = *active {
        tracing::info!("Securely wiping RAM storage at {:?}", path);
        Self::secure_wipe(path)?;
      }
      *active = None;
      Ok(())
    }

    fn active_path(&self) -> Option<&Path> {
      None
    }
  }

  unsafe extern "C" {
    fn getuid() -> u32;
  }

  fn get_uid() -> u32 {
    unsafe { getuid() }
  }
}

#[cfg(target_os = "windows")]
pub mod windows {
  use super::*;
  use std::sync::Mutex;

  pub struct DirectStorage {
    active_path: Mutex<Option<PathBuf>>,
  }

  impl DirectStorage {
    #[must_use]
    pub fn new() -> Self {
      Self { active_path: Mutex::new(None) }
    }
  }

  impl Default for DirectStorage {
    fn default() -> Self {
      Self::new()
    }
  }

  impl KeyStorage for DirectStorage {
    fn activate(&self, source_keys_dir: &Path) -> WardResult<PathBuf> {
      let mut active = self.active_path.lock().unwrap();
      let canonical = fs::canonicalize(source_keys_dir)?;
      tracing::info!("Activating direct storage at {:?}", canonical);
      *active = Some(canonical.clone());
      Ok(canonical)
    }

    fn deactivate(&self) -> WardResult<()> {
      let mut active = self.active_path.lock().unwrap();
      tracing::info!("Deactivating direct storage");
      *active = None;
      Ok(())
    }

    fn active_path(&self) -> Option<&Path> {
      None
    }
  }
}

#[cfg(test)]
pub mod mock {
  use super::*;
  use std::sync::Mutex;

  pub struct MockStorage {
    active_path: Mutex<Option<PathBuf>>,
    pub activate_count: std::sync::atomic::AtomicUsize,
    pub deactivate_count: std::sync::atomic::AtomicUsize,
  }

  impl MockStorage {
    pub fn new() -> Self {
      Self {
        active_path: Mutex::new(None),
        activate_count: std::sync::atomic::AtomicUsize::new(0),
        deactivate_count: std::sync::atomic::AtomicUsize::new(0),
      }
    }
  }

  impl KeyStorage for MockStorage {
    fn activate(&self, source_keys_dir: &Path) -> WardResult<PathBuf> {
      self.activate_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
      let mut active = self.active_path.lock().unwrap();
      *active = Some(source_keys_dir.to_path_buf());
      Ok(source_keys_dir.to_path_buf())
    }

    fn deactivate(&self) -> WardResult<()> {
      self.deactivate_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
      let mut active = self.active_path.lock().unwrap();
      *active = None;
      Ok(())
    }

    fn active_path(&self) -> Option<&Path> {
      None
    }
  }
}

#[cfg(target_os = "linux")]
pub fn create_platform_storage() -> Box<dyn KeyStorage> {
  Box::new(linux::RamStorage::new())
}

#[cfg(target_os = "windows")]
pub fn create_platform_storage() -> Box<dyn KeyStorage> {
  Box::new(windows::DirectStorage::new())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn create_platform_storage() -> Box<dyn KeyStorage> {
  struct FallbackStorage;
  impl KeyStorage for FallbackStorage {
    fn activate(&self, source_keys_dir: &Path) -> WardResult<PathBuf> {
      Ok(source_keys_dir.to_path_buf())
    }
    fn deactivate(&self) -> WardResult<()> {
      Ok(())
    }
    fn active_path(&self) -> Option<&Path> {
      None
    }
  }
  Box::new(FallbackStorage)
}
