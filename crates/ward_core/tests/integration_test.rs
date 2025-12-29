//! Integration tests for Ward core functionality.
//!
//! These tests use a mock filesystem to simulate USB drive detection
//! without requiring actual hardware.

use std::fs;
use tempfile::TempDir;
use ward_core::{
  Config, Manifest, WardDrive,
  manifest::{KEYS_DIR, MANIFEST_FILE, WARD_DIR},
};

/// Creates a mock USB drive structure for testing.
fn create_mock_usb(temp_dir: &TempDir, label: &str) -> std::path::PathBuf {
  let mount_path = temp_dir.path().to_path_buf();
  let ward_dir = mount_path.join(WARD_DIR);
  let keys_dir = ward_dir.join(KEYS_DIR);

  fs::create_dir_all(&keys_dir).unwrap();

  // Create a mock GPG key file
  fs::write(keys_dir.join("pubring.kbx"), b"mock public key data").unwrap();
  fs::write(keys_dir.join("trustdb.gpg"), b"mock trust database").unwrap();

  // Create mock private keys directory
  let private_keys_dir = keys_dir.join("private-keys-v1.d");
  fs::create_dir_all(&private_keys_dir).unwrap();
  fs::write(private_keys_dir.join("mock-key.key"), b"mock private key data")
    .unwrap();

  // Create manifest
  let mut manifest = Manifest::new(label);
  manifest.update_checksum(&ward_dir).unwrap();
  manifest.save(&ward_dir).unwrap();

  mount_path
}

#[test]
fn test_mock_ward_drive_detection() {
  let temp_dir = TempDir::new().unwrap();
  let mount_path = create_mock_usb(&temp_dir, "Test Drive");

  // Test that Ward detects the mock drive
  let drive = WardDrive::detect(&mount_path).unwrap();

  assert_eq!(drive.manifest.label, "Test Drive");
  assert!(!drive.manifest.uuid.is_empty());
  assert!(drive.keys_dir.exists());
}

#[test]
fn test_ward_drive_init_creates_structure() {
  let temp_dir = TempDir::new().unwrap();
  let mount_path = temp_dir.path();

  // Initialize a new Ward drive
  let drive = WardDrive::init(mount_path, "New Drive").unwrap();

  // Verify structure was created
  assert!(drive.ward_dir.exists());
  assert!(drive.keys_dir.exists());
  assert!(drive.ward_dir.join(MANIFEST_FILE).exists());

  // Verify manifest contents
  let manifest = Manifest::load(&drive.ward_dir).unwrap();
  assert_eq!(manifest.label, "New Drive");
}

#[test]
fn test_manifest_integrity_check() {
  let temp_dir = TempDir::new().unwrap();
  let mount_path = create_mock_usb(&temp_dir, "Integrity Test");

  let drive = WardDrive::detect(&mount_path).unwrap();

  // Integrity should pass initially
  assert!(drive.manifest.verify_integrity(&drive.ward_dir).is_ok());

  // Modify a file
  let key_file = drive.keys_dir.join("pubring.kbx");
  fs::write(&key_file, b"tampered data").unwrap();

  // Integrity should now fail
  assert!(drive.manifest.verify_integrity(&drive.ward_dir).is_err());
}

#[test]
fn test_config_allow_list() {
  let mut config = Config::default();
  let uuid = "test-uuid-12345";

  // Initially not allowed
  assert!(!config.is_drive_allowed(uuid));

  // Allow the drive
  config.allow_drive(uuid);
  assert!(config.is_drive_allowed(uuid));

  // Disallow the drive
  config.disallow_drive(uuid);
  assert!(!config.is_drive_allowed(uuid));
}

#[test]
fn test_manifest_serialization_roundtrip() {
  let temp_dir = TempDir::new().unwrap();
  let ward_dir = temp_dir.path().join(WARD_DIR);
  fs::create_dir_all(&ward_dir).unwrap();

  let original = Manifest::new("Roundtrip Test");
  original.save(&ward_dir).unwrap();

  let loaded = Manifest::load(&ward_dir).unwrap();

  assert_eq!(original.uuid, loaded.uuid);
  assert_eq!(original.label, loaded.label);
}

#[test]
fn test_ward_drive_detection_fails_without_manifest() {
  let temp_dir = TempDir::new().unwrap();
  let mount_path = temp_dir.path();

  // Create the .ward directory but no manifest
  fs::create_dir_all(mount_path.join(WARD_DIR).join(KEYS_DIR)).unwrap();

  // Detection should fail
  assert!(WardDrive::detect(mount_path).is_err());
}

#[test]
fn test_ward_drive_detection_fails_without_keys_dir() {
  let temp_dir = TempDir::new().unwrap();
  let mount_path = temp_dir.path();
  let ward_dir = mount_path.join(WARD_DIR);

  // Create manifest without keys directory
  fs::create_dir_all(&ward_dir).unwrap();
  let manifest = Manifest::new("No Keys");
  manifest.save(&ward_dir).unwrap();

  // Detection should fail because keys dir is missing
  assert!(WardDrive::detect(mount_path).is_err());
}

#[test]
fn test_multiple_ward_drives() {
  let temp_dir1 = TempDir::new().unwrap();
  let temp_dir2 = TempDir::new().unwrap();

  let mount1 = create_mock_usb(&temp_dir1, "Drive 1");
  let mount2 = create_mock_usb(&temp_dir2, "Drive 2");

  let drive1 = WardDrive::detect(&mount1).unwrap();
  let drive2 = WardDrive::detect(&mount2).unwrap();

  // UUIDs should be different
  assert_ne!(drive1.manifest.uuid, drive2.manifest.uuid);

  // Labels should match what we set
  assert_eq!(drive1.manifest.label, "Drive 1");
  assert_eq!(drive2.manifest.label, "Drive 2");
}

#[test]
fn test_path_traversal_prevention() {
  let temp_dir = TempDir::new().unwrap();
  let mount_path = temp_dir.path();

  // Try to detect with a path traversal attempt
  // canonicalize should prevent this
  let malicious_path = mount_path.join("..");

  // This should either fail or resolve to the parent safely
  // The key is that it doesn't allow accessing outside the expected path
  let result = WardDrive::detect(&malicious_path);
  // We just verify it doesn't panic and handles the case gracefully
  assert!(result.is_err());
}
