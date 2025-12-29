use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use ward_core::{
  Config, DaemonStatus, DriveMonitor, WardDrive, config::DaemonState,
};

#[derive(Parser, Debug)]
#[command(name = "wardctl")]
#[command(author, version, about, long_about = None)]
struct Args {
  #[arg(short, long)]
  verbose: bool,
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
  Init {
    path: PathBuf,
    #[arg(short, long, default_value = "Ward Drive")]
    label: String,
  },
  Allow {
    path: Option<PathBuf>,
  },
  Disallow {
    uuid: String,
  },
  Status,
  List,
  Install,
  Uninstall,
  Config,
}

fn main() -> Result<()> {
  let args = Args::parse();

  // Initialize logging
  let log_level = if args.verbose { "debug" } else { "warn" };

  let filter = tracing_subscriber::EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

  tracing_subscriber::registry()
    .with(filter)
    .with(tracing_subscriber::fmt::layer())
    .init();

  match args.command {
    Command::Init { path, label } => cmd_init(&path, &label),
    Command::Allow { path } => cmd_allow(path.as_deref()),
    Command::Disallow { uuid } => cmd_disallow(&uuid),
    Command::Status => cmd_status(),
    Command::List => cmd_list(),
    Command::Install => cmd_install(),
    Command::Uninstall => cmd_uninstall(),
    Command::Config => cmd_config(),
  }
}

fn cmd_init(path: &PathBuf, label: &str) -> Result<()> {
  println!("Initializing Ward drive at {:?}...", path);

  let drive =
    WardDrive::init(path, label).context("Failed to initialize Ward drive")?;

  println!("Ward drive initialized successfully!");
  println!("  UUID:  {}", drive.manifest.uuid);
  println!("  Label: {}", drive.manifest.label);
  println!("  Path:  {:?}", drive.ward_dir);
  println!();
  println!("Next steps:");
  println!("  1. Copy your GPG keys to {:?}", drive.keys_dir);
  println!("  2. Run 'wardctl allow {:?}' to authorize this drive", path);

  Ok(())
}

fn cmd_allow(path: Option<&std::path::Path>) -> Result<()> {
  let drive = match path {
    Some(p) => WardDrive::detect(p).context("Failed to detect Ward drive")?,
    None => DriveMonitor::find_ward_drive()?
      .context("No Ward drive detected. Please specify a path.")?,
  };

  let mut config = Config::load()?;

  if config.is_drive_allowed(&drive.manifest.uuid) {
    println!("Drive is already authorized:");
  } else {
    config.allow_drive(&drive.manifest.uuid);
    config.save()?;
    println!("Drive authorized:");
  }

  println!("  UUID:  {}", drive.manifest.uuid);
  println!("  Label: {}", drive.manifest.label);

  Ok(())
}

fn cmd_disallow(uuid: &str) -> Result<()> {
  let mut config = Config::load()?;

  if !config.is_drive_allowed(uuid) {
    println!("Drive with UUID {} is not in the allow list.", uuid);
    return Ok(());
  }

  config.disallow_drive(uuid);
  config.save()?;

  println!("Drive with UUID {} has been removed from the allow list.", uuid);

  Ok(())
}

fn cmd_status() -> Result<()> {
  let state = DaemonState::load()?;
  let status = DaemonStatus::from_state(&state);

  println!("Ward Status");
  println!("===========");
  println!("State: {}", status.state);

  if let Some(uuid) = status.active_drive_uuid {
    println!();
    println!("Active Drive");
    println!("  UUID:      {}", uuid);
    if let Some(path) = status.active_mount_path {
      println!("  Mount:     {:?}", path);
    }
    if let Some(home) = status.current_gpg_home {
      println!("  GPG Home:  {:?}", home);
    }
  } else {
    println!();
    println!("No Ward drive is currently active.");
  }

  Ok(())
}

