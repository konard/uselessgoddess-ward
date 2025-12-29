use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use ward_core::WardDaemon;

#[derive(Parser, Debug)]
#[command(name = "ward")]
#[command(author, version, about, long_about = None)]
struct Args {
  #[arg(short, long)]
  foreground: bool,
  #[arg(short, long)]
  verbose: bool,
  #[arg(long, default_value = "info")]
  log_level: String,
}

fn main() -> Result<()> {
  let args = Args::parse();
  let log_level = if args.verbose { "debug" } else { &args.log_level };
  let filter = tracing_subscriber::EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));
  tracing_subscriber::registry()
    .with(filter)
    .with(tracing_subscriber::fmt::layer())
    .init();

  tracing::info!("Ward daemon starting");
  let daemon = Arc::new(WardDaemon::new()?);

  let daemon_clone = Arc::clone(&daemon);
  ctrlc::set_handler(move || {
    tracing::info!("Received shutdown signal");
    daemon_clone.signal_stop();
  })?;

  daemon.run()?;
  tracing::info!("Ward daemon stopped");
  Ok(())
}
