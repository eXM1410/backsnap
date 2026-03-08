//! Headless CLI sync: run from systemd or terminal without GUI.

use super::helpers::*;
use super::sync_cmd::*;
use crate::config;

/// CLI/systemd entry point — headless sync with human-readable output.
#[allow(clippy::print_stderr)]
pub fn run_sync_headless(config_path_override: Option<String>) -> i32 {
    let c = if let Some(path) = config_path_override {
        match config::load_config_from(std::path::Path::new(&path)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("arclight: Config-Fehler: {}", e);
                return 1;
            }
        }
    } else {
        cfg()
    };

    if let Err(e) = validate_sync_config(&c) {
        eprintln!("arclight: Config-Validierung fehlgeschlagen: {}", e);
        return 1;
    }

    if let Some(parent) = std::path::Path::new(&c.sync.log_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let _lock = match SyncLock::acquire() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("arclight: {}", e);
            sync_log(&c.sync.log_path, &format!("CLI FEHLER: {}", e));
            return 1;
        }
    };

    match sync_core(&c, &SyncMode::Headless) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("arclight: FEHLER: {}", e);
            sync_log(&c.sync.log_path, &format!("CLI FEHLER: {}", e));
            1
        }
    }
}
