mod cli;
mod commands;
mod config;
mod gateway;
mod scanner;
pub(crate) mod sysfs;
mod sysmon;
mod util;
mod widget;

use commands::*;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconEvent,
    Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_window_state::{AppHandleExt as _, Builder as WindowStateBuilder, StateFlags};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::print_stderr, clippy::expect_used)]
pub fn run() {
    // Single-instance guard via lockfile
    let lock_path = format!(
        "{}/arclight.lock",
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())
    );
    let lock_file = std::fs::File::create(&lock_path).expect("Cannot create lock file");
    use std::os::unix::io::AsRawFd;
    // SAFETY: flock() on a valid fd is safe — no memory concerns, only blocks/returns errno.
    #[allow(unsafe_code)]
    let ret = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        eprintln!("arclight läuft bereits");
        std::process::exit(0);
    }
    // Keep lock_file alive for the lifetime of the process
    let _lock = lock_file;

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(
            WindowStateBuilder::default()
                .with_state_flags(StateFlags::all())
                .with_denylist(&["widget"])
                .build(),
        )
        .setup(|app| {
            let log_level = log::LevelFilter::Info;
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log_level)
                    .build(),
            )?;

            // ── Tray Menu ──────────────────────────────────────────
            let handle = app.handle().clone();
            let show = MenuItemBuilder::with_id("show", "Arclight öffnen").build(&handle)?;
            let widget = MenuItemBuilder::with_id("widget", "Widget ein/aus").build(&handle)?;
            let quit = MenuItemBuilder::with_id("quit", "Beenden").build(&handle)?;
            let menu = MenuBuilder::new(&handle)
                .item(&show)
                .separator()
                .item(&widget)
                .separator()
                .item(&quit)
                .build()?;

            if let Some(tray) = app.tray_by_id("main-tray") {
                tray.set_menu(Some(menu))?;
                tray.on_menu_event(move |app_handle, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(w) = app_handle.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    "widget" => {
                        widget::toggle_widget(app_handle);
                    }
                    "quit" => {
                        // Kill any Jarvis TTS/music processes before exit
                        cleanup_jarvis_processes();
                        app_handle.exit(0);
                    }
                    _ => {}
                });
                tray.on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        let app_handle = tray.app_handle();
                        if let Some(w) = app_handle.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                });
            }

            // ── Core background bootstrap ────────────────────────────
            let app_handle = app.handle().clone();
            commands::assistant::set_app_handle(app_handle.clone());

            // Wake-word readiness must not wait behind slower service boot.
            // The listener manages its own background thread and model load.
            commands::assistant::spawn_clap_listener();

            // Fast-return setup: show UI/tray immediately, do heavy startup in background.
            std::thread::Builder::new()
                .name("arclight-core-bootstrap".into())
                .spawn(move || {
                    // HTTP API Gateway (for mobile app)
                    let token = std::env::var("ARCLIGHT_TOKEN").unwrap_or_default();
                    gateway::spawn_gateway(token);

                    // Background services for Jarvis
                    // Ensure llama-server is running
                    let health_ok = reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(2))
                        .build()
                        .ok()
                        .and_then(|c| c.get("http://localhost:8080/health").send().ok())
                        .map(|r| r.status().is_success())
                        .unwrap_or(false);
                    if !health_ok {
                        log::info!("[startup] llama-server not running — starting it");
                        let _ = std::process::Command::new("systemctl")
                            .args(["--user", "start", "llama-server.service"])
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                    } else {
                        log::info!("[startup] llama-server already running");
                    }
                    commands::assistant::ensure_orpheus_tts_running();

                    // Ensure whisper-server (whisper.cpp hipBLAS) is running
                    let whisper_ok = reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(2))
                        .build()
                        .ok()
                        .and_then(|c| c.get("http://127.0.0.1:8178/").send().ok())
                        .map(|r| r.status().is_success())
                        .unwrap_or(false);
                    if !whisper_ok {
                        log::info!("[startup] whisper-server not running — starting it");
                        let _ = std::process::Command::new("systemctl")
                            .args(["--user", "start", "whisper-server.service"])
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                    } else {
                        log::info!("[startup] whisper-server already running");
                    }

                    // Pre-warm STT worker (connects to whisper-server, near-instant).
                    if let Err(e) = commands::assistant::warmup_stt_worker() {
                        log::warn!("[startup] STT worker pre-warm failed: {e}");
                    }

                    log::info!("[startup] Core background bootstrap complete");
                })
                .ok();

            // Device auto-connect is already internally threaded, so just trigger it.
            commands::corsair::auto_connect_devices();

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    // Save widget position before hiding
                    if let Some(w) = window.app_handle().get_webview_window("widget") {
                        widget::save_widget_pos(&w);
                    }
                    // Persist size/position even when we keep running in the tray.
                    let _ = window.app_handle().save_window_state(StateFlags::all());
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_system_status,
            get_snapshots,
            create_snapshot,
            delete_snapshot,
            get_snapper_limits,
            run_snapper_cleanup,
            run_sync,
            get_sync_status,
            get_sync_log,
            get_sync_scope,
            get_timer_config,
            set_timer_enabled,
            rollback_snapshot,
            get_snapper_diff,
            get_btrfs_usage,
            get_health,
            get_subvolumes,
            get_config,
            get_activity_log,
            save_config_cmd,
            detect_disks,
            reset_config,
            scan_excludes,
            scan_cleanup,
            cancel_scan,
            delete_cleanup_paths,
            get_cleanup_dir_contents,
            install_timer,
            uninstall_timer,
            get_system_monitor,
            get_boot_info,
            verify_backup,
            get_integration_status,
            install_system_integration,
            uninstall_system_integration,
            get_tuning_status,
            apply_tuning,
            get_gpu_oc_status,
            apply_gpu_oc,
            reset_gpu_oc,
            get_gpu_oc_profile,
            install_gpu_oc_service,
            uninstall_gpu_oc_service,
            get_gpu_oc_service_status,
            get_pi_devices,
            get_pi_status_all,
            get_pi_status,
            get_pi_tent_history,
            get_pi_tent_status,
            pi_reboot,
            pi_shutdown,
            pi_run_command,
            test_pi_connection,
            add_pi_device,
            remove_pi_device,
            open_pi_remote,
            get_boot_health,
            backup_boot_entries,
            restore_boot_entries,
            delete_boot_backup,
            get_corsair_status,
            corsair_ccxt_connect,
            corsair_ccxt_disconnect,
            corsair_ccxt_poll,
            corsair_set_fan_speed,
            corsair_set_fan_curve,
            corsair_apply_fan_curves,
            corsair_set_rgb,
            corsair_nexus_connect,
            corsair_nexus_disconnect,
            corsair_nexus_status,
            corsair_nexus_display,
            corsair_nexus_set_page,
            corsair_nexus_next_page,
            corsair_nexus_prev_page,
            corsair_nexus_set_auto_cycle,
            corsair_nexus_refresh_sys,
            corsair_nexus_get_layout,
            corsair_nexus_set_layout,
            corsair_nexus_reset_layout,
            corsair_nexus_get_frame,
            corsair_save_profile,
            openrgb_connect,
            openrgb_disconnect,
            openrgb_status,
            openrgb_refresh,
            openrgb_set_color,
            openrgb_set_zone_color,
            openrgb_set_led,
            openrgb_set_zone_leds,
            openrgb_set_mode,
            openrgb_off,
            openrgb_all_off,
            lighting_master_power,
            lighting_master_color,
            govee_master_brightness,
            govee_lamp_color,
            lighting_master_brightness,
            govee_master_power,
            rgb_master_power,
            rgb_master_brightness,
            assistant_chat,
            assistant_status,
            jarvis_listener_enabled,
            jarvis_set_listener_enabled,
            jarvis_speak,
            jarvis_listen,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Headless sync for systemd/CLI — no GUI, no Tauri runtime
