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

#[derive(Debug, Clone)]
pub struct RemovableDrive {
  pub mount_path: PathBuf,
  pub name: Option<String>,
  pub total_bytes: u64,
  pub is_ward_drive: bool,
}

#[derive(Debug, Clone)]
pub enum DriveEvent {
  Connected(RemovableDrive),
  Disconnected(PathBuf),
  WardDetected(WardDrive),
  WardRemoved(PathBuf),
}

pub type DriveEventCallback = Box<dyn Fn(DriveEvent) + Send + Sync>;

pub struct DriveMonitor {
  poll_interval: Duration,
  stop_signal: Arc<AtomicBool>,
  #[allow(dead_code)]
  known_mounts: HashSet<PathBuf>,
}

impl DriveMonitor {
  #[must_use]
  pub fn new(poll_interval_ms: u64) -> Self {
    Self {
      poll_interval: Duration::from_millis(poll_interval_ms),
      stop_signal: Arc::new(AtomicBool::new(false)),
      known_mounts: HashSet::new(),
    }
  }

  #[must_use]
  pub fn scan_removable_drives() -> Vec<RemovableDrive> {
    let disks = Disks::new_with_refreshed_list();
    let mut drives = Vec::new();
    for disk in disks.list() {
      if !Self::is_removable(disk) {
        continue;
      }
      let mount_path = disk.mount_point().to_path_buf();
      let name = disk.name().to_string_lossy().into_owned();
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

  fn is_removable(disk: &sysinfo::Disk) -> bool {
    matches!(
      disk.kind(),
      sysinfo::DiskKind::Unknown(_) | sysinfo::DiskKind::SSD
    ) || Self::looks_like_usb(disk)
  }

  fn looks_like_usb(disk: &sysinfo::Disk) -> bool {
    let mount_point = disk.mount_point();
    #[cfg(target_os = "linux")]
    {
      let path_str = mount_point.to_string_lossy();
      path_str.starts_with("/media/")
        || path_str.starts_with("/mnt/")
        || path_str.starts_with("/run/media/")
    }
    #[cfg(target_os = "windows")]
    {
      let path_str = mount_point.to_string_lossy();
      !path_str.starts_with("C:")
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
      let path_str = mount_point.to_string_lossy();
      path_str.starts_with("/Volumes/")
    }
  }

  pub fn find_ward_drive() -> WardResult<Option<WardDrive>> {
    let drives = Self::scan_removable_drives();
    for drive in drives {
      if let Ok(ward_drive) = WardDrive::detect(&drive.mount_path) {
        return Ok(Some(ward_drive));
      }
    }
    Ok(None)
  }

  #[must_use]
  pub fn is_mount_available(mount_path: &std::path::Path) -> bool {
    mount_path.exists() && mount_path.is_dir()
  }

  pub fn start<F>(&mut self, callback: F) -> StopHandle
  where
    F: Fn(DriveEvent) + Send + Sync + 'static,
  {
    let stop_signal = Arc::clone(&self.stop_signal);
    let poll_interval = self.poll_interval;
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
        for drive in &current_drives {
          if !known_mounts.contains(&drive.mount_path) {
            tracing::info!("New drive detected: {:?}", drive.mount_path);
            callback(DriveEvent::Connected(drive.clone()));
            if let Ok(ward_drive) = WardDrive::detect(&drive.mount_path) {
              tracing::info!(
                "Ward drive detected: {}",
                ward_drive.manifest.uuid
              );
              callback(DriveEvent::WardDetected(ward_drive));
            }
          }
        }
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

  pub fn stop(&self) {
    self.stop_signal.store(true, Ordering::Relaxed);
  }
}

pub struct StopHandle {
  stop_signal: Arc<AtomicBool>,
}

impl StopHandle {
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
    let drives = DriveMonitor::scan_removable_drives();
    println!("Found {} drives", drives.len());
  }

  #[test]
  fn test_find_ward_drive_no_drive() {
    let result = DriveMonitor::find_ward_drive();
    assert!(result.is_ok());
  }
}
