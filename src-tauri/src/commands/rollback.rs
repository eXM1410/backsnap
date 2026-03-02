//! Btrfs rollback commands: snapshot-based root subvolume rollback with crash-safety.
//!
//! Architecture: GUI calls `rollback_snapshot` Tauri command → spawns
//! `pkexec backsnap --rollback-elevated <snap_id>` → entire rollback runs as root,
//! streaming JSON progress to stdout → GUI relays as Tauri events. One pkexec prompt.

use super::helpers::*;
use super::mount::AutoUmount;
use std::fs;
use std::path::Path;

const ROLLBACK_RECOVER_TMPDIR: &str = "/tmp/backsnap-rollback-recover";

// ─── Tauri command: spawn elevated subprocess + relay progress ───

#[tauri::command]
pub async fn rollback_snapshot(
    app: tauri::AppHandle,
    config: String,
    id: u32,
) -> Result<CommandResult, String> {
    validate_config(&config)?;
    let c = cfg();
    let root_config = &c.rollback.root_config;
    if config != *root_config {
        return Err(format!(
            "Rollback nur für {}-Config unterstützt",
            root_config
        ));
    }

    tokio::task::spawn_blocking(move || run_rollback_elevated(&app, id))
        .await
        .map_err(|e| format!("Spawn error: {}", e))?
}

/// Spawn `pkexec backsnap --rollback-elevated <snap_id>` and relay JSON progress.
fn run_rollback_elevated(app: &tauri::AppHandle, snap_id: u32) -> Result<CommandResult, String> {
    let id = snap_id.to_string();
    relay_elevated_subprocess(app, &["--rollback-elevated", &id])
}

// ─── Elevated CLI: runs as root, streams JSON progress to stdout ───

/// CLI entry point for `pkexec backsnap --rollback-elevated <snap_id>`.
#[allow(clippy::print_stderr, clippy::needless_pass_by_value)]
pub fn run_rollback_elevated_cli(snap_id: u32, config_path_override: Option<String>) -> i32 {
    if let Err(e) = preload_cli_config(config_path_override.as_deref()) {
        eprintln!("backsnap: {}", e);
        return 1;
    }
    emit_cli_result(do_rollback_root(snap_id), "ROLLBACK FEHLER")
}

// ─── Recovery Wizard (CLI) ───────────────────────────────────

/// CLI entry point for `backsnap --rollback-recover`.
///
/// This is meant to be used from a rescue shell (already root). It guides the user
/// through restoring the previously backed-up root subvolume (e.g. `@.broken-*`) back
/// to the active root subvolume name (e.g. `@`).
#[allow(clippy::print_stdout, clippy::print_stderr, clippy::needless_pass_by_value)]
pub fn run_rollback_recover_cli(config_path_override: Option<String>) -> i32 {
    if let Err(e) = preload_cli_config(config_path_override.as_deref()) {
        eprintln!("backsnap: {}", e);
        return 1;
    }

    if !is_root() {
        eprintln!("backsnap: Rollback-Recovery benötigt root (im Rescue-Modus bist du i.d.R. schon root).\n\
Bitte starte: sudo backsnap --rollback-recover");
        return 1;
    }

    match do_rollback_recover_root() {
        Ok(msg) => {
            println!("{}", msg);
            0
        }
        Err(e) => {
            eprintln!("backsnap: {}", e);
            1
        }
    }
}

#[allow(clippy::print_stdout)]
fn prompt(prompt: &str) -> Result<String, String> {
    use std::io::Write;
    print!("{}", prompt);
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut buf = String::new();
    std::io::stdin()
        .read_line(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf.trim().to_string())
}