pub fn run_sync_cli(config_path: Option<String>) -> i32 {
    commands::run_sync_headless(config_path)
}

/// Native verify-collect helper — runs as root via pkexec
/// Mounts backup, reads fstab + EFI entries, outputs JSON
pub fn run_verify_collect(json: &str) -> i32 {
    commands::run_verify_collect(json)
}

/// Native sync-elevated helper — runs as root via pkexec
/// Executes the full sync, streaming JSON progress lines to stdout
pub fn run_sync_elevated(config_path: Option<String>) -> i32 {
    commands::run_sync_elevated_cli(config_path)
}

/// Native rollback-elevated helper — runs as root via pkexec
/// Executes the full rollback, streaming JSON progress lines to stdout
pub fn run_rollback_elevated(snap_id: u32, config_path: Option<String>) -> i32 {
    commands::run_rollback_elevated_cli(snap_id, config_path)
}

/// Rollback recovery wizard (CLI) — intended for rescue shells.
pub fn run_rollback_recover(config_path: Option<String>) -> i32 {
    commands::run_rollback_recover_cli(config_path)
}

/// Native sysfs write helper — runs as root via pkexec
/// Expects JSON: [{"path": "/sys/...", "value": "123"}, ...]
pub fn run_sysfs_write(json: &str) -> i32 {
    cli::run_sysfs_write(json)
}

/// Native privileged file-ops helper — runs as root via a single pkexec.
/// Accepts JSON array of operations:
///   {"op":"write",  "path":"...", "content":"..."}
///   {"op":"copy",   "src":"...",  "dst":"..."}
///   {"op":"delete", "path":"..."}
///   {"op":"mkdir",  "path":"..."}
///   {"op":"chmod",  "path":"...", "mode": 755}
pub fn run_file_ops(json: &str) -> i32 {
    cli::run_file_ops(json)
}

/// Kill any Jarvis-related child processes (mpv TTS/music, orpheus).
fn cleanup_jarvis_processes() {
    use std::process::{Command, Stdio};
    for pattern in &[
        "mpv.*jarvis",
        "mpv.*entrance",
        "pw-cat.*record",
        "openwake_listener",
        "jarvis_listener",
    ] {
        let _ = Command::new("pkill")
            .args(["-f", pattern])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
