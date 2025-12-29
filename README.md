# Ward

**Ward** is an automated GPG key manager that treats a standard USB flash drive as a hardware security token. It runs as a background service/daemon, detects specifically initialized USB drives, and hot-swaps the system's GPG configuration to use keys stored on the drive.

## Features

- **Plug & Sign**: Zero user interaction after setup
- **Cross-platform**: Supports Linux and Windows
- **Security-focused**:
  - Linux: Keys are copied to RAM (tmpfs) and never touch the disk
  - Secure wiping on drive removal
  - Integrity verification via checksums
- **Modular architecture**: Clean separation of concerns using Rust workspace

## Project Structure

```
ward/
├── Cargo.toml              # Workspace configuration
├── rustfmt.toml            # Rust formatting rules
├── INSTALL.md              # Installation guide
├── README.md               # This file
└── crates/
    ├── ward_core/          # Core library
    │   └── src/
    │       ├── lib.rs      # Public API
    │       ├── config.rs   # Configuration management
    │       ├── daemon.rs   # Daemon state machine
    │       ├── error.rs    # Error types
    │       ├── gpg.rs      # GPG agent management
    │       ├── manifest.rs # Ward drive manifest
    │       ├── monitor.rs  # Drive detection
    │       └── storage.rs  # Key storage backends
    ├── ward_daemon/        # Background service
    │   └── src/main.rs     # Daemon binary
    └── ward_ctl/           # Control CLI
        └── src/main.rs     # CLI binary
```

## Quick Start

```bash
# Build
cargo build --release

# Initialize a USB drive
wardctl init /path/to/usb --label "My Keys"

# Copy your GPG keys
cp -r ~/.gnupg/* /path/to/usb/.ward/keys/

# Authorize the drive
wardctl allow /path/to/usb

# Start the daemon
ward --foreground
```

## How It Works

1. **Idle**: Ward watches for removable drives
2. **Detection**: When a USB drive is connected, Ward scans for `.ward/manifest.toml`
3. **Validation**: The drive UUID is checked against the allow list
4. **Activation**:
   - Kills existing `gpg-agent`
   - On Linux: Copies keys to RAM (`/dev/shm` or `/run/user/UID`)
   - Creates symlink from `~/.gnupg` to the active keys
   - Restarts `gpg-agent`
5. **Monitoring**: Watches for drive removal
6. **Deactivation**: On removal, restores original configuration and securely wipes RAM

## Ward Drive Format

```
USB Root/
└── .ward/
    ├── manifest.toml    # UUID, label, checksum
    └── keys/            # GPG home directory content
        ├── pubring.kbx
        ├── trustdb.gpg
        └── private-keys-v1.d/
```

## Commands

- `wardctl init <path>` - Initialize a USB drive
- `wardctl allow [path]` - Authorize a Ward drive
- `wardctl disallow <uuid>` - Remove a drive from allow list
- `wardctl status` - Show daemon status
- `wardctl list` - List detected Ward drives
- `wardctl config` - Show configuration
- `wardctl install` - Install as system service
- `wardctl uninstall` - Uninstall system service

## Security Considerations

### Linux
- Keys are stored in RAM (tmpfs) at `/dev/shm` or `/run/user/UID`
- Directories use `0700` permissions, files use `0600`
- RAM is securely wiped (overwritten with zeros) on removal

### Windows
- Keys are accessed directly from the USB drive
- Consider using an encrypted USB drive (BitLocker To Go)

### General
- Only authorized drives (by UUID) can activate
- Manifest includes integrity checksum
- Path canonicalization prevents traversal attacks

## Building from Source

### Requirements
- Rust 1.75+ (2024 edition)
- GPG 2.x

### Build
```bash
cargo build --release
```

### Test
```bash
cargo test
```

## License

MIT
