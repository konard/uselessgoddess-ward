# Ward

Automated GPG key manager that treats a USB flash drive as a hardware security token.

## Quick Start

```bash
cargo build --release

wardctl init /path/to/usb --label "My Keys"
cp -r ~/.gnupg/* /path/to/usb/.ward/keys/
wardctl allow /path/to/usb
ward --foreground
```

## Commands

- `wardctl init <path>` - Initialize a USB drive
- `wardctl allow [path]` - Authorize a Ward drive
- `wardctl disallow <uuid>` - Remove from allow list
- `wardctl status` - Show daemon status
- `wardctl list` - List detected Ward drives
- `wardctl install` - Show service installation instructions
- `wardctl uninstall` - Show service uninstallation instructions

## Ward Drive Format

```
USB Root/
└── .ward/
    ├── manifest.toml
    └── keys/
```

## Security

- Linux: Keys copied to RAM (tmpfs), securely wiped on removal
- Windows: Direct USB access, use encrypted drive
- Only authorized drives (by UUID) can activate

## License

MIT