#[allow(clippy::print_stdout)]
fn do_rollback_recover_root() -> Result<String, String> {
    let _lock = SyncLock::acquire().map_err(|_e| {
        "Recovery nicht möglich: Es läuft bereits ein Sync oder Rollback.".to_string()
    })?;

    let c = cfg();
    let root_subvol = &c.rollback.root_subvol;
    let boot_uuid = get_boot_uuid();

    let tmpdir = ROLLBACK_RECOVER_TMPDIR;
    let _ = fs::create_dir_all(tmpdir);

    let dev_arg = format!("UUID={}", boot_uuid);
    let mount_opts = format!("subvolid=5,{}", c.sync.mount_options);
    let mount_res = run_cmd("mount", &["-o", &mount_opts, &dev_arg, tmpdir]);
    if !mount_res.success {
        return Err(format!(
            "Konnte Btrfs-Top-Level nicht mounten: {}",
            mount_res.stderr.trim()
        ));
    }
    let _umount_guard = AutoUmount(tmpdir.to_string());

    let current_path = format!("{}/{}", tmpdir, root_subvol);
    if !Path::new(&current_path).exists() {
        return Err(format!(
            "Aktives Root-Subvolume '{}' nicht gefunden unter {}",
            root_subvol, tmpdir
        ));
    }

    // Find backups from our rollback flow
    let mut broken: Vec<String> = fs::read_dir(tmpdir)
        .map_err(|e| format!("read_dir {}: {}", tmpdir, e))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&format!("{}.broken-", root_subvol)))
        .collect();

    if broken.is_empty() {
        return Err(format!(
            "Keine Rollback-Backups gefunden (erwartet: {}.broken-<timestamp>).\n\
Wenn du manuell zurück willst, prüfe: ls {}",
            root_subvol, tmpdir
        ));
    }

    // Sort newest first (timestamps are lexicographically sortable)
    broken.sort();
    broken.reverse();

    println!("backsnap Rollback-Recovery Wizard\n");
    println!("Aktives Root: {}", root_subvol);
    println!("Gefundene Backups:");
    for (i, name) in broken.iter().enumerate() {
        println!("  [{}] {}", i + 1, name);
    }
    println!();

    let choice = prompt("Welche Nummer soll wiederhergestellt werden? ")?;
    let idx: usize = choice
        .parse()
        .map_err(|_e| "Bitte eine Zahl eingeben.".to_string())?;
    if idx == 0 || idx > broken.len() {
        return Err("Ungültige Auswahl.".to_string());
    }
    let selected = &broken[idx - 1];
    let selected_path = format!("{}/{}", tmpdir, selected);

    println!("\nAuswahl: {} → {}", selected, root_subvol);
    println!(
        "Das aktuelle Root '{}' wird vorher als *.bad-* gesichert.",
        root_subvol
    );
    let confirm = prompt("Zum Fortfahren 'YES' tippen: ")?;
    if confirm != "YES" {
        return Err("Abgebrochen.".to_string());
    }

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let bad_name = format!("{}.bad-{}", root_subvol, timestamp);
    let bad_path = format!("{}/{}", tmpdir, bad_name);

    // Swap: current root → bad, selected broken → root
    fs::rename(&current_path, &bad_path)
        .map_err(|e| format!("Konnte {} nicht sichern: {}", root_subvol, e))?;
    if let Err(e) = fs::rename(&selected_path, &current_path) {
        // Try to roll back the roll-back (best-effort)
        let _ = fs::rename(&bad_path, &current_path);
        return Err(format!("Konnte Backup nicht aktivieren: {}", e));
    }

    println!("\n✓ Wiederhergestellt.");
    println!("Aktives Root ist jetzt wieder: {}", root_subvol);
    println!("Vorheriger Zustand liegt unter: {}", bad_name);
    println!("\nNächster Schritt: reboot");

    Ok("Recovery abgeschlossen.".to_string())
}


/// Full rollback running as root — all fs ops are native Rust, no pkexec needed.
fn do_rollback_root(snap_id: u32) -> Result<CommandResult, String> {
    let _lock = SyncLock::acquire().map_err(|_e| {
        "Rollback nicht möglich: Es läuft bereits ein Sync oder Rollback. \
         Bitte warten bis der laufende Vorgang beendet ist."
            .to_string()
    })?;
    let c = cfg();
    let boot_uuid = get_boot_uuid();

    emit_sync_progress(
        "prepare",
        &format!("Rollback auf Snapshot #{} vorbereiten...", snap_id),
        10,
    );

    let tmpdir = ROLLBACK_TMPDIR;
    let _ = fs::create_dir_all(tmpdir);

    let dev_arg = format!("UUID={}", boot_uuid);
    let mount_opts = format!("subvolid=5,{}", c.sync.mount_options);
    // Already root — direct mount
    let mount_res = run_cmd("mount", &["-o", &mount_opts, &dev_arg, tmpdir]);
    if !mount_res.success {
        return Err(format!(
            "Konnte Btrfs-Root nicht mounten: {}",
            mount_res.stderr
        ));
    }
    let umount_guard = AutoUmount(tmpdir.to_string());

    let result = do_rollback_inner_root(snap_id, tmpdir, &c);

    drop(umount_guard);
    let _ = fs::remove_dir(tmpdir);

    result
}

