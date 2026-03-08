//! Config management commands: get, save, reset, detect, scan.

use super::helpers::*;
use crate::config;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Emitter;

#[derive(serde::Serialize, Clone, Debug)]
struct ExcludeScanRuntimeStats {
    cpu_threads: usize,
    io_workers_cap: usize,
    rayon_threads: usize,
    tokio_blocking_task: usize,
}

fn exclude_scan_runtime_stats() -> ExcludeScanRuntimeStats {
    let cpu_threads = std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(4);
    ExcludeScanRuntimeStats {
        cpu_threads,
        io_workers_cap: cpu_threads.min(8),
        rayon_threads: rayon::current_num_threads(),
        tokio_blocking_task: 1,
    }
}

fn exclude_scan_log_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("arclight")
        .join("exclude-scan-last.jsonl")
}

fn write_scan_log_line(log_file: &Arc<Mutex<Option<std::fs::File>>>, payload: &serde_json::Value) {
    if let Ok(mut guard) = log_file.lock() {
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(file, "{}", payload);
        }
    }
}

#[tauri::command]
pub async fn get_config() -> Result<config::AppConfig, String> {
    tokio::task::spawn_blocking(config::load_config)
        .await
        .map_err(|e| format!("Config-Thread panicked: {}", e))?
}

#[tauri::command]
pub async fn get_activity_log() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(|| Ok(read_activity_log_lines(5000)))
        .await
        .map_err(|e| format!("Activity-Log thread panicked: {}", e))?
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn save_config_cmd(new_config: config::AppConfig) -> Result<(), String> {
    config::save_config(&new_config)?;
    invalidate_caches();
    Ok(())
}

