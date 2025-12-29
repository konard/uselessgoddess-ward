//! Ward daemon state machine and core logic.

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

/// The current state of the Ward daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WardState {
  /// Daemon is watching for removable drives.
  Idle,

  /// A mount was detected, scanning for Ward configuration.
  Scanning,

  /// Validating the Ward drive against the allow list.
  Validating,

  /// Activating the Ward drive (swapping GPG configuration).
  Activating,

  /// A Ward drive is active and being monitored.
  Active,

  /// The Ward drive was removed, cleaning up.
  Deactivating,

  /// Daemon is shutting down.
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

/// The Ward daemon.
pub struct WardDaemon {
  /// Current daemon state.
  state: Arc<Mutex<WardState>>,

  /// Configuration.
  config: Config,

  /// Persistent daemon state.
  daemon_state: Arc<Mutex<DaemonState>>,

  /// Key storage backend.
  storage: Box<dyn KeyStorage>,

  /// GPG manager.
  gpg_manager: GpgManager,

  /// Stop signal.
  stop_signal: Arc<AtomicBool>,
}

impl WardDaemon {
  /// Creates a new Ward daemon.
  ///
  /// # Errors
  ///
  /// Returns an error if configuration cannot be loaded.
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

  /// Returns the current daemon state.
  #[must_use]
  pub fn state(&self) -> WardState {
    *self.state.lock().unwrap()
  }

  /// Returns a reference to the daemon state data.
  pub fn daemon_state(&self) -> std::sync::MutexGuard<'_, DaemonState> {
    self.daemon_state.lock().unwrap()
  }

  /// Signals the daemon to stop.
  pub fn signal_stop(&self) {
    self.stop_signal.store(true, Ordering::Relaxed);
  }

  /// Checks if stop was signaled.
  fn should_stop(&self) -> bool {
    self.stop_signal.load(Ordering::Relaxed)
  }

  /// Sets the daemon state.
  fn set_state(&self, new_state: WardState) {
    let mut state = self.state.lock().unwrap();
    tracing::info!("State transition: {} -> {}", *state, new_state);
    *state = new_state;
  }

  /// Handles a detected Ward drive.
  fn handle_ward_detected(&self, drive: WardDrive) -> WardResult<()> {
    self.set_state(WardState::Validating);

    // Check if this drive is authorized
    if !self.config.is_drive_allowed(&drive.manifest.uuid) {
      tracing::warn!(
        "Drive {} ({}) is not authorized",
        drive.manifest.uuid,
        drive.manifest.label
      );
      self.set_state(WardState::Idle);
      return Err(WardError::UnauthorizedDrive { uuid: drive.manifest.uuid });
    }

    // Verify integrity
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

  /// Activates a Ward drive.
  fn activate_drive(&self, drive: &WardDrive) -> WardResult<()> {
    self.set_state(WardState::Activating);

    // Activate storage (copy to RAM on Linux, direct on Windows)
    let active_path = self.storage.activate(&drive.keys_dir)?;

    // Swap GPG configuration
    self.gpg_manager.swap_to(&active_path)?;

    // Update daemon state
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

  /// Handles Ward drive removal.
  #[allow(dead_code)]
  fn handle_ward_removed(&self, mount_path: &PathBuf) -> WardResult<()> {
    let state = self.daemon_state.lock().unwrap();

    // Check if this was our active drive
    if state.active_mount_path.as_ref() != Some(mount_path) {
      return Ok(());
    }
    drop(state);

    tracing::warn!("Active Ward drive removed!");
    self.deactivate()?;

    Ok(())
  }

  /// Deactivates the current Ward drive.
  pub fn deactivate(&self) -> WardResult<()> {
    self.set_state(WardState::Deactivating);

    // Swap GPG configuration back
    self.gpg_manager.swap_back()?;

    // Deactivate storage (secure wipe on Linux)
    self.storage.deactivate()?;

    // Clear daemon state
    {
      let mut state = self.daemon_state.lock().unwrap();
      state.clear_active();
      state.save()?;
    }

    self.set_state(WardState::Idle);

    tracing::info!("Ward drive deactivated");

    Ok(())
  }

  /// Runs the daemon event loop.
  ///
  /// This blocks until `signal_stop()` is called.
  pub fn run(&self) -> WardResult<()> {
    tracing::info!("Ward daemon starting");

    // Check for existing active state (recovery from crash)
    {
      let state = self.daemon_state.lock().unwrap();
      if state.is_active() {
        tracing::warn!(
          "Recovering from previous active state: {:?}",
          state.active_drive_uuid
        );
        // Check if the drive is still present
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

    // Initial scan for Ward drives
    if let Some(drive) = DriveMonitor::find_ward_drive()? {
      if self.config.is_drive_allowed(&drive.manifest.uuid) {
        tracing::info!("Found authorized Ward drive on startup");
        let _ = self.handle_ward_detected(drive);
      }
    }

    // Main event loop
    let poll_interval = Duration::from_millis(self.config.poll_interval_ms);

    while !self.should_stop() {
      thread::sleep(poll_interval);

      match self.state() {
        WardState::Idle => {
          // Look for new Ward drives
          if let Some(drive) = DriveMonitor::find_ward_drive()? {
            if self.config.is_drive_allowed(&drive.manifest.uuid) {
              let _ = self.handle_ward_detected(drive);
            }
          }
        }
        WardState::Active => {
          // Check if the active drive is still present
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

    // Clean shutdown
    self.set_state(WardState::Stopping);

    if self.state() == WardState::Active {
      self.deactivate()?;
    }

    tracing::info!("Ward daemon stopped");

    Ok(())
  }
}

/// Status information for display.
#[derive(Debug, Clone)]
pub struct DaemonStatus {
  /// Current state.
  pub state: WardState,

  /// Active drive UUID, if any.
  pub active_drive_uuid: Option<String>,

  /// Active mount path, if any.
  pub active_mount_path: Option<PathBuf>,

  /// Current GPG home path.
  pub current_gpg_home: Option<PathBuf>,
}

impl DaemonStatus {
  /// Creates a status from daemon state.
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
