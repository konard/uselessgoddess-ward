//! Ward Core - GPG key manager using USB drives as security tokens.
//!
//! This crate provides the core functionality for the Ward system:
//!
//! - Manifest handling for Ward USB drives
//! - Configuration management
//! - Key storage abstraction (RAM-based for Linux, direct for Windows)
//! - GPG agent management
//! - Drive detection and monitoring
//! - Daemon state machine

pub mod config;
pub mod daemon;
pub mod error;
pub mod gpg;
pub mod manifest;
pub mod monitor;
pub mod storage;

// Re-exports for convenience
pub use config::{Config, DaemonState};
pub use daemon::{DaemonStatus, WardDaemon, WardState};
pub use error::{WardError, WardResult};
pub use manifest::{KEYS_DIR, MANIFEST_FILE, Manifest, WARD_DIR, WardDrive};
pub use monitor::{DriveEvent, DriveMonitor, RemovableDrive};
pub use storage::{KeyStorage, create_platform_storage};
