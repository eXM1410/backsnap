# backsnap

> Btrfs snapshot management & NVMe-to-NVMe backup GUI for Linux — built with Tauri 2, React 19, and Rust.

![License](https://img.shields.io/badge/license-MIT-blue)
![Platform](https://img.shields.io/badge/platform-Linux-brightgreen)
![Tauri](https://img.shields.io/badge/Tauri-2.10-orange)
![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange)

backsnap is a desktop application that provides a graphical interface for managing Btrfs snapshots (via snapper) and performing full disk-to-disk NVMe synchronization. It auto-detects your disks, generates a config, and lets you manage everything from a single dashboard.

## Features

- **Dashboard** — System status, boot disk, snapshot counts, sync status, disk usage at a glance
- **Snapshot Management** — Create, delete, diff, and rollback Btrfs snapshots across all snapper configs
- **NVMe Sync** — Full 3-phase rsync-based disk sync (system → home → boot) with live progress events
- **Rollback** — One-click rollback to any snapshot with automatic backup of current `@` subvolume and recovery instructions
- **Timer/Schedule** — Manage the systemd timer for automatic daily sync
- **Disk Overview** — Btrfs filesystem usage, disk space, subvolume listing
- **Logs** — Real-time log viewer with auto-scroll, color coding, and line numbers
- **Settings** — Full configuration UI: disk selection via auto-detection, exclude lists, subvolume mapping, mount options

## Architecture

```
┌─────────────────────────────────────────────┐
│              React 19 Frontend              │
│  Dashboard · Snapshots · Sync · Settings …  │
│        Tailwind CSS · lucide-react          │
├─────────────────────────────────────────────┤
│             Tauri 2 IPC Bridge              │
│          invoke() · listen(events)          │
├─────────────────────────────────────────────┤
│              Rust Backend                   │
│   commands.rs — all system operations       │
│   config.rs   — TOML config + auto-detect   │
│                                             │
│   snapper · rsync · btrfs · systemctl       │
│        (via pkexec for root ops)            │
└─────────────────────────────────────────────┘
```

## How It Works

### Sync

backsnap synchronizes a **primary** Btrfs disk to a **backup** Btrfs disk using rsync. It detects which disk you booted from and syncs to the other:

1. **Phase 1 — System (`/`):** Mount the backup disk's `@` subvolume, rsync the root filesystem excluding `/home`, `/boot`, virtual filesystems, and configurable paths
2. **Phase 2 — Home (`/home/`):** Mount `@home`, rsync with configurable excludes (caches, `node_modules`, Steam/Games, etc.)
3. **Phase 3 — Boot (`/boot/`):** Mount the backup EFI partition, rsync kernel/initramfs (excluding boot entries and EFI to avoid UEFI conflicts)

All rsync operations run with `ionice -c3` (idle I/O priority) and use `pkexec` for privilege escalation.

Progress is reported to the frontend in real-time via Tauri events.

### Rollback

Rollback works at the Btrfs subvolume level:

1. Mount the Btrfs root (`subvolid=5`)
2. Move the current `@` to `@.broken-<timestamp>`
3. Create a writable snapshot from the selected snapper snapshot as the new `@`
4. Old `@.broken-*` backups are cleaned up (configurable, default: keep 2)
5. A reboot is required for changes to take effect

Recovery instructions are displayed after rollback in case something goes wrong.

### Configuration

On first launch, backsnap auto-detects:
- All Btrfs partitions (via `lsblk`)
- Which disk is the boot disk
- Existing snapper configs
- Current username (for smart home excludes)

A TOML config file is generated at `~/.config/backsnap/config.toml` with sensible defaults. Everything is editable in the Settings page or by hand:

```toml
[disks]
primary_uuid = "your-primary-uuid"
primary_label = "Samsung 970 EVO"
backup_uuid = "your-backup-uuid"
backup_label = "XPG SPECTRIX S40G"

[sync]
timer_unit = "nvme-sync.timer"
mount_options = "compress=zstd,noatime"
mount_base = "/mnt/backsnap"
log_path = "/var/log/backsnap-sync.log"
system_excludes = ["/home/*", "/boot/*", "/proc/*", "/sys/*", ...]
home_excludes = [".cache", "**/node_modules", "**/__pycache__", ...]
home_extra_excludes = ["user/Games", "user/.local/share/Steam/..."]
extra_excludes_on_primary = true

[[sync.subvolumes]]
name = "system"
subvol = "@"
source = "/"
delete = true

[[sync.subvolumes]]
name = "home"
subvol = "@home"
source = "/home/"
delete = true

[boot]
sync_enabled = true
excludes = ["loader/entries/*", "loader/loader.conf", "EFI/"]

[snapper]
expected_configs = ["root", "home"]

[rollback]
max_broken_backups = 2
recovery_label = "Rescue"
root_subvol = "@"
```

## Requirements

- **Linux** with **Btrfs** root filesystem
- **snapper** — snapshot management
- **rsync** — file synchronization
- **btrfs-progs** — Btrfs tools
- **systemd** — timer management
- **pkexec / polkit** — privilege escalation (no root required)
- A second Btrfs-formatted NVMe/disk for sync target

## Build

```bash
# Prerequisites
pacman -S rust nodejs npm webkit2gtk-4.1   # Arch/CachyOS
# or: apt install libwebkit2gtk-4.1-dev    # Debian/Ubuntu

# Clone & build
git clone https://github.com/eXM1410/backsnap.git
cd backsnap
npm install
npx vite build
cargo build --manifest-path src-tauri/Cargo.toml --release

# Binary is at:
./src-tauri/target/release/backsnap
```

### Development

```bash
npx tauri dev
```

## Systemd Timer Setup

backsnap manages an existing systemd timer. To set one up:

```bash
# /etc/systemd/system/nvme-sync.service
[Unit]
Description=NVMe Btrfs Sync

[Service]
Type=oneshot
ExecStart=/path/to/backsnap-sync-script
# or call rsync directly

# /etc/systemd/system/nvme-sync.timer
[Unit]
Description=Daily NVMe Sync

[Timer]
OnCalendar=daily
RandomizedDelaySec=1h
Persistent=true

[Install]
WantedBy=timers.target
```

```bash
sudo systemctl enable --now nvme-sync.timer
```

The timer name is configurable in Settings.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Frontend | React 19, Vite 6, Tailwind CSS 3, lucide-react |
| Backend | Rust, Tauri 2.10, tokio, serde, chrono, toml |
| System | snapper, rsync, btrfs-progs, systemd, pkexec |

## Screenshots

*Coming soon*

## License

MIT
