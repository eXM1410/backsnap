//! NVMe sync commands: rsync-based subvolume sync, boot sync, EFI patching.

use super::boot_patch::*;
use super::efi::*;
use super::fstab::*;
use super::helpers::*;
use super::mount::*;
use crate::config::AppConfig;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// Number of log tail lines shown in sync status.
const SYNC_STATUS_LOG_TAIL: usize = 50;

// ─── Sync Context ─────────────────────────────────────────────

#[derive(Debug)]
pub(super) struct SyncContext {
    pub backup_dev: String,
    pub mount_base: String,
    pub direction: String,
}

pub(super) fn detect_sync_direction(c: &AppConfig) -> Result<SyncContext, String> {
    use super::boot::DiskSide;

    if c.disks.primary_uuid.is_empty() || c.disks.backup_uuid.is_empty() {
        return Err(
            "Disks nicht konfiguriert. Bitte unter Einstellungen Primary und Backup Disk wählen."
                .to_string(),
        );
    }

    let current_uuid = get_boot_uuid();
    match DiskSide::from_uuid(&current_uuid, c) {
        DiskSide::Primary => {}
        DiskSide::Backup => return Err(
            "SICHERHEITSSPERRE: System ist vom Backup gebootet! \
             Ein Sync würde die Primary-Disk mit älteren Backup-Daten überschreiben. \
             Bitte vom Primary-Disk booten, um einen Sync durchzuführen."
                .to_string(),
        ),
        DiskSide::Unknown => return Err(format!(
            "Boot-UUID {} entspricht weder Primary ({}) noch Backup ({}). Bitte Einstellungen prüfen.",
            current_uuid, c.disks.primary_uuid, c.disks.backup_uuid
        )),
    }

    let backup_dev = crate::sysfs::resolve_uuid(&c.disks.backup_uuid).ok_or_else(|| {
        format!(
            "Backup-Disk nicht gefunden (UUID: {}). Ist sie eingebaut?",
            c.disks.backup_uuid
        )
    })?;

    Ok(SyncContext {
        backup_dev,
        mount_base: c.sync.mount_base.clone(),
        direction: format!("{} -> {}", c.disks.primary_label, c.disks.backup_label),
    })
}

// ─── Rsync ────────────────────────────────────────────────────

// ─── Rsync Helpers ────────────────────────────────────────────

/// Truncate a (potentially huge) rsync stderr to at most `max_lines` lines.
/// Appends a note if lines were trimmed.
fn truncate_stderr(stderr: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    if lines.len() <= max_lines {
        return stderr.trim().to_string();
    }
    let kept: Vec<&str> = lines[..max_lines].to_vec();
    format!(
        "{}\n... ({} weitere Zeilen gekürzt)",
        kept.join("\n"),
        lines.len() - max_lines
    )
}

fn parse_rsync_make_way_paths(stderr: &str) -> Vec<String> {
    const PREFIXES: [&str; 3] = [
        "could not make way for new symlink: ",
        "could not make way for new directory: ",
        "could not make way for new file: ",
    ];

    let mut paths: Vec<String> = Vec::new();
    for line in stderr.lines() {
        let trimmed = line.trim();
        for pfx in PREFIXES {
            if let Some(rest) = trimmed.strip_prefix(pfx) {
                let rel = rest.trim();
                if !rel.is_empty() {
                    paths.push(rel.to_string());
                }
                break;
            }
        }
    }
    paths
}

fn is_safe_relative_path(p: &str) -> bool {
    let path = Path::new(p);
    if path.is_absolute() {
        return false;
    }
    for comp in path.components() {
        match comp {
            Component::Normal(_) => {}
            _ => return false,
        }
    }
    true
}

fn remove_rsync_conflicts(dst: &str, rel_paths: &[String]) {
    for rel in rel_paths {
        if !is_safe_relative_path(rel) {
            continue;
        }
        let full = Path::new(dst).join(rel);
        let full_s = full.to_string_lossy().into_owned();
        let _ = run_privileged("rm", &["-rf", "--", &full_s]);
    }
}

