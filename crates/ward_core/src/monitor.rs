//! Drive detection and monitoring.

use std::{
  collections::HashSet,
  path::PathBuf,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  thread,
  time::Duration,
};

use sysinfo::Disks;

use crate::{error::WardResult, manifest::WardDrive};

/// Represents a removable drive detected on the system.
#[derive(Debug, Clone)]
pub struct RemovableDrive {
  /// Mount point path.
  pub mount_path: PathBuf,

  /// Drive name/label (if available).
  pub name: Option<String>,

  /// Total size in bytes.
  pub total_bytes: u64,

  /// Whether this drive contains a Ward configuration.
  pub is_ward_drive: bool,
}

/// Event types for drive monitoring.
#[derive(Debug, Clone)]
pub enum DriveEvent {
  /// A new drive was connected.
  Connected(RemovableDrive),

  /// A drive was disconnected.
  Disconnected(PathBuf),

  /// A Ward drive was detected.
  WardDetected(WardDrive),

  /// A Ward drive was removed.
  WardRemoved(PathBuf),
}

/// Callback type for drive events.
pub type DriveEventCallback = Box<dyn Fn(DriveEvent) + Send + Sync>;

/// Drive monitor that watches for USB drive connections/disconnections.
pub struct DriveMonitor {
  /// Polling interval.
  poll_interval: Duration,

  /// Stop signal for the monitor thread.
  stop_signal: Arc<AtomicBool>,

  /// Currently known mount points (used by the background thread).
  #[allow(dead_code)]
  known_mounts: HashSet<PathBuf>,
}

impl DriveMonitor {
  /// Creates a new drive monitor with the specified polling interval.
  #[must_use]
  pub fn new(poll_interval_ms: u64) -> Self {
    Self {
      poll_interval: Duration::from_millis(poll_interval_ms),
      stop_signal: Arc::new(AtomicBool::new(false)),
      known_mounts: HashSet::new(),
    }
  }

  /// Scans for removable drives on the system.
  #[must_use]
  pub fn scan_removable_drives() -> Vec<RemovableDrive> {
    let disks = Disks::new_with_refreshed_list();
    let mut drives = Vec::new();

    for disk in disks.list() {
      // Check if it's a removable drive
      if !Self::is_removable(disk) {
        continue;
      }

      let mount_path = disk.mount_point().to_path_buf();
      let name = disk.name().to_string_lossy().into_owned();

      // Check if it's a Ward drive
      let is_ward_drive = WardDrive::detect(&mount_path).is_ok();

      drives.push(RemovableDrive {
        mount_path,
        name: if name.is_empty() { None } else { Some(name) },
        total_bytes: disk.total_space(),
        is_ward_drive,
      });
    }

    drives
  }

  /// Checks if a disk is removable.
  fn is_removable(disk: &sysinfo::Disk) -> bool {
    // sysinfo doesn't directly expose "removable" flag,
    // so we use heuristics based on the disk kind
    matches!(
      disk.kind(),
      sysinfo::DiskKind::Unknown(_) | sysinfo::DiskKind::SSD
    ) || Self::looks_like_usb(disk)
  }

  /// Heuristic to detect USB drives.
  fn looks_like_usb(disk: &sysinfo::Disk) -> bool {
    let mount_point = disk.mount_point();

    #[cfg(target_os = "linux")]
    {
      // On Linux, USB drives are often mounted under /media or /mnt
      let path_str = mount_point.to_string_lossy();
      path_str.starts_with("/media/")
        || path_str.starts_with("/mnt/")
        || path_str.starts_with("/run/media/")
    }

    #[cfg(target_os = "windows")]
    {
      // On Windows, check if it's not a system drive
      let path_str = mount_point.to_string_lossy();
      !path_str.starts_with("C:")
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
      // On other systems, assume external if mounted under /Volumes (macOS)
      let path_str = mount_point.to_string_lossy();
      path_str.starts_with("/Volumes/")
    }
  }

  /// Finds a Ward drive among the currently mounted drives.
  pub fn find_ward_drive() -> WardResult<Option<WardDrive>> {
    let drives = Self::scan_removable_drives();

    for drive in drives {
      if let Ok(ward_drive) = WardDrive::detect(&drive.mount_path) {
        return Ok(Some(ward_drive));
      }
    }

    Ok(None)
  }

  /// Checks if a specific mount path is still available.
  #[must_use]
  pub fn is_mount_available(mount_path: &std::path::Path) -> bool {
    mount_path.exists() && mount_path.is_dir()
  }

  /// Starts the drive monitor in a background thread.
  ///
  /// Returns a stop handle that can be used to stop the monitor.
  pub fn start<F>(&mut self, callback: F) -> StopHandle
  where
    F: Fn(DriveEvent) + Send + Sync + 'static,
  {
    let stop_signal = Arc::clone(&self.stop_signal);
    let poll_interval = self.poll_interval;

    // Initial scan
    let mut known_mounts: HashSet<PathBuf> =
      Self::scan_removable_drives().into_iter().map(|d| d.mount_path).collect();

    let callback = Arc::new(callback);

    thread::spawn(move || {
      tracing::info!("Drive monitor started with {:?} interval", poll_interval);

      while !stop_signal.load(Ordering::Relaxed) {
        thread::sleep(poll_interval);

        let current_drives = Self::scan_removable_drives();
        let current_mounts: HashSet<PathBuf> =
          current_drives.iter().map(|d| d.mount_path.clone()).collect();

        // Check for new drives
        for drive in &current_drives {
          if !known_mounts.contains(&drive.mount_path) {
            tracing::info!("New drive detected: {:?}", drive.mount_path);
            callback(DriveEvent::Connected(drive.clone()));

            // Check if it's a Ward drive
            if let Ok(ward_drive) = WardDrive::detect(&drive.mount_path) {
              tracing::info!(
                "Ward drive detected: {}",
                ward_drive.manifest.uuid
              );
              callback(DriveEvent::WardDetected(ward_drive));
            }
          }
        }

        // Check for removed drives
        for mount in &known_mounts {
          if !current_mounts.contains(mount) {
            tracing::info!("Drive removed: {:?}", mount);
            callback(DriveEvent::Disconnected(mount.clone()));
            callback(DriveEvent::WardRemoved(mount.clone()));
          }
        }

        known_mounts = current_mounts;
      }

      tracing::info!("Drive monitor stopped");
    });

    StopHandle { stop_signal: Arc::clone(&self.stop_signal) }
  }

  /// Stops the drive monitor.
  pub fn stop(&self) {
    self.stop_signal.store(true, Ordering::Relaxed);
  }
}

/// Handle to stop the drive monitor.
pub struct StopHandle {
  stop_signal: Arc<AtomicBool>,
}

impl StopHandle {
  /// Stops the monitor.
  pub fn stop(&self) {
    self.stop_signal.store(true, Ordering::Relaxed);
  }
}

impl Drop for StopHandle {
  fn drop(&mut self) {
    self.stop();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_scan_removable_drives() {
    // This test just ensures the function doesn't panic
    let drives = DriveMonitor::scan_removable_drives();
    // We can't assert on the result since it depends on the system
    println!("Found {} drives", drives.len());
  }

  #[test]
  fn test_find_ward_drive_no_drive() {
    // Without a real Ward drive, this should return None
    let result = DriveMonitor::find_ward_drive();
    assert!(result.is_ok());
    // Note: might find a Ward drive on the test system, so we don't assert None
  }
}