#[tauri::command]
pub async fn scan_excludes(app: tauri::AppHandle) -> Result<(), String> {
    crate::scanner::CANCEL_SCAN.store(false, std::sync::atomic::Ordering::SeqCst);
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let log_path = exclude_scan_log_path();
    let runtime_stats = exclude_scan_runtime_stats();
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .ok();
    let log_file = Arc::new(Mutex::new(log_file));

    let _ = app.emit(
        "exclude-scan-log-path",
        log_path.to_string_lossy().into_owned(),
    );
    let _ = app.emit("exclude-scan-runtime-stats", runtime_stats.clone());
    log_activity_with_app(
        &app,
        "exclude-scan",
        &format!(
            "Exclude-Scan gestartet (user={}, io_workers={}, rayon_threads={})",
            username, runtime_stats.io_workers_cap, runtime_stats.rayon_threads
        ),
    );

    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let run_id = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let found_count = Arc::new(Mutex::new(0_u64));
        let phase_count = Arc::new(Mutex::new(0_u64));

        write_scan_log_line(
            &log_file,
            &serde_json::json!({
                "event": "scan_started",
                "run_id": run_id,
                "user": username,
                "log_path": log_path.to_string_lossy(),
                "runtime_stats": runtime_stats,
            }),
        );

        let log_for_found = Arc::clone(&log_file);
        let log_for_phase = Arc::clone(&log_file);
        let count_for_found = Arc::clone(&found_count);
        let count_for_phase = Arc::clone(&phase_count);
        let phase_start = Arc::new(Mutex::new(Instant::now()));

        crate::scanner::scan_home_excludes_streaming(
            &username,
            |exclude| {
                if let Ok(mut c) = count_for_found.lock() {
                    *c += 1;
                }
                log_activity_with_app(
                    &app,
                    "exclude-scan",
                    &format!(
                        "Treffer: {} | {:?} | {}",
                        exclude.path, exclude.category, exclude.size_human
                    ),
                );
                write_scan_log_line(
                    &log_for_found,
                    &serde_json::json!({
                        "event": "exclude_found",
                        "path": exclude.path,
                        "category": exclude.category,
                        "reason": exclude.reason,
                        "size_bytes": exclude.size_bytes,
                        "size_human": exclude.size_human,
                        "auto_exclude": exclude.auto_exclude,
                    }),
                );
                let _ = app.emit("exclude-found", exclude);
            },
            |phase| {
                if let Ok(mut c) = count_for_phase.lock() {
                    *c += 1;
                }
                if let Ok(mut ps) = phase_start.lock() {
                    let elapsed = ps.elapsed();
                    if phase.phase > 1 {
                        log_activity_with_app(
                            &app,
                            "exclude-scan",
                            &format!("Phase {} abgeschlossen in {:.2?}", phase.phase - 1, elapsed),
                        );
                    }
                    *ps = Instant::now();
                }
                log_activity_with_app(
                    &app,
                    "exclude-scan",
                    &format!("Starte Phase {}: {}", phase.phase, phase.label),
                );
                write_scan_log_line(
                    &log_for_phase,
                    &serde_json::json!({
                        "event": "scan_phase",
                        "phase": phase.phase,
                        "label": phase.label,
                    }),
                );
                let _ = app.emit("exclude-phase", phase);
            },
            {
                let last_emit =
                    std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
                let app_clone_prog = app.clone();
                move |path: &std::path::Path| {
                    if let Ok(mut last) = last_emit.try_lock() {
                        if last.elapsed().as_millis() > 50 {
                            let _ = app_clone_prog
                                .emit("exclude-progress", path.to_string_lossy().into_owned());
                            *last = std::time::Instant::now();
                        }
                    }
                }
            },
        );

        let found_total = found_count.lock().map(|c| *c).unwrap_or_default();
        let phase_total = phase_count.lock().map(|c| *c).unwrap_or_default();

        if let Ok(ps) = phase_start.lock() {
            log_activity_with_app(
                &app,
                "exclude-scan",
                &format!("Letzte Phase abgeschlossen in {:.2?}", ps.elapsed()),
            );
        }

        write_scan_log_line(
            &log_file,
            &serde_json::json!({
                "event": "scan_finished",
                "run_id": run_id,
                "duration_ms": started.elapsed().as_millis(),
                "phase_events": phase_total,
                "exclude_events": found_total,
            }),
        );

        log_activity_with_app(
            &app,
            "exclude-scan",
            &format!(
                "Exclude-Scan komplett beendet in {:.2?} ({} Treffer, {} Phasen)",
                started.elapsed(),
                found_total,
                phase_total
            ),
        );

        let _ = app.emit("exclude-scan-done", ());
    })
    .await
    .map_err(|e| format!("Scan thread panicked: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn reset_config(app: tauri::AppHandle) -> Result<config::AppConfig, String> {
    log_activity_with_app(&app, "autodetect", "Auto-Detect gestartet");

    let old_config = tokio::task::spawn_blocking(|| config::load_config().ok())
        .await
        .map_err(|e| format!("Config-Lese-Thread panicked: {}", e))?;

    log_activity_with_app(
        &app,
        "autodetect",
        "Analysiere Disk- und Boot-Konfiguration...",
    );
    let new_config = tokio::task::spawn_blocking(config::auto_detect_config)
        .await
        .map_err(|e| format!("Auto-detect thread panicked: {}", e))?;

    config::save_config(&new_config)?;
    invalidate_caches();

    let disks = tokio::task::spawn_blocking(config::detect_btrfs_disks)
        .await
        .map_err(|e| format!("Disk-Detect thread panicked: {}", e))?;

    let summary = format!(
        "{} Disks · {} Subvols · {}",
        disks.len(),
        new_config.sync.subvolumes.len(),
        new_config.boot.bootloader_type
    );

    let changed = old_config.as_ref().map_or(true, |old| {
        old.disks.primary_uuid != new_config.disks.primary_uuid
            || old.disks.backup_uuid != new_config.disks.backup_uuid
            || old.boot.bootloader_type != new_config.boot.bootloader_type
            || old.sync.subvolumes.len() != new_config.sync.subvolumes.len()
    });

    if changed {
        log_activity_with_app(
            &app,
            "autodetect",
            &format!("Scan abgeschlossen — {}", summary),
        );
    } else {
        log_activity_with_app(
            &app,
            "autodetect",
            &format!(
                "Scan abgeschlossen — {} — Keine Änderungen nötig ✓",
                summary
            ),
        );
    }

    Ok(new_config)
}

#[tauri::command]
pub async fn detect_disks() -> Result<Vec<config::DetectedDisk>, String> {
    tokio::task::spawn_blocking(config::detect_btrfs_disks)
        .await
        .map_err(|e| format!("Disk detection thread panicked: {}", e))
}
