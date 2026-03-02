//! Tauri command handlers, split into logical submodules.
//!
//! Architecture:
//! - `helpers` — shared utilities (run_cmd, run_privileged, validation, etc.)
//! - `config_cmd` — config CRUD commands
//! - `health` — health check + system status
//! - `snapshots` — snapper snapshot commands
//! - `sync_cmd` — rsync sync orchestration
//! - `mount` — mount/unmount helpers + RAII guard
//! - `fstab` — privileged I/O + fstab patching
//! - `boot_patch` — boot entry patching + cross-boot entries
//! - `efi` — EFI partition helpers + snapshot cleanup
//! - `verify` — backup verification
//! - `scope` — sync scope analysis
//! - `boot` — boot info & validation
//! - `rollback` — btrfs rollback
//! - `timer` — systemd timer install/uninstall
//! - `headless` — CLI/systemd headless sync (no Tauri runtime)

mod boot;
mod boot_guard;
mod boot_patch;
mod cleanup;
mod config_cmd;
mod efi;
mod fstab;
mod headless;
mod health;
pub mod helpers;
mod install;
mod mount;
mod rollback;
mod scope;
mod snapshots;
mod sync_cmd;
mod pi_remote;
mod timer;
mod tuning;
mod verify;

// Re-export all Tauri commands so lib.rs can use `commands::*`
pub use boot::get_boot_info;
pub use boot_guard::{backup_boot_entries, delete_boot_backup, get_boot_health, restore_boot_entries};
pub use cleanup::{cancel_scan, delete_cleanup_paths, get_cleanup_dir_contents, scan_cleanup};
pub use config_cmd::*;
pub use headless::run_sync_headless;
pub use health::{get_health, get_system_status};
pub use install::{
    get_integration_status, install_system_integration, uninstall_system_integration,
};
pub use rollback::{rollback_snapshot, run_rollback_elevated_cli, run_rollback_recover_cli};
pub use scope::get_sync_scope;
pub use snapshots::*;
pub use sync_cmd::{
    get_btrfs_usage, get_sync_log, get_sync_status, get_system_monitor, run_sync,
    run_sync_elevated_cli,
};
pub use timer::*;
pub use tuning::{
    apply_gpu_oc, apply_tuning, get_gpu_oc_profile, get_gpu_oc_service_status, get_gpu_oc_status,
    get_tuning_status, install_gpu_oc_service, reset_gpu_oc, uninstall_gpu_oc_service,
};
pub use pi_remote::{
    add_pi_device, get_pi_devices, get_pi_status, get_pi_status_all, open_pi_remote,
    pi_reboot, pi_run_command, pi_shutdown, remove_pi_device, test_pi_connection,
};
pub use verify::{run_verify_collect, verify_backup};
