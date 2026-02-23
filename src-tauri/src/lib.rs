mod commands;
mod config;

use commands::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_system_status,
            get_snapshots,
            create_snapshot,
            delete_snapshot,
            run_sync,
            get_sync_status,
            get_sync_log,
            get_timer_config,
            set_timer_enabled,
            rollback_snapshot,
            get_snapper_diff,
            get_btrfs_usage,
            get_health,
            get_subvolumes,
            get_config,
            save_config_cmd,
            detect_disks,
            reset_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