/// Build rsync argument list. `extra_info` appended to `--info=stats1`.
fn build_rsync_args(src: &str, dst: &str, excludes: &[String], delete: bool, extra_info: &str) -> Vec<String> {
    let info = if extra_info.is_empty() { "stats1".to_string() } else { format!("stats1,{extra_info}") };
    let mut args = vec![
        "-aAXx".into(), format!("--info={info}"), "--no-inc-recursive".into(),
        "--numeric-ids".into(), "--force".into(),
    ];
    // NOTE: Do NOT use --delete-excluded — would remove .snapshots on dest.
    if delete { args.push("--delete".into()); }
    for exc in excludes { args.push(format!("--exclude={exc}")); }
    args.push(src.into());
    args.push(dst.into());
    args
}

/// Run rsync blocking (ionice when root, pkexec when not). Retries once on exit 23.
pub(super) fn run_rsync(
    src: &str, dst: &str, excludes: &[String], delete: bool,
) -> Result<CommandResult, String> {
    let run = |args: &[String]| -> CommandResult {
        let refs: Vec<&str> = args.iter().map(std::string::String::as_str).collect();
        if is_root() {
            let mut full = vec!["-c3".to_string(), "rsync".to_string()];
            full.extend_from_slice(args);
            let f: Vec<&str> = full.iter().map(std::string::String::as_str).collect();
            run_cmd("ionice", &f)
        } else {
            run_privileged("rsync", &refs)
        }
    };

    let args = build_rsync_args(src, dst, excludes, delete, "");
    let result = run(&args);

    if result.success || result.exit_code == 24 { return Ok(result); }
    // Self-heal type-conflicts (symlink vs dir) and retry once
    if result.exit_code == 23 {
        let rels = parse_rsync_make_way_paths(&result.stderr);
        if !rels.is_empty() {
            remove_rsync_conflicts(dst, &rels);
            let retry = run(&args);
            if retry.success || retry.exit_code == 24 { return Ok(retry); }
        }
    }
    Err(format!("rsync {} -> {}: exit={} {}", src, dst, result.exit_code, truncate_stderr(&result.stderr, 20)))
}

// ─── Streaming rsync with live byte progress ──────────────────

