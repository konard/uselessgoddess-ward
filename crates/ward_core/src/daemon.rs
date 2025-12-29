use std::{
  path::PathBuf,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  thread,
  time::Duration,
};

use crate::{
  config::{Config, DaemonState},
  error::{WardError, WardResult},
  gpg::GpgManager,
  manifest::WardDrive,
  monitor::DriveMonitor,
  storage::{KeyStorage, create_platform_storage},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WardState {
  Idle,
  Scanning,
  Validating,
  Activating,
  Active,
  Deactivating,
  Stopping,
}

impl std::fmt::Display for WardState {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Idle => write!(f, "idle"),
      Self::Scanning => write!(f, "scanning"),
      Self::Validating => write!(f, "validating"),
      Self::Activating => write!(f, "activating"),
      Self::Active => write!(f, "active"),
      Self::Deactivating => write!(f, "deactivating"),
      Self::Stopping => write!(f, "stopping"),
    }
  }
}

pub struct WardDaemon {
  state: Arc<Mutex<WardState>>,
  config: Config,
  daemon_state: Arc<Mutex<DaemonState>>,
  storage: Box<dyn KeyStorage>,
  gpg_manager: GpgManager,
  stop_signal: Arc<AtomicBool>,
}

impl WardDaemon {
  pub fn new() -> WardResult<Self> {
    let config = Config::load()?;
    let daemon_state = DaemonState::load()?;
    let storage = create_platform_storage();
    let gpg_manager = GpgManager::new(config.gpg_backup_path.clone());
    Ok(Self {
      state: Arc::new(Mutex::new(WardState::Idle)),
      config,
      daemon_state: Arc::new(Mutex::new(daemon_state)),
      storage,
      gpg_manager,
      stop_signal: Arc::new(AtomicBool::new(false)),
    })
  }

  #[must_use]
  pub fn state(&self) -> WardState {
    *self.state.lock().unwrap()
  }

  pub fn daemon_state(&self) -> std::sync::MutexGuard<'_, DaemonState> {
    self.daemon_state.lock().unwrap()
  }

  pub fn signal_stop(&self) {
    self.stop_signal.store(true, Ordering::Relaxed);
  }

  fn should_stop(&self) -> bool {
    self.stop_signal.load(Ordering::Relaxed)
  }

  fn set_state(&self, new_state: WardState) {
    let mut state = self.state.lock().unwrap();
    tracing::info!("State transition: {} -> {}", *state, new_state);
    *state = new_state;
  }

  fn handle_ward_detected(&self, drive: WardDrive) -> WardResult<()> {
    self.set_state(WardState::Validating);
    if !self.config.is_drive_allowed(&drive.manifest.uuid) {
      tracing::warn!(
        "Drive {} ({}) is not authorized",
        drive.manifest.uuid,
        drive.manifest.label
      );
      self.set_state(WardState::Idle);
      return Err(WardError::UnauthorizedDrive { uuid: drive.manifest.uuid });
    }
    if let Err(e) = drive.manifest.verify_integrity(&drive.ward_dir) {
      tracing::error!("Integrity check failed: {}", e);
      self.set_state(WardState::Idle);
      return Err(e);
    }
    tracing::info!(
      "Authorized Ward drive detected: {} ({})",
      drive.manifest.label,
      drive.manifest.uuid
    );
    self.activate_drive(&drive)?;
    Ok(())
  }

  fn activate_drive(&self, drive: &WardDrive) -> WardResult<()> {
    self.set_state(WardState::Activating);
    let active_path = self.storage.activate(&drive.keys_dir)?;
    self.gpg_manager.swap_to(&active_path)?;
    {
      let mut state = self.daemon_state.lock().unwrap();
      state.active_drive_uuid = Some(drive.manifest.uuid.clone());
      state.active_mount_path = Some(drive.mount_path.clone());
      state.current_gpg_home = Some(active_path);
      state.backup_path = Some(self.config.gpg_backup_path.clone());
      state.save()?;
    }
    self.set_state(WardState::Active);
    tracing::info!(
      "Ward drive {} activated successfully",
      drive.manifest.label
    );
    Ok(())
  }

  #[allow(dead_code)]
  fn handle_ward_removed(&self, mount_path: &PathBuf) -> WardResult<()> {
    let state = self.daemon_state.lock().unwrap();
    if state.active_mount_path.as_ref() != Some(mount_path) {
      return Ok(());
    }
    drop(state);
    tracing::warn!("Active Ward drive removed!");
    self.deactivate()?;
    Ok(())
  }

  pub fn deactivate(&self) -> WardResult<()> {
    self.set_state(WardState::Deactivating);
    self.gpg_manager.swap_back()?;
    self.storage.deactivate()?;
    {
      let mut state = self.daemon_state.lock().unwrap();
      state.clear_active();
      state.save()?;
    }
    self.set_state(WardState::Idle);
    tracing::info!("Ward drive deactivated");
    Ok(())
  }

  pub fn run(&self) -> WardResult<()> {
    tracing::info!("Ward daemon starting");
    {
      let state = self.daemon_state.lock().unwrap();
      if state.is_active() {
        tracing::warn!(
          "Recovering from previous active state: {:?}",
          state.active_drive_uuid
        );
        if let Some(ref mount_path) = state.active_mount_path {
          if !DriveMonitor::is_mount_available(mount_path) {
            tracing::warn!(
              "Previous active drive is no longer present, deactivating"
            );
            drop(state);
            self.deactivate()?;
          }
        }
      }
    }
    if let Some(drive) = DriveMonitor::find_ward_drive()? {
      if self.config.is_drive_allowed(&drive.manifest.uuid) {
        tracing::info!("Found authorized Ward drive on startup");
        let _ = self.handle_ward_detected(drive);
      }
    }
    let poll_interval = Duration::from_millis(self.config.poll_interval_ms);
    while !self.should_stop() {
      thread::sleep(poll_interval);
      match self.state() {
        WardState::Idle => {
          if let Some(drive) = DriveMonitor::find_ward_drive()? {
            if self.config.is_drive_allowed(&drive.manifest.uuid) {
              let _ = self.handle_ward_detected(drive);
            }
          }
        }
        WardState::Active => {
          let state = self.daemon_state.lock().unwrap();
          if let Some(ref mount_path) = state.active_mount_path {
            if !DriveMonitor::is_mount_available(mount_path) {
              drop(state);
              let _ = self.deactivate();
            }
          }
        }
        _ => {}
      }
    }
    self.set_state(WardState::Stopping);
    if self.state() == WardState::Active {
      self.deactivate()?;
    }
    tracing::info!("Ward daemon stopped");
    Ok(())
  }
}

#[derive(Debug, Clone)]
pub struct DaemonStatus {
  pub state: WardState,
  pub active_drive_uuid: Option<String>,
  pub active_mount_path: Option<PathBuf>,
  pub current_gpg_home: Option<PathBuf>,
}

impl DaemonStatus {
  #[must_use]
  pub fn from_state(daemon_state: &DaemonState) -> Self {
    Self {
      state: if daemon_state.is_active() {
        WardState::Active
      } else {
        WardState::Idle
      },
      active_drive_uuid: daemon_state.active_drive_uuid.clone(),
      active_mount_path: daemon_state.active_mount_path.clone(),
      current_gpg_home: daemon_state.current_gpg_home.clone(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_ward_state_display() {
    assert_eq!(WardState::Idle.to_string(), "idle");
    assert_eq!(WardState::Active.to_string(), "active");
  }
}
