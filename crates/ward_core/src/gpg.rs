use std::{
  fs,
  path::{Path, PathBuf},
  process::Command,
};

use crate::{
  config::gpg_home,
  error::{WardError, WardResult},
};

pub struct GpgManager {
  original_home: PathBuf,
  backup_path: PathBuf,
}

impl GpgManager {
  #[must_use]
  pub fn new(backup_path: PathBuf) -> Self {
    Self { original_home: gpg_home(), backup_path }
  }

  pub fn kill_agent(&self) -> WardResult<()> {
    tracing::info!("Killing gpg-agent");
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

  pub fn backup_home(&self) -> WardResult<()> {
    if !self.original_home.exists() {
      tracing::debug!("No GPG home to backup");
      return Ok(());
    }
    if self.original_home.is_symlink() {
      tracing::debug!("GPG home is already a symlink, skipping backup");
      return Ok(());
    }
    tracing::info!(
      "Backing up {:?} to {:?}",
      self.original_home,
      self.backup_path
    );
    if self.backup_path.exists() {
      fs::remove_dir_all(&self.backup_path)?;
    }
    fs::rename(&self.original_home, &self.backup_path)?;
    Ok(())
  }

  pub fn restore_home(&self) -> WardResult<()> {
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
      } else if self.original_home.is_dir() {
        fs::remove_dir_all(&self.original_home)?;
      }
    }
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

  pub fn create_symlink(&self, target: &Path) -> WardResult<()> {
    tracing::info!("Creating symlink {:?} -> {:?}", self.original_home, target);
    if !target.exists() {
      return Err(WardError::Symlink(format!(
        "Target does not exist: {:?}",
        target
      )));
    }
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
        self.backup_home()?;
      }
    }
    #[cfg(unix)]
    {
      std::os::unix::fs::symlink(target, &self.original_home)?;
    }
    #[cfg(windows)]
    {
      std::os::windows::fs::symlink_dir(target, &self.original_home)?;
    }
    Ok(())
  }

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

  pub fn swap_to(&self, keys_path: &Path) -> WardResult<()> {
    tracing::info!("Swapping GPG home to {:?}", keys_path);
    self.kill_agent()?;
    if self.original_home.exists() && !self.original_home.is_symlink() {
      self.backup_home()?;
    }
    self.create_symlink(keys_path)?;
    self.start_agent(&self.original_home)?;
    Ok(())
  }

  pub fn swap_back(&self) -> WardResult<()> {
    tracing::info!("Restoring original GPG configuration");
    self.kill_agent()?;
    self.remove_symlink()?;
    self.restore_home()?;
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