/// Spawn rsync streaming progress2 events to the frontend. Retries once on exit 23.
pub(super) fn run_rsync_streaming(
    src: &str, dst: &str, excludes: &[String], delete: bool, phase_name: &str,
) -> Result<CommandResult, String> {
    let args = build_rsync_args(src, dst, excludes, delete, "progress2");

    let mut child = {
        let (cmd, full) = if is_root() {
            let mut v = vec!["-c3".to_string(), "rsync".to_string()];
            v.extend(args); ("ionice", v)
        } else {
            let mut v = vec!["rsync".to_string()];
            v.extend(args); ("pkexec", v)
        };
        Command::new(cmd).args(&full)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn().map_err(|e| format!("{cmd} rsync spawn: {e}"))?
    };

    let stdout = child.stdout.take().ok_or("rsync: no stdout")?;
    let stderr = child.stderr.take().ok_or("rsync: no stderr")?;
    let phase = phase_name.to_string();

    // True streaming: process bytes as they arrive, emit events per progress line.
    // rsync --info=progress2 uses \r to overwrite the progress line in-place,
    // so we split on both \r and \n to capture each update individually.
    let stdout_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut stats_output = String::new();
        let mut segment: Vec<u8> = Vec::with_capacity(256);

        loop {
            let consumed = {
                let buf = match reader.fill_buf() {
                    Ok([]) | Err(_) => break,
                    Ok(b) => b,
                };
                let len = buf.len();
                for &byte in buf {
                    if byte == b'\r' || byte == b'\n' {
                        if !segment.is_empty() {
                            let s = String::from_utf8_lossy(&segment).trim().to_string();
                            segment.clear();
                            if !s.is_empty() {
                                if let Some((bytes, pct, speed)) = parse_rsync_progress2(&s) {
                                    emit_sync_bytes(&phase, bytes, pct, &speed);
                                } else {
                                    stats_output.push_str(&s);
                                    stats_output.push('\n');
                                }
                            }
                        }
                    } else {
                        segment.push(byte);
                    }
                }
                len
            };
            reader.consume(consumed);
        }

        // Flush any trailing segment (stats line without trailing newline)
        if !segment.is_empty() {
            let s = String::from_utf8_lossy(&segment).trim().to_string();
            if !s.is_empty() {
                stats_output.push_str(&s);
                stats_output.push('\n');
            }
        }
        stats_output
    });

    // Read stderr in a separate thread to prevent pipe deadlock
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        BufReader::new(stderr).read_to_string(&mut buf).ok();
        buf
    });

    let status = child.wait().map_err(|e| format!("rsync wait: {}", e))?;
    let stdout_str = stdout_thread.join().unwrap_or_default();
    let stderr_str = stderr_thread.join().unwrap_or_default();
    let exit_code = status.code().unwrap_or(-1);

    if exit_code != 0 && exit_code != 24 {
        // Retry once via blocking run_rsync on type-conflicts
        if exit_code == 23 {
            let rels = parse_rsync_make_way_paths(&stderr_str);
            if !rels.is_empty() {
                remove_rsync_conflicts(dst, &rels);
                if let Ok(r) = run_rsync(src, dst, excludes, delete) { return Ok(r); }
            }
        }
        return Err(format!("rsync {} -> {}: exit={} {}", src, dst, exit_code, truncate_stderr(&stderr_str, 20)));
    }
    Ok(CommandResult { success: true, stdout: stdout_str, stderr: stderr_str, exit_code })
}

/// Parse an rsync --info=progress2 progress line.
/// Format: "  1,234,567  45%  12.34MB/s  0:00:17 (xfr#1, to-chk=N/T)"
fn parse_rsync_progress2(line: &str) -> Option<(u64, u8, String)> {
    let trimmed = line.trim();
    if !trimmed.contains('%') || !trimmed.contains("to-chk=") {
        return None;
    }
    let pct_idx = trimmed.find('%')?;
    let before = &trimmed[..pct_idx];
    let tokens: Vec<&str> = before.split_whitespace().collect();
    let pct: u8 = tokens.last()?.parse().ok()?;
    let bytes: u64 = tokens.first()?.replace(',', "").parse().ok()?;
    let after = trimmed[pct_idx + 1..].trim();
    let speed = after.split_whitespace().next().unwrap_or_default().to_string();
    Some((bytes, pct, speed))
}

// ─── Sync Status ──────────────────────────────────────────────

