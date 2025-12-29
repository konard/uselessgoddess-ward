//! GPG agent management and symlink handling.

use std::{
  fs,
  path::{Path, PathBuf},
  process::Command,
};

use crate::{
  config::gpg_home,
  error::{WardError, WardResult},
};

/// Manages GPG agent lifecycle and home directory symlinks.
pub struct GpgManager {
  /// Original GPG home directory path.
  original_home: PathBuf,

  /// Backup path for the original GPG home.
  backup_path: PathBuf,
}

impl GpgManager {
  /// Creates a new GPG manager.
  #[must_use]
  pub fn new(backup_path: PathBuf) -> Self {
    Self { original_home: gpg_home(), backup_path }
  }

  /// Kills any running gpg-agent processes.
  ///
  /// # Errors
  ///
  /// Returns an error if the agent cannot be killed.
  pub fn kill_agent(&self) -> WardResult<()> {
    tracing::info!("Killing gpg-agent");

    // Try gpgconf first (preferred method)
    let result = Command::new("gpgconf").args(["--kill", "gpg-agent"]).output();

    match result {
      Ok(output) if output.status.success() => {
        tracing::debug!("gpg-agent killed via gpgconf");
        return Ok(());
      }
      Ok(output) => {
        tracing::debug!(
          "gpgconf kill returned non-zero: {}",
          String::from_utf8_lossy(&output.stderr)
        );
      }
      Err(e) => {
        tracing::debug!("gpgconf not available: {}", e);
      }
    }

    // Fallback: try to kill agent manually
    #[cfg(unix)]
    {
      let _ = Command::new("pkill").args(["-9", "gpg-agent"]).output();
    }

    #[cfg(windows)]
    {
      let _ =
        Command::new("taskkill").args(["/F", "/IM", "gpg-agent.exe"]).output();
    }

    Ok(())
  }

  /// Starts the gpg-agent with the specified home directory.
  ///
  /// # Errors
  ///
  /// Returns an error if the agent cannot be started.
  pub fn start_agent(&self, gpg_home: &Path) -> WardResult<()> {
    tracing::info!("Starting gpg-agent with home: {:?}", gpg_home);

    let result = Command::new("gpg-agent")
      .arg("--daemon")
      .env("GNUPGHOME", gpg_home)
      .output();

    match result {
      Ok(output) if output.status.success() => {
        tracing::debug!("gpg-agent started successfully");
        Ok(())
      }
      Ok(output) => {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Agent might already be running, which is fine
        if stderr.contains("already running") {
          tracing::debug!("gpg-agent was already running");
          Ok(())
        } else {
          Err(WardError::GpgAgent(format!("Failed to start agent: {}", stderr)))
        }
      }
      Err(e) => {
        Err(WardError::GpgAgent(format!("Failed to execute gpg-agent: {}", e)))
      }
    }
  }

  /// Backs up the current GPG home directory.
  ///
  /// # Errors
  ///
  /// Returns an error if the backup cannot be created.
  pub fn backup_home(&self) -> WardResult<()> {
    if !self.original_home.exists() {
      tracing::debug!("No GPG home to backup");
      return Ok(());
    }

    // Check if it's a symlink - don't backup symlinks
    if self.original_home.is_symlink() {
      tracing::debug!("GPG home is already a symlink, skipping backup");
      return Ok(());
    }

    tracing::info!(
      "Backing up {:?} to {:?}",
      self.original_home,
      self.backup_path
    );

    // Remove old backup if exists
    if self.backup_path.exists() {
      fs::remove_dir_all(&self.backup_path)?;
    }

    // Rename (atomic move) the directory
    fs::rename(&self.original_home, &self.backup_path)?;

    Ok(())
  }

  /// Restores the GPG home directory from backup.
  ///
  /// # Errors
  ///
  /// Returns an error if the restore fails.
  pub fn restore_home(&self) -> WardResult<()> {
    // Remove current GPG home (likely a symlink)
    if self.original_home.exists() || self.original_home.is_symlink() {
      if self.original_home.is_symlink() {
        // Remove symlink
        #[cfg(unix)]
        fs::remove_file(&self.original_home)?;
        #[cfg(windows)]
        {
          // On Windows, junction points are removed with remove_dir
          if self.original_home.is_dir() {
            fs::remove_dir(&self.original_home)?;
          } else {
            fs::remove_file(&self.original_home)?;
          }
        }
      } else if self.original_home.is_dir() {
        fs::remove_dir_all(&self.original_home)?;
      }
    }

    // Restore from backup if it exists
    if self.backup_path.exists() {
      tracing::info!(
        "Restoring {:?} from {:?}",
        self.original_home,
        self.backup_path
      );
      fs::rename(&self.backup_path, &self.original_home)?;
    }

    Ok(())
  }

