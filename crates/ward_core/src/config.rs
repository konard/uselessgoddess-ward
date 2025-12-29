use std::{collections::HashSet, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{WardError, WardResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
  #[serde(default)]
  pub allowed_drives: HashSet<String>,
  #[serde(default = "default_backup_path")]
  pub gpg_backup_path: PathBuf,
  #[serde(default = "default_poll_interval")]
  pub poll_interval_ms: u64,
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
  1000
}

impl Config {
  pub fn load() -> WardResult<Self> {
    let config_path = config_file();
    if !config_path.exists() {
      return Ok(Self::default());
    }
    let content = fs::read_to_string(&config_path)?;
    toml::from_str(&content).map_err(|e| WardError::Config(e.to_string()))
  }

  pub fn save(&self) -> WardResult<()> {
    let config_path = config_file();
    if let Some(parent) = config_path.parent() {
      fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(self)
      .map_err(|e| WardError::Config(e.to_string()))?;
    fs::write(&config_path, content)?;
    Ok(())
  }

  pub fn allow_drive(&mut self, uuid: &str) {
    self.allowed_drives.insert(uuid.to_string());
  }

  pub fn disallow_drive(&mut self, uuid: &str) {
    self.allowed_drives.remove(uuid);
  }

  #[must_use]
  pub fn is_drive_allowed(&self, uuid: &str) -> bool {
    self.allowed_drives.contains(uuid)
  }
}

#[must_use]
pub fn config_dir() -> PathBuf {
  dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("ward")
}

#[must_use]
pub fn config_file() -> PathBuf {
  config_dir().join("config.toml")
}

#[must_use]
pub fn state_file() -> PathBuf {
  config_dir().join("state.toml")
}

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonState {
  pub active_drive_uuid: Option<String>,
  pub active_mount_path: Option<PathBuf>,
  pub current_gpg_home: Option<PathBuf>,
  pub backup_path: Option<PathBuf>,
}

impl DaemonState {
  pub fn load() -> WardResult<Self> {
    let state_path = state_file();
    if !state_path.exists() {
      return Ok(Self::default());
    }
    let content = fs::read_to_string(&state_path)?;
    toml::from_str(&content).map_err(|e| WardError::Config(e.to_string()))
  }

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

  pub fn clear_active(&mut self) {
    self.active_drive_uuid = None;
    self.active_mount_path = None;
    self.current_gpg_home = None;
    self.backup_path = None;
  }

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