/// Pass `boot_uuid` if already known to avoid a redundant `findmnt` subprocess call.
pub(crate) fn get_sync_status_internal(c: &AppConfig, boot_uuid: Option<&str>) -> SyncStatus {
    let timer = run_cmd("systemctl", &["is-active", &c.sync.timer_unit]);
    let timer_active = timer.stdout.trim() == "active";

    let timer_info = run_cmd(
        "systemctl",
        &[
            "show",
            &c.sync.timer_unit,
            "--property=NextElapseUSecRealtime,LastTriggerUSec",
            "--no-pager",
        ],
    );
    let mut timer_next = None;
    let mut timer_last_trigger = None;
    for line in timer_info.stdout.lines() {
        if let Some(val) = line.strip_prefix("NextElapseUSecRealtime=") {
            if !val.is_empty() {
                timer_next = Some(val.to_string());
            }
        } else if let Some(val) = line.strip_prefix("LastTriggerUSec=") {
            if !val.is_empty() {
                timer_last_trigger = Some(val.to_string());
            }
        }
    }

    let log_content = fs::read_to_string(&c.sync.log_path).unwrap_or_default();
    let all_lines: Vec<&str> = log_content.lines().collect();
    let log_lines: Vec<String> = all_lines
        .iter()
        .rev()
        .take(SYNC_STATUS_LOG_TAIL)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|l| (*l).to_string())
        .collect();

    let last_sync = log_lines
        .iter()
        .rev()
        .find(|l| l.contains("Sync fertig"))
        .cloned();

    let last_duration = last_sync.as_ref().and_then(|line| {
        line.find("(Dauer: ").map(|start| {
            let rest = &line[start + 8..];
            rest.trim_end_matches(" ===")
                .trim_end_matches(')')
                .to_string()
        })
    });

    let boot_uuid = boot_uuid.map_or_else(get_boot_uuid, std::string::ToString::to_string);
    let direction = format!(
        "{} -> {}",
        disk_label(&boot_uuid, c),
        if boot_uuid == c.disks.primary_uuid {
            &c.disks.backup_label
        } else {
            &c.disks.primary_label
        }
    );

    SyncStatus {
        last_sync,
        last_duration,
        timer_active,
        timer_next,
        timer_last_trigger,
        direction,
        log_tail: log_lines,
        sync_running: SYNC_RUNNING.load(Ordering::SeqCst),
    }
}

// ─── Tauri Commands ───────────────────────────────────────────

#[tauri::command]
pub async fn run_sync(app: tauri::AppHandle) -> Result<CommandResult, String> {
    if SYNC_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Sync läuft bereits".to_string());
    }

    let result = tokio::task::spawn_blocking(move || run_sync_elevated(&app))
        .await
        .map_err(|e| format!("Spawn error: {}", e))
        .and_then(|r| r);

    SYNC_RUNNING.store(false, Ordering::SeqCst);
    invalidate_caches();
    result
}

/// Spawn a single `pkexec backsnap --sync-elevated` subprocess and relay
/// JSON progress lines as Tauri events. **One pkexec prompt** for the entire sync.
fn run_sync_elevated(app: &tauri::AppHandle) -> Result<CommandResult, String> {
    relay_elevated_subprocess(app, &["--sync-elevated"])
}

// ─── Shared Sync Core ─────────────────────────────────────────

/// Determines how sync progress is reported and which rsync variant to use.
pub(super) enum SyncMode {
    /// Elevated subprocess (GUI): JSON progress to stdout, streaming rsync with byte progress.
    Elevated,
    /// Headless (CLI/systemd): human-readable println, blocking rsync.
    Headless,
}

impl SyncMode {
    #[allow(clippy::print_stdout)]
    fn progress(&self, step: &str, detail: &str, pct: u8) {
        match self {
            Self::Elevated => emit_sync_progress(step, detail, pct),
            Self::Headless => println!("backsnap: {}", detail),
        }
    }

    fn rsync(
        &self, src: &str, dst: &str, excludes: &[String], delete: bool, phase: &str,
    ) -> Result<CommandResult, String> {
        match self {
            Self::Elevated => run_rsync_streaming(src, dst, excludes, delete, phase),
            Self::Headless => run_rsync(src, dst, excludes, delete),
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Elevated => "",
            Self::Headless => " (CLI)",
        }
    }
}

/// Map percentage index to progress value in [10..95].
fn phase_pct(i: usize, per_phase: f32) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    { (10.0 + (i as f32 * per_phase)).min(95.0) as u8 }
}

/// Build the exclude list for a subvolume based on its source mount.
fn excludes_for_subvol(
    source: &str, c: &AppConfig, log_path: &str, log_prefix: &str,
) -> Vec<String> {
    let home_mp = super::helpers::get_home_mountpoint();
    let home_trimmed = home_mp.trim_end_matches('/');

    let raw: Vec<String> = if source == "/" {
        c.sync.system_excludes.clone()
    } else if source == home_trimmed || source == home_mp {
        let mut exc = c.sync.home_excludes.clone();
        if c.sync.extra_excludes_on_primary {
            exc.extend(c.sync.home_extra_excludes.clone());
            sync_log(log_path, &format!("{} (mit Extra-Excludes)", log_prefix));
        }
        exc
    } else {
        Vec::new()
    };
    let clean = sanitize_excludes(&raw);
    if clean.len() != raw.len() {
        sync_log(log_path, &format!("{}: Excludes normalisiert ({} -> {})", log_prefix, raw.len(), clean.len()));
    }
    clean
}

