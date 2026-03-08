//! Configuration module: types, I/O, detection, and auto-configuration.
//!
//! Architecture:
//! - `types` — config data structures (AppConfig, DiskConfig, SyncConfig, etc.)
//! - `io` — load/save config from/to TOML
//! - `detect` — auto-detect config, subvolumes, build home excludes
//! - `disk` — detect btrfs disks via lsblk
//! - `boot` — detect bootloader type (systemd-boot, grub)

mod boot;
mod detect;
mod disk;
pub(crate) mod io;
mod types;

pub use detect::auto_detect_config;
pub use disk::{detect_btrfs_disks, DetectedDisk};
pub use io::*;
pub use io::{invalidate_config_cache, set_config_cache};
pub use types::*;
