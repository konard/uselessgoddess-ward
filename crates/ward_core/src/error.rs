use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum WardError {
  #[error("manifest error at {path}: {message}")]
  Manifest { path: PathBuf, message: String },
  #[error("drive with UUID {uuid} is not authorized")]
  UnauthorizedDrive { uuid: String },
  #[error("filesystem error: {0}")]
  Filesystem(#[from] std::io::Error),
  #[error("configuration error: {0}")]
  Config(String),
  #[error("gpg-agent error: {0}")]
  GpgAgent(String),
  #[error("symlink error: {0}")]
  Symlink(String),
  #[error("drive monitor error: {0}")]
  DriveMonitor(String),
  #[error(
    "integrity check failed for {path}: expected {expected}, got {actual}"
  )]
  IntegrityCheck { path: PathBuf, expected: String, actual: String },
}

pub type WardResult<T> = Result<T, WardError>;