fn do_rollback_inner_root(
    snap_id: u32,
    tmpdir: &str,
    c: &crate::config::AppConfig,
) -> Result<CommandResult, String> {
    let root_subvol = &c.rollback.root_subvol;
    let possible_paths = [
        format!("{}/@.snapshots/{}/snapshot", tmpdir, snap_id),
        format!("{}/@/.snapshots/{}/snapshot", tmpdir, snap_id),
        format!("{}/.snapshots/{}/snapshot", tmpdir, snap_id),
    ];
    let snap_path = possible_paths.into_iter().find(|p| Path::new(p).exists())
        .ok_or_else(|| format!("Snapshot #{} nicht gefunden (geprüfte Pfade: @.snapshots, @/.snapshots, .snapshots)", snap_id))?;

    let diff = run_cmd(
        "snapper",
        &[
            "-c",
            &c.rollback.root_config,
            "status",
            &format!("{}..0", snap_id),
        ],
    );
    let diff_count = diff.stdout.lines().count();
    emit_sync_progress(
        "info",
        &format!(
            "{} geänderte Dateien seit Snapshot #{}",
            diff_count, snap_id
        ),
        25,
    );

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let broken_name = format!("{}.broken-{}", root_subvol, timestamp);
    let current_at = format!("{}/{}", tmpdir, root_subvol);
    let broken_path = format!("{}/{}", tmpdir, broken_name);

    if !Path::new(&current_at).exists() {
        return Err(format!("Kein aktives {} Subvolume gefunden", root_subvol));
    }

    // Cleanup old broken backups (keep last N)
    emit_sync_progress("cleanup", "Alte Backups aufräumen...", 30);
    if let Ok(entries) = fs::read_dir(tmpdir) {
        let mut broken_dirs: Vec<String> = entries
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&format!("{}.broken-", root_subvol)))
            .map(|n| format!("{}/{}", tmpdir, n))
            .collect();
        broken_dirs.sort();
        while broken_dirs.len() > c.rollback.max_broken_backups {
            let old = broken_dirs.remove(0);
            sync_log(
                &c.sync.log_path,
                &format!("Cleanup: lösche altes Backup {}", old),
            );
            let _ = run_cmd("btrfs", &["subvolume", "delete", &old]);
        }
    }

    // ── Crash-safe rollback: snapshot first, then atomic swap ──

    let new_root_tmp = format!("{}/{}.rollback-{}", tmpdir, root_subvol, timestamp);
    emit_sync_progress(
        "snapshot",
        &format!("Snapshot #{} erstellen...", snap_id),
        50,
    );
    let snap_res = run_cmd(
        "btrfs",
        &["subvolume", "snapshot", &snap_path, &new_root_tmp],
    );
    if !snap_res.success {
        return Err(format!(
            "Konnte Snapshot nicht erstellen: {}",
            snap_res.stderr
        ));
    }

    // Move current root → broken (native rename, already root)
    emit_sync_progress(
        "backup",
        &format!("Aktuelles {} sichern als {} ...", root_subvol, broken_name),
        65,
    );
    sync_log(
        &c.sync.log_path,
        &format!(
            "Rollback #{}: mv {} -> {}",
            snap_id, root_subvol, broken_name
        ),
    );

    if let Err(e) = fs::rename(&current_at, &broken_path) {
        let _ = run_cmd("btrfs", &["subvolume", "delete", &new_root_tmp]);
        return Err(format!("Konnte {} nicht verschieben: {}", root_subvol, e));
    }

    // Move new snapshot → root name (native rename)
    emit_sync_progress(
        "activate",
        &format!("Neues {} aktivieren...", root_subvol),
        80,
    );
    if let Err(e) = fs::rename(&new_root_tmp, &current_at) {
        sync_log(
            &c.sync.log_path,
            "KRITISCH: Rename fehlgeschlagen, stelle altes Root wieder her",
        );
        if fs::rename(&broken_path, &current_at).is_ok() {
            let del = run_cmd("btrfs", &["subvolume", "delete", &new_root_tmp]);
            if !del.success {
                sync_log(
                    &c.sync.log_path,
                    &format!(
                        "WARNUNG: Temp-Snapshot {} konnte nicht gelöscht werden: {}",
                        new_root_tmp, del.stderr
                    ),
                );
            }
            return Err(format!(
                "Konnte neues Root nicht aktivieren: {}. Altes Root wiederhergestellt.",
                e
            ));
        }
        let msg = format!(
            "FATAL: System ist möglicherweise nicht bootfähig! \
             Weder neues Root noch altes Root ({}) konnte nach {} verschoben werden. \
             Bitte von Live-USB booten und manuell reparieren. \
             Temp-Snapshot: {}, Broken-Backup: {}",
            broken_path, current_at, new_root_tmp, broken_path
        );
        sync_log(&c.sync.log_path, &format!("KRITISCH: {}", msg));
        return Err(msg);
    }

    let snapshots_dir = format!("{}/.snapshots", current_at);
    let _ = fs::create_dir_all(&snapshots_dir);

    sync_log(
        &c.sync.log_path,
        &format!("Rollback #{} erfolgreich. Backup: {}", snap_id, broken_name),
    );

    emit_sync_progress(
        "done",
        &format!(
            "Rollback erfolgreich! Backup: {}. Bitte neustarten.",
            broken_name
        ),
        100,
    );

    let boot_uuid = get_boot_uuid();
    let recovery_info = format!(
        "Falls etwas schiefgeht:\n\
         1. Im Boot-Menü '{}' wählen\n\
         2. mount -o subvolid=5 UUID={} /mnt\n\
         3. mv /mnt/{} /mnt/{}.bad\n\
         4. mv /mnt/{} /mnt/{}\n\
         5. reboot",
        c.rollback.recovery_label, boot_uuid, root_subvol, root_subvol, broken_name, root_subvol
    );

    Ok(CommandResult {
        success: true,
        stdout: format!(
            "Rollback auf Snapshot #{} erfolgreich!\n\
             Backup: {}\n\
             Geänderte Dateien: {}\n\n\
             {}\n\n\
             Bitte jetzt neustarten.",
            snap_id, broken_name, diff_count, recovery_info
        ),
        stderr: String::new(),
        exit_code: 0,
    })
}
