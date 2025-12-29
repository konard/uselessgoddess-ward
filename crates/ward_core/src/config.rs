//! Configuration management for Ward.

use std::{collections::HashSet, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{WardError, WardResult};

/// Ward daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
  /// UUIDs of allowed Ward drives.
  #[serde(default)]
  pub allowed_drives: HashSet<String>,

  /// Path to store GPG backup when swapping.
  #[serde(default = "default_backup_path")]
  pub gpg_backup_path: PathBuf,

  /// Polling interval in milliseconds for drive detection.
  #[serde(default = "default_poll_interval")]
  pub poll_interval_ms: u64,

  /// Enable verbose logging.
  #[serde(default)]
  pub verbose: bool,
}

impl Default for Config {
  fn default() -> Self {
    Self {
      allowed_drives: HashSet::new(),
      gpg_backup_path: default_backup_path(),
      poll_interval_ms: default_poll_interval(),
      verbose: false,
    }
  }
}

fn default_backup_path() -> PathBuf {
  config_dir().join("gnupg_backup")
}

const fn default_poll_interval() -> u64 {
  1000 // 1 second
}

impl Config {
  /// Loads the configuration from the default location.
  ///
  /// # Errors
  ///
  /// Returns an error if the config file exists but cannot be read or parsed.
  pub fn load() -> WardResult<Self> {
    let config_path = config_file();

    if !config_path.exists() {
      return Ok(Self::default());
    }

    let content = fs::read_to_string(&config_path)?;
    toml::from_str(&content).map_err(|e| WardError::Config(e.to_string()))
  }

  /// Saves the configuration to the default location.
  ///
  /// # Errors
  ///
  /// Returns an error if the config file cannot be written.
  pub fn save(&self) -> WardResult<()> {
    let config_path = config_file();

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
      fs::create_dir_all(parent)?;
    }

    let content = toml::to_string_pretty(self)
      .map_err(|e| WardError::Config(e.to_string()))?;

    fs::write(&config_path, content)?;
    Ok(())
  }

  /// Adds a drive UUID to the allow list.
  pub fn allow_drive(&mut self, uuid: &str) {
    self.allowed_drives.insert(uuid.to_string());
  }

  /// Removes a drive UUID from the allow list.
  pub fn disallow_drive(&mut self, uuid: &str) {
    self.allowed_drives.remove(uuid);
  }

  /// Checks if a drive UUID is allowed.
  #[must_use]
  pub fn is_drive_allowed(&self, uuid: &str) -> bool {
    self.allowed_drives.contains(uuid)
  }
}

/// Returns the Ward configuration directory.
#[must_use]
pub fn config_dir() -> PathBuf {
  dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("ward")
}

/// Returns the path to the Ward configuration file.
#[must_use]
pub fn config_file() -> PathBuf {
  config_dir().join("config.toml")
}

/// Returns the path to the Ward state file (for daemon state).
#[must_use]
pub fn state_file() -> PathBuf {
  config_dir().join("state.toml")
}

/// Returns the default GPG home directory for the current platform.
#[must_use]
pub fn gpg_home() -> PathBuf {
  #[cfg(windows)]
  {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("gnupg")
  }

  #[cfg(not(windows))]
  {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".gnupg")
  }
}

/// Daemon state persisted across restarts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonState {
  /// UUID of the currently active Ward drive, if any.
  pub active_drive_uuid: Option<String>,

  /// Mount path of the active drive.
  pub active_mount_path: Option<PathBuf>,

  /// Path to the current RAM storage (Linux) or direct path (Windows).
  pub current_gpg_home: Option<PathBuf>,

  /// Path to the backup of the original GPG home.
  pub backup_path: Option<PathBuf>,
}

impl DaemonState {
  /// Loads the daemon state from disk.
  ///
  /// # Errors
  ///
  /// Returns an error if the state file exists but cannot be read.
  pub fn load() -> WardResult<Self> {
    let state_path = state_file();

    if !state_path.exists() {
      return Ok(Self::default());
    }

    let content = fs::read_to_string(&state_path)?;
    toml::from_str(&content).map_err(|e| WardError::Config(e.to_string()))
  }

  /// Saves the daemon state to disk.
  ///
  /// # Errors
  ///
  /// Returns an error if the state file cannot be written.
  pub fn save(&self) -> WardResult<()> {
    let state_path = state_file();

    if let Some(parent) = state_path.parent() {
      fs::create_dir_all(parent)?;
    }

    let content = toml::to_string_pretty(self)
      .map_err(|e| WardError::Config(e.to_string()))?;

    fs::write(&state_path, content)?;
    Ok(())
  }

  /// Clears the active drive state.
  pub fn clear_active(&mut self) {
    self.active_drive_uuid = None;
    self.active_mount_path = None;
    self.current_gpg_home = None;
    self.backup_path = None;
  }

  /// Returns true if a Ward drive is currently active.
  #[must_use]
  pub fn is_active(&self) -> bool {
    self.active_drive_uuid.is_some()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_config_default() {
    let config = Config::default();
    assert!(config.allowed_drives.is_empty());
    assert_eq!(config.poll_interval_ms, 1000);
  }

  #[test]
  fn test_config_allow_drive() {
    let mut config = Config::default();
    let uuid = "test-uuid-1234";

    assert!(!config.is_drive_allowed(uuid));
    config.allow_drive(uuid);
    assert!(config.is_drive_allowed(uuid));
    config.disallow_drive(uuid);
    assert!(!config.is_drive_allowed(uuid));
  }

  #[test]
  fn test_daemon_state() {
    let mut state = DaemonState::default();
    assert!(!state.is_active());

    state.active_drive_uuid = Some("test-uuid".to_string());
    assert!(state.is_active());

    state.clear_active();
    assert!(!state.is_active());
  }
}