fn cmd_list() -> Result<()> {
  println!("Scanning for Ward drives...");
  println!();

  let drives = DriveMonitor::scan_removable_drives();
  let config = Config::load()?;

  let mut found = false;

  for drive in drives {
    if let Ok(ward_drive) = WardDrive::detect(&drive.mount_path) {
      found = true;
      let authorized = config.is_drive_allowed(&ward_drive.manifest.uuid);
      let status = if authorized { "[authorized]" } else { "[not authorized]" };

      println!(
        "{} - {} {}",
        ward_drive.manifest.label, ward_drive.manifest.uuid, status
      );
      println!("  Mount: {:?}", drive.mount_path);
      println!();
    }
  }

  if !found {
    println!("No Ward drives detected.");
    println!();
    println!("To create a new Ward drive:");
    println!("  wardctl init /path/to/usb-drive");
  }

  Ok(())
}

fn cmd_config() -> Result<()> {
  let config = Config::load()?;

  println!("Ward Configuration");
  println!("==================");
  println!("Config file:    {:?}", ward_core::config::config_file());
  println!("Poll interval:  {} ms", config.poll_interval_ms);
  println!("Backup path:    {:?}", config.gpg_backup_path);
  println!();
  println!("Authorized Drives ({}):", config.allowed_drives.len());

  if config.allowed_drives.is_empty() {
    println!("  (none)");
  } else {
    for uuid in &config.allowed_drives {
      println!("  - {}", uuid);
    }
  }

  Ok(())
}

fn cmd_install() -> Result<()> {
  #[cfg(target_os = "linux")]
  {
    install_systemd_service()?;
  }

  #[cfg(target_os = "windows")]
  {
    install_windows_service()?;
  }

  #[cfg(not(any(target_os = "linux", target_os = "windows")))]
  {
    println!(
      "Automatic service installation is not supported on this platform."
    );
    println!(
      "Please run the ward daemon manually or set up a service using your system's"
    );
    println!("service manager.");
  }

  Ok(())
}

fn cmd_uninstall() -> Result<()> {
  #[cfg(target_os = "linux")]
  {
    uninstall_systemd_service()?;
  }

  #[cfg(target_os = "windows")]
  {
    uninstall_windows_service()?;
  }

  #[cfg(not(any(target_os = "linux", target_os = "windows")))]
  {
    println!(
      "Automatic service uninstallation is not supported on this platform."
    );
  }

  Ok(())
}

#[cfg(target_os = "linux")]
fn install_systemd_service() -> Result<()> {
  use std::env;

  let exe_path = env::current_exe()?;
  let ward_path = exe_path
    .parent()
    .unwrap_or(std::path::Path::new("/usr/local/bin"))
    .join("ward");

  println!("Linux service installation:");
  println!();
  println!("Create ~/.config/systemd/user/ward.service with:");
  println!();
  println!("[Unit]");
  println!("Description=Ward GPG Key Manager");
  println!("After=network.target");
  println!();
  println!("[Service]");
  println!("Type=simple");
  println!("ExecStart={}", ward_path.display());
  println!("Restart=on-failure");
  println!("RestartSec=5");
  println!();
  println!("[Install]");
  println!("WantedBy=multi-user.target");
  println!();
  println!("Then run:");
  println!("  systemctl --user daemon-reload");
  println!("  systemctl --user enable ward");
  println!("  systemctl --user start ward");

  Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_systemd_service() -> Result<()> {
  println!("Linux service uninstallation:");
  println!();
  println!("Run:");
  println!("  systemctl --user stop ward");
  println!("  systemctl --user disable ward");
  println!("  rm ~/.config/systemd/user/ward.service");
  println!("  systemctl --user daemon-reload");

  Ok(())
}

#[cfg(target_os = "windows")]
fn install_windows_service() -> Result<()> {
  use std::env;

  let exe_path = env::current_exe()?;
  let ward_path = exe_path
    .parent()
    .unwrap_or(std::path::Path::new("C:\\Program Files\\Ward"))
    .join("ward.exe");

  println!("Windows service installation:");
  println!();
  println!("Run the following command as Administrator:");
  println!("  sc create Ward binPath= \"{}\" start= auto", ward_path.display());
  println!("  sc start Ward");
  println!();
  println!("Or add Ward to startup:");
  println!(
    "  Create a shortcut to {} in your Startup folder",
    ward_path.display()
  );

  Ok(())
}

#[cfg(target_os = "windows")]
fn uninstall_windows_service() -> Result<()> {
  println!("Windows service uninstallation:");
  println!();
  println!("Run the following commands as Administrator:");
  println!("  sc stop Ward");
  println!("  sc delete Ward");

  Ok(())
}