/// Patch fstab + create shared-subvolume mount dirs after syncing root.
fn post_root_sync(mnt: &str, c: &AppConfig, backup_dev: &str) {
    let backup_efi_dev = derive_efi_partition(backup_dev);
    let primary_dev = crate::sysfs::resolve_uuid(&c.disks.primary_uuid).unwrap_or_default();
    let primary_efi_dev = derive_efi_partition(&primary_dev);
    let backup_efi_uuid = get_partition_uuid(&backup_efi_dev);
    let primary_efi_uuid = get_partition_uuid(&primary_efi_dev);

    if let Err(e) = patch_backup_fstab(
        mnt, &c.disks.primary_uuid, &c.disks.backup_uuid,
        &primary_efi_uuid, &backup_efi_uuid,
        &c.sync.shared_subvolumes, &c.sync.log_path,
    ) {
        sync_log(&c.sync.log_path, &format!("WARNUNG fstab-Patch: {}", e));
    }

    // Create mount points for shared subvolumes (excluded from rsync under /mnt/*).
    for sv in &c.sync.shared_subvolumes {
        let dir = format!("{}/mnt/{}", mnt, sv.trim_start_matches('@'));
        if let Err(e) = fs::create_dir_all(&dir) {
            sync_log(&c.sync.log_path, &format!("WARNUNG: Mountpoint {} erstellen: {}", dir, e));
        }
    }
}

/// Sync boot partition: rsync, bootctl update, patch entries, write cross-entries.
fn sync_boot(c: &AppConfig, mode: &SyncMode, backup_dev: &str) {
    let backup_efi = derive_efi_partition(backup_dev);
    let boot_mnt = BOOT_MOUNT;
    let _ = fs::create_dir_all(boot_mnt);

    let mount_res = run_privileged("mount", &["-o", "rw", &backup_efi, boot_mnt]);
    if !mount_res.success {
        sync_log(&c.sync.log_path, &format!("WARNUNG: Konnte Backup-EFI {} nicht mounten: {}", backup_efi, mount_res.stderr));
        return;
    }
    let _boot_guard = AutoUmount(boot_mnt.to_string());

    // Cross-entries are managed separately — exclude from rsync.
    let mut excludes = sanitize_excludes(&c.boot.excludes);
    excludes.push("*-cross-*".to_string());

    if let Err(e) = mode.rsync("/boot/", &format!("{}/", boot_mnt), &excludes, true, "boot") {
        sync_log(&c.sync.log_path, &format!("WARNUNG Boot-Sync: {}", e));
    } else {
        sync_log(&c.sync.log_path, "Boot-Dateien OK.");
    }

    // Update systemd-boot on backup ESP
    let bl = run_privileged("bootctl", &["update", &format!("--esp-path={}", boot_mnt), "--no-variables"]);
    let bl_err = bl.stderr.trim();
    if bl.success {
        sync_log(&c.sync.log_path, "Bootloader-Update (systemd-boot) auf Backup-EFI OK.");
    } else if bl_err.contains("same boot loader version in place already") {
        sync_log(&c.sync.log_path, &format!("Bootloader-Update: bereits aktuell. {}", bl_err));
    } else {
        sync_log(&c.sync.log_path, &format!("WARNUNG Bootloader-Update: {}", bl_err));
    }

    if let Err(e) = patch_backup_boot_entries(
        boot_mnt, &c.disks.primary_uuid, &c.disks.backup_uuid, &c.disks.backup_label, &c.sync.log_path,
    ) {
        sync_log(&c.sync.log_path, &format!("WARNUNG Boot-Entry-Patch: {}", e));
    }

    write_cross_boot_entries(
        boot_mnt, &c.disks.primary_uuid, &c.disks.backup_uuid,
        &c.disks.primary_label, &c.disks.backup_label, &c.sync.log_path,
    );
}