  /// Creates a symlink from the GPG home to the active key storage.
  ///
  /// # Errors
  ///
  /// Returns an error if the symlink cannot be created.
  pub fn create_symlink(&self, target: &Path) -> WardResult<()> {
    tracing::info!("Creating symlink {:?} -> {:?}", self.original_home, target);

    // Ensure target exists
    if !target.exists() {
      return Err(WardError::Symlink(format!(
        "Target does not exist: {:?}",
        target
      )));
    }

    // Remove existing GPG home if it exists
    if self.original_home.exists() || self.original_home.is_symlink() {
      if self.original_home.is_symlink() {
        #[cfg(unix)]
        fs::remove_file(&self.original_home)?;
        #[cfg(windows)]
        {
          if self.original_home.is_dir() {
            fs::remove_dir(&self.original_home)?;
          } else {
            fs::remove_file(&self.original_home)?;
          }
        }
      } else {
        // It's a real directory - backup first
        self.backup_home()?;
      }
    }

    // Create symlink
    #[cfg(unix)]
    {
      std::os::unix::fs::symlink(target, &self.original_home)?;
    }

    #[cfg(windows)]
    {
      // Use junction point for directories on Windows
      // Junction points don't require admin privileges
      std::os::windows::fs::symlink_dir(target, &self.original_home)?;
    }

    Ok(())
  }

  /// Removes the GPG home symlink.
  ///
  /// # Errors
  ///
  /// Returns an error if the symlink cannot be removed.
  pub fn remove_symlink(&self) -> WardResult<()> {
    if !self.original_home.is_symlink() {
      tracing::debug!("GPG home is not a symlink");
      return Ok(());
    }

    tracing::info!("Removing symlink {:?}", self.original_home);

    #[cfg(unix)]
    fs::remove_file(&self.original_home)?;

    #[cfg(windows)]
    {
      if self.original_home.is_dir() {
        fs::remove_dir(&self.original_home)?;
      } else {
        fs::remove_file(&self.original_home)?;
      }
    }

    Ok(())
  }

  /// Performs a full swap to use keys from the specified path.
  ///
  /// This:
  /// 1. Kills any running gpg-agent
  /// 2. Backs up the current GPG home
  /// 3. Creates a symlink to the new keys
  /// 4. Starts gpg-agent with the new home
  ///
  /// # Errors
  ///
  /// Returns an error if any step fails.
  pub fn swap_to(&self, keys_path: &Path) -> WardResult<()> {
    tracing::info!("Swapping GPG home to {:?}", keys_path);

    // Step 1: Kill existing agent
    self.kill_agent()?;

    // Step 2: Backup current home (if not already a symlink)
    if self.original_home.exists() && !self.original_home.is_symlink() {
      self.backup_home()?;
    }

    // Step 3: Create symlink
    self.create_symlink(keys_path)?;

    // Step 4: Start agent with new home
    self.start_agent(&self.original_home)?;

    Ok(())
  }

  /// Restores the original GPG configuration.
  ///
  /// # Errors
  ///
  /// Returns an error if restoration fails.
  pub fn swap_back(&self) -> WardResult<()> {
    tracing::info!("Restoring original GPG configuration");

    // Step 1: Kill agent
    self.kill_agent()?;

    // Step 2: Remove symlink
    self.remove_symlink()?;

    // Step 3: Restore backup
    self.restore_home()?;

    // Step 4: Restart agent (optional - user might not want it)
    // self.start_agent(&self.original_home)?;

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  #[test]
  fn test_gpg_manager_creation() {
    let temp_dir = TempDir::new().unwrap();
    let backup_path = temp_dir.path().join("backup");
    let manager = GpgManager::new(backup_path);
    assert!(!manager.backup_path.as_os_str().is_empty());
  }
}
