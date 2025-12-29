# Ward Installation Guide

Ward is an automated GPG key manager that treats a USB flash drive as a hardware security token.

## Building from Source

### Prerequisites

- Rust 1.75 or later (2024 edition)
- GPG 2.x installed

### Build

```bash
cargo build --release
```

The binaries will be located at:
- `target/release/ward` - The daemon
- `target/release/wardctl` - The control CLI

### Install Binaries

```bash
# Linux/macOS
sudo cp target/release/ward target/release/wardctl /usr/local/bin/

# Windows (run as Administrator)
copy target\release\ward.exe C:\Program Files\Ward\
copy target\release\wardctl.exe C:\Program Files\Ward\
```

## Quick Start

### 1. Prepare a USB Drive

Insert a USB flash drive and note its mount point.

```bash
# Initialize the drive
wardctl init /media/your-usb-drive --label "My GPG Key"
```

This creates a `.ward` directory on the drive with the following structure:
```
.ward/
  manifest.toml    # Drive metadata and checksum
  keys/            # Your GPG keys go here
```

### 2. Copy Your GPG Keys

```bash
# Copy your existing GPG home to the drive
cp -r ~/.gnupg/* /media/your-usb-drive/.ward/keys/
```

### 3. Authorize the Drive

```bash
wardctl allow /media/your-usb-drive
```

### 4. Start the Daemon

```bash
# Run in foreground (for testing)
ward --foreground --verbose

# Or install as a service
wardctl install
```

## Usage

### Check Status

```bash
wardctl status
```

### List Detected Drives

```bash
wardctl list
```

### View Configuration

```bash
wardctl config
```

### Remove a Drive from Allow List

```bash
wardctl disallow <UUID>
```

## How It Works

1. **Idle State**: Ward watches for removable drives
2. **Detection**: When a USB drive is connected, Ward scans for `.ward/manifest.toml`
3. **Validation**: The drive UUID is checked against the allow list
4. **Activation**:
   - On Linux: Keys are copied to RAM (`/dev/shm` or `/run/user/UID`)
   - On Windows: The GPG home symlinks directly to the USB
   - The existing `~/.gnupg` is backed up and replaced with a symlink
   - `gpg-agent` is restarted with the new configuration
5. **Active**: Ward monitors the drive presence
6. **Removal**: When the drive is removed:
   - `gpg-agent` is killed
   - The symlink is removed
   - The backup is restored
   - On Linux: RAM storage is securely wiped

## Security Notes

### Linux
- Keys are copied to RAM (tmpfs) and never written to disk
- Storage is securely wiped (overwritten with zeros) on removal
- Permissions are set to `0700` for directories, `0600` for files

### Windows
- Keys remain on the USB drive (not copied to RAM)
- Use an encrypted USB drive for additional security
- Consider using BitLocker To Go

## Service Installation

### Linux (systemd)

```bash
wardctl install
systemctl --user daemon-reload
systemctl --user enable ward
systemctl --user start ward
```

### Windows

Run as Administrator:
```powershell
sc create Ward binPath= "C:\Program Files\Ward\ward.exe" start= auto
sc start Ward
```

Or add to startup folder for per-user operation.

## Uninstallation

### Linux

```bash
systemctl --user stop ward
systemctl --user disable ward
wardctl uninstall
sudo rm /usr/local/bin/ward /usr/local/bin/wardctl
```

### Windows

Run as Administrator:
```powershell
sc stop Ward
sc delete Ward
rmdir /s "C:\Program Files\Ward"
```

## Configuration

Configuration is stored in:
- Linux: `~/.config/ward/config.toml`
- Windows: `%APPDATA%/ward/config.toml`

Example configuration:
```toml
poll_interval_ms = 1000
verbose = false

[allowed_drives]
# UUIDs of authorized drives are stored here
```

## Troubleshooting

### Drive Not Detected

1. Check that the drive is mounted
2. Verify the `.ward` directory exists
3. Run `wardctl list` to see detected drives

### GPG Not Working After Activation

1. Check `wardctl status` for active drive info
2. Verify `GNUPGHOME` environment variable
3. Try restarting `gpg-agent`:
   ```bash
   gpgconf --kill gpg-agent
   gpg-agent --daemon
   ```

### Permission Denied

On Linux, ensure you have access to the RAM storage:
```bash
ls -la /dev/shm
ls -la /run/user/$(id -u)
```