/// Core sync logic shared by elevated (GUI) and headless (CLI/systemd) modes.
/// Sync a single subvolume: mount → rsync → post-hooks → cleanup → unmount.
fn sync_single_subvol(
    sv: &crate::config::SubvolSync,
    ctx: &SyncContext,
    c: &AppConfig,
    mode: &SyncMode,
    n: usize,
    total: usize,
    ppp: f32,
) -> Result<(), String> {
    let mnt = format!("{}-{}", ctx.mount_base, sv.name);
    let log_prefix = format!("Phase {}/{}: {}", n, total, sv.source);

    mode.progress(&sv.name, &format!("[{n}/{total}] {} synchronisieren...", sv.source), phase_pct(n - 1, ppp));
    sync_log(&c.sync.log_path, &format!("{} ...", log_prefix));

    mount_subvol(&ctx.backup_dev, &mnt, &sv.subvol, &c.sync.mount_options)
        .map_err(|e| { sync_log(&c.sync.log_path, &format!("FEHLER mount {}: {}", sv.subvol, e)); e })?;
    let _umount = AutoUmount(mnt.clone());

    let excludes = excludes_for_subvol(&sv.source, c, &c.sync.log_path, &log_prefix);
    let src = if sv.source.ends_with('/') { sv.source.clone() } else { format!("{}/", sv.source) };

    match mode.rsync(&src, &format!("{}/", mnt), &excludes, sv.delete, &sv.name) {
        Ok(r) => {
            let stats: Vec<&str> = r.stdout.lines()
                .filter(|l| l.contains("bytes") || l.contains("transferred")).collect();
            let msg = format!("{} OK. {}", sv.name, stats.join(" | "));
            sync_log(&c.sync.log_path, &msg);
            #[allow(clippy::print_stdout)]
            if matches!(mode, SyncMode::Headless) { println!("backsnap: {}", msg); }
        }
        Err(e) => {
            sync_log(&c.sync.log_path, &format!("FEHLER {}-Sync: {}", sv.name, e));
            return Err(e);
        }
    }

    if sv.source == "/" { post_root_sync(&mnt, c, &ctx.backup_dev); }
    cleanup_backup_snapshots(&mnt, &sv.name, &c.sync.log_path);
    mode.progress(&sv.name, &format!("{} fertig", sv.name), phase_pct(n, ppp));
    Ok(())
}

pub(super) fn sync_core(c: &AppConfig, mode: &SyncMode) -> Result<CommandResult, String> {
    let start_time = Instant::now();
    rotate_log(&c.sync.log_path, c.sync.log_max_lines);

    mode.progress("init", "Preflight-Check...", 0);
    if !cmd_exists("rsync") { return Err("rsync nicht installiert".to_string()); }

    let ctx = detect_sync_direction(c)?;
    sync_log(&c.sync.log_path, &format!("=== Sync Start{}: {} ===", mode.label(), ctx.direction));
    mode.progress("init", &format!("Richtung: {}", ctx.direction), 5);

    let sv_names: Vec<&str> = c.sync.subvolumes.iter().map(|sv| sv.subvol.as_str()).collect();
    ensure_backup_subvolumes(&ctx.backup_dev, &sv_names, &c.sync.mount_options, &c.sync.log_path);

    let total = c.sync.subvolumes.len() + usize::from(c.boot.sync_enabled);
    #[allow(clippy::cast_precision_loss)]
    let ppp = if total > 0 { 80.0_f32 / total as f32 } else { 80.0 };

    // ── Sync subvolumes ──
    for (i, sv) in c.sync.subvolumes.iter().enumerate() {
        sync_single_subvol(sv, &ctx, c, mode, i + 1, total, ppp)?;
    }

    // ── Boot sync (optional) ──
    if c.boot.sync_enabled {
        let _efi_lock = EfiMountLock::acquire().map_err(|e| format!("EFI-Lock fehlgeschlagen: {}", e))?;
        mode.progress("boot", &format!("[{total}/{total}] Boot synchronisieren..."), phase_pct(c.sync.subvolumes.len(), ppp));
        sync_log(&c.sync.log_path, &format!("Phase {t}/{t}: Sync /boot ...", t = total));
        sync_boot(c, mode, &ctx.backup_dev);
    }

    let dur = format_duration(start_time.elapsed().as_secs());
    mode.progress("done", &format!("Sync abgeschlossen in {}", dur), 100);
    sync_log(&c.sync.log_path, &format!("=== Sync fertig{}: {} (Dauer: {}) ===", mode.label(), ctx.direction, dur));

    Ok(CommandResult {
        success: true,
        stdout: format!("Sync abgeschlossen: {} (Dauer: {})", ctx.direction, dur),
        stderr: String::new(),
        exit_code: 0,
    })
}

