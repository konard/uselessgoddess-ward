//! Error types for Ward.

use std::path::PathBuf;

/// Errors that can occur during Ward operations.
#[derive(Debug, thiserror::Error)]
pub enum WardError {
  /// Failed to read or parse manifest file.
  #[error("manifest error at {path}: {message}")]
  Manifest { path: PathBuf, message: String },

  /// The drive UUID is not in the allow list.
  #[error("drive with UUID {uuid} is not authorized")]
  UnauthorizedDrive { uuid: String },

  /// Failed to perform filesystem operation.
  #[error("filesystem error: {0}")]
  Filesystem(#[from] std::io::Error),

  /// Failed to parse configuration.
  #[error("configuration error: {0}")]
  Config(String),

  /// GPG agent operation failed.
  #[error("gpg-agent error: {0}")]
  GpgAgent(String),

  /// Symlink operation failed.
  #[error("symlink error: {0}")]
  Symlink(String),

  /// Drive monitoring error.
  #[error("drive monitor error: {0}")]
  DriveMonitor(String),

  /// Validation checksum mismatch.
  #[error(
    "integrity check failed for {path}: expected {expected}, got {actual}"
  )]
  IntegrityCheck { path: PathBuf, expected: String, actual: String },
}

/// Result type for Ward operations.
pub type WardResult<T> = Result<T, WardError>;
