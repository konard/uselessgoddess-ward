# Installation

## Build

```bash
cargo build --release
sudo cp target/release/ward target/release/wardctl /usr/local/bin/
```

## Setup

```bash
wardctl init /media/usb --label "My Key"
cp -r ~/.gnupg/* /media/usb/.ward/keys/
wardctl allow /media/usb
ward --foreground
```

## Service

Run `wardctl install` and `wardctl uninstall` for platform-specific instructions.

## Config

Linux: `~/.config/ward/config.toml`
Windows: `%APPDATA%/ward/config.toml`