/// GUI elevated sync entry point — calls sync_core with streaming rsync + JSON progress.
pub(super) fn do_sync() -> Result<CommandResult, String> {
    let _lock = SyncLock::acquire()?;
    let c = cfg();
    validate_sync_config(&c)?;
    sync_core(&c, &SyncMode::Elevated)
}

#[tauri::command]
pub async fn get_sync_status() -> Result<SyncStatus, String> {
    tokio::task::spawn_blocking(|| {
        let c = cfg();
        Ok(get_sync_status_internal(&c, None))
    })
    .await
    .map_err(|e| format!("Sync-Status thread panicked: {}", e))?
}

/// Maximum number of log lines returned to the frontend.
/// Keeps IPC payload small and prevents the UI from choking on huge logs.
const SYNC_LOG_MAX_TAIL: usize = 500;

#[tauri::command]
pub async fn get_sync_log() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(|| {
        let c = cfg();
        let content = fs::read_to_string(&c.sync.log_path)
            .unwrap_or_else(|_| "Log nicht vorhanden".to_string());
        let all: Vec<String> = content.lines().map(std::string::ToString::to_string).collect();
        if all.len() > SYNC_LOG_MAX_TAIL {
            Ok(all[all.len() - SYNC_LOG_MAX_TAIL..].to_vec())
        } else {
            Ok(all)
        }
    })
    .await
    .map_err(|e| format!("Sync-Log thread panicked: {}", e))?
}

/// TTL for btrfs usage cache: 5 minutes.
const BTRFS_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(300);

#[tauri::command]
pub async fn get_btrfs_usage() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        if let Some(cached) = BTRFS_USAGE_CACHE.get(BTRFS_CACHE_TTL) {
            return Ok(cached);
        }
        let result = run_privileged("btrfs", &["filesystem", "usage", "/", "--human-readable"]);
        if result.success {
            BTRFS_USAGE_CACHE.set(result.stdout.clone());
            Ok(result.stdout)
        } else {
            Err(result.stderr)
        }
    })
    .await
    .map_err(|e| format!("Btrfs-Thread panicked: {}", e))?
}

#[tauri::command]
pub async fn get_system_monitor() -> Result<crate::sysmon::SystemMonitorData, String> {
    tokio::task::spawn_blocking(|| Ok(crate::sysmon::read_system_monitor()))
        .await
        .map_err(|e| format!("Monitor-Thread panicked: {}", e))?
}

// ─── Elevated Sync CLI ────────────────────────────────────────

