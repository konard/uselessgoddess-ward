//! Key storage abstraction and implementations.

use std::{
  fs,
  path::{Path, PathBuf},
};

use crate::error::WardResult;

/// Trait for GPG key storage backends.
///
/// This abstracts the differences between Linux (RAM-based) and Windows
/// (direct USB access) storage strategies.
pub trait KeyStorage: Send + Sync {
  /// Activates the key storage, copying or linking keys as needed.
  ///
  /// Returns the path to the GPG home directory to use.
  fn activate(&self, source_keys_dir: &Path) -> WardResult<PathBuf>;

  /// Deactivates the key storage, securely wiping any copies.
  fn deactivate(&self) -> WardResult<()>;

  /// Returns the current active GPG home path, if any.
  fn active_path(&self) -> Option<&Path>;

  /// Returns true if this storage is currently active.
  fn is_active(&self) -> bool {
    self.active_path().is_some()
  }
}

/// Linux implementation using RAM-based storage.
///
/// On Linux, we copy keys to `/dev/shm` or `/run/user/UID` (a tmpfs) to ensure
/// keys never touch the hard drive.
#[cfg(target_os = "linux")]
pub mod linux {
  use super::*;
  use std::sync::Mutex;

  /// RAM-based key storage for Linux.
  pub struct RamStorage {
    /// Base path for RAM storage (e.g., `/dev/shm` or `/run/user/UID`).
    ram_base: PathBuf,

    /// Current active storage path.
    active_path: Mutex<Option<PathBuf>>,
  }

  impl RamStorage {
    /// Creates a new RAM storage instance.
    ///
    /// Attempts to use `/run/user/UID` first, falling back to `/dev/shm`.
    #[must_use]
    pub fn new() -> Self {
      let ram_base = Self::find_ram_path();
      Self { ram_base, active_path: Mutex::new(None) }
    }

    /// Finds the best RAM-based path for the current user.
    fn find_ram_path() -> PathBuf {
      // Try XDG_RUNTIME_DIR first (usually /run/user/UID)
      if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime_dir);
        if path.exists() {
          return path;
        }
      }

      // Try /run/user/UID
      let uid = get_uid();
      let run_user = PathBuf::from(format!("/run/user/{uid}"));
      if run_user.exists() {
        return run_user;
      }

      // Fall back to /dev/shm
      PathBuf::from("/dev/shm")
    }

    /// Securely wipes a file by overwriting with zeros.
    fn secure_wipe(path: &Path) -> WardResult<()> {
      if !path.exists() {
        return Ok(());
      }

      if path.is_file() {
        // Overwrite file contents with zeros
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

    /// Recursively copies a directory with proper permissions.
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

      // Deactivate any existing storage first
      if let Some(ref path) = *active {
        tracing::info!("Deactivating existing storage at {:?}", path);
        Self::secure_wipe(path)?;
      }

      // Create new RAM storage directory
      let storage_name = format!("ward-gpg-{}", std::process::id());
      let storage_path = self.ram_base.join(&storage_name);

      tracing::info!(
        "Copying keys from {:?} to RAM storage at {:?}",
        source_keys_dir,
        storage_path
      );

      // Copy keys to RAM
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
      // Note: This is a simplified implementation. In production,
      // we'd need interior mutability or a different approach.
      None
    }
  }

  // SAFETY: getuid() is always safe to call and has no side effects.
  unsafe extern "C" {
    fn getuid() -> u32;
  }

  /// Gets the current user ID.
  fn get_uid() -> u32 {
    // SAFETY: getuid() is always safe to call and has no side effects.
    unsafe { getuid() }
  }
}

/// Windows implementation using direct USB access.
///
/// On Windows, we access the keys directly from the USB drive since RAM-based
/// storage is less straightforward.
#[cfg(target_os = "windows")]
pub mod windows {
  use super::*;
  use std::sync::Mutex;

  /// Direct USB access storage for Windows.
  pub struct DirectStorage {
    /// Current active storage path (the USB keys directory).
    active_path: Mutex<Option<PathBuf>>,
  }

  impl DirectStorage {
    /// Creates a new direct storage instance.
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

      // Canonicalize the path for safety
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

/// Platform-agnostic mock storage for testing.
#[cfg(test)]
pub mod mock {
  use super::*;
  use std::sync::Mutex;

  /// Mock storage that records operations for testing.
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

/// Creates the appropriate key storage for the current platform.
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
  // Fallback for other platforms - use a simple approach
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
