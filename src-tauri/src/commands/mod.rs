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
//! - `corsair` — Corsair HID device control (Commander Core XT, iCUE NEXUS)/// - `openrgb` — Direct USB HID RGB control (IT8297, K70, Aerox 3, QCK Prism)
pub mod assistant;
mod boot;
mod boot_guard;
mod boot_patch;
mod cleanup;
mod config_cmd;
pub(crate) mod corsair;
mod desktop_launcher;
mod efi;
mod fstab;
mod headless;
pub(crate) mod health;
pub mod helpers;
mod install;
pub(crate) mod intent;
pub(crate) mod lighting;
mod mount;
pub(crate) mod openrgb;
pub(crate) mod pi_remote;
pub(crate) mod pi_tent;
mod rollback;
mod scope;
pub(crate) mod snapshots;
mod sync_cmd;
pub(crate) mod timer;
pub(crate) mod tuning;
mod verify;

// Re-export all Tauri commands so lib.rs can use `commands::*`
pub use assistant::{
    assistant_chat, assistant_status, jarvis_listen, jarvis_listener_enabled,
    jarvis_set_listener_enabled, jarvis_speak,
};
pub use boot::get_boot_info;
pub use boot_guard::{
    backup_boot_entries, delete_boot_backup, get_boot_health, restore_boot_entries,
};
pub use cleanup::{cancel_scan, delete_cleanup_paths, get_cleanup_dir_contents, scan_cleanup};
pub use config_cmd::*;
pub use corsair::{
    corsair_apply_fan_curves, corsair_ccxt_connect, corsair_ccxt_disconnect, corsair_ccxt_poll,
    corsair_nexus_connect, corsair_nexus_disconnect, corsair_nexus_display,
    corsair_nexus_get_frame, corsair_nexus_get_layout, corsair_nexus_next_page,
    corsair_nexus_prev_page, corsair_nexus_refresh_sys, corsair_nexus_reset_layout,
    corsair_nexus_set_auto_cycle, corsair_nexus_set_layout, corsair_nexus_set_page,
    corsair_nexus_status, corsair_save_profile, corsair_set_fan_curve, corsair_set_fan_speed,
    corsair_set_rgb, get_corsair_status,
};
pub use desktop_launcher::{launch_desktop_app, list_desktop_apps};
pub use headless::run_sync_headless;
pub use health::{get_health, get_system_status};
pub use install::{
    get_integration_status, install_system_integration, uninstall_system_integration,
};
pub use lighting::{
    govee_lamp_color, govee_master_brightness, govee_master_power,
    lighting_master_brightness, lighting_master_color, lighting_master_power,
    rgb_master_brightness, rgb_master_power,
};
pub use openrgb::{
    openrgb_all_off, openrgb_connect, openrgb_disconnect, openrgb_off, openrgb_refresh,
    openrgb_set_color, openrgb_set_led, openrgb_set_mode, openrgb_set_zone_color,
    openrgb_set_zone_leds, openrgb_status,
};
pub use pi_remote::{
    add_pi_device, get_pi_devices, get_pi_status, get_pi_status_all, open_pi_remote, pi_reboot,
    pi_run_command, pi_shutdown, remove_pi_device, test_pi_connection,
};
pub use pi_tent::{get_pi_tent_history, get_pi_tent_status};
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
pub use verify::{run_verify_collect, verify_backup};