/// CLI entry point for `pkexec backsnap --sync-elevated`.
/// Runs the full sync as root, streaming JSON progress lines to stdout.
/// The GUI reads these lines and relays them as Tauri events.
///
/// `config_path_override`: if set, load config from this path instead of the
/// default (which would resolve to `/root/.config/...` under pkexec).
#[allow(clippy::print_stderr, clippy::needless_pass_by_value)]
pub fn run_sync_elevated_cli(config_path_override: Option<String>) -> i32 {
    if let Err(e) = preload_cli_config(config_path_override.as_deref()) {
        eprintln!("backsnap: {}", e);
        return 1;
    }
    emit_cli_result(do_sync(), "FEHLER (elevated)")
}

// ─── Unit Tests ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_rsync_progress2 ──────────────────────────────────

    #[test]
    fn progress2_parses_mid_transfer() {
        let line = "    1,234,567  45%  12.34MB/s    0:00:17 (xfr#1, to-chk=456/789)";
        let (bytes, pct, speed) = parse_rsync_progress2(line).expect("should parse");
        assert_eq!(bytes, 1_234_567);
        assert_eq!(pct, 45);
        assert_eq!(speed, "12.34MB/s");
    }

    #[test]
    fn progress2_parses_100_percent() {
        let line = "  5,368,709,120 100%  98.76MB/s    0:00:52 (xfr#892, to-chk=0/1024)";
        let (bytes, pct, _) = parse_rsync_progress2(line).expect("should parse");
        assert_eq!(pct, 100);
        assert_eq!(bytes, 5_368_709_120);
    }

    #[test]
    fn progress2_rejects_stats_line() {
        assert!(parse_rsync_progress2("Number of files: 1,234 (reg: 1,100)").is_none());
        assert!(parse_rsync_progress2("Total file size: 50,000 bytes").is_none());
        assert!(parse_rsync_progress2("").is_none());
        assert!(parse_rsync_progress2("sending incremental file list").is_none());
    }

    #[test]
    fn progress2_rejects_line_without_to_chk() {
        // Line has % but no to-chk= — should not be treated as progress
        assert!(parse_rsync_progress2("  50% done approximately").is_none());
    }

    // ── sanitize_excludes ─────────────────────────────────────

    #[test]
    fn sanitize_removes_duplicates() {
        let input = vec!["/foo".to_string(), "/bar".to_string(), "/foo".to_string()];
        let out = sanitize_excludes(&input);
        assert_eq!(out.len(), 2);
        assert!(out.contains(&"/foo".to_string()));
        assert!(out.contains(&"/bar".to_string()));
    }

    #[test]
    fn sanitize_strips_empty_entries() {
        let input = vec!["/foo".to_string(), "".to_string(), "  ".to_string()];
        let out = sanitize_excludes(&input);
        assert_eq!(out, vec!["/foo".to_string()]);
    }

    // ── detect_sync_direction error cases ────────────────────

    #[test]
    fn direction_rejects_empty_uuids() {
        let cfg = crate::config::AppConfig {
            disks: crate::config::DiskConfig {
                primary_uuid: String::new(),
                primary_label: String::new(),
                backup_uuid: String::new(),
                backup_label: String::new(),
            },
            sync: crate::config::SyncConfig {
                timer_unit: String::new(),
                service_unit: String::new(),
                log_path: String::new(),
                log_max_lines: 0,
                mount_options: String::new(),
                mount_base: String::new(),
                subvolumes: vec![],
                system_excludes: vec![],
                home_excludes: vec![],
                home_extra_excludes: vec![],
                extra_excludes_on_primary: false,
                shared_subvolumes: vec![],
            },
            boot: crate::config::BootConfig {
                sync_enabled: false,
                bootloader_type: crate::config::BootloaderType::SystemdBoot,
                excludes: vec![],
            },
            snapper: crate::config::SnapperConfig {
                expected_configs: vec![],
            },
            rollback: crate::config::RollbackConfig {
                max_broken_backups: 2,
                recovery_label: String::new(),
                root_subvol: "@".to_string(),
                root_config: "root".to_string(),
            },
        };
        let result = detect_sync_direction(&cfg);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("konfiguriert"),
            "expected config error, got: {}",
            msg
        );
    }
}
