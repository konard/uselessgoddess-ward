//! Ward daemon - Background service for GPG key management.
//!
//! This daemon monitors for USB drives containing Ward configurations and
//! automatically swaps the system's GPG configuration when authorized drives
//! are connected or disconnected.

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use ward_core::WardDaemon;

/// Ward daemon - Automated GPG key manager.
#[derive(Parser, Debug)]
#[command(name = "ward")]
#[command(author, version, about, long_about = None)]
struct Args {
  /// Run in foreground mode (don't daemonize).
  #[arg(short, long)]
  foreground: bool,

  /// Enable verbose logging.
  #[arg(short, long)]
  verbose: bool,

  /// Log level (trace, debug, info, warn, error).
  #[arg(long, default_value = "info")]
  log_level: String,
}

fn main() -> Result<()> {
  let args = Args::parse();

  // Initialize logging
  let log_level = if args.verbose { "debug" } else { &args.log_level };

  let filter = tracing_subscriber::EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

  tracing_subscriber::registry()
    .with(filter)
    .with(tracing_subscriber::fmt::layer())
    .init();

  tracing::info!("Ward daemon starting");

  // Create and run the daemon
  let daemon = Arc::new(WardDaemon::new()?);

  // Set up signal handlers for graceful shutdown
  #[cfg(unix)]
  {
    let daemon_clone = Arc::clone(&daemon);
    ctrlc::set_handler(move || {
      tracing::info!("Received shutdown signal");
      daemon_clone.signal_stop();
    })?;
  }

  #[cfg(windows)]
  {
    let daemon_clone = Arc::clone(&daemon);
    ctrlc::set_handler(move || {
      tracing::info!("Received shutdown signal");
      daemon_clone.signal_stop();
    })?;
  }

  // Run the daemon
  daemon.run()?;

  tracing::info!("Ward daemon stopped");

  Ok(())
}
