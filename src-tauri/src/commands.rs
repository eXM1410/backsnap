use crate::config::{self, AppConfig};
use crate::sysmon;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tauri::Emitter;

static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);

// ─── Data Types ───────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub uuid: String,
    pub size: String,
    pub mountpoint: String,
    pub fstype: String,
    pub used: String,
    pub avail: String,
    pub use_percent: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Snapshot {
    pub id: u32,
    pub snap_type: String,
    pub pre_id: Option<u32>,
    pub date: String,
    pub user: String,
    pub cleanup: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SyncStatus {
    pub last_sync: Option<String>,
    pub last_duration: Option<String>,
    pub timer_active: bool,
    pub timer_next: Option<String>,
    pub timer_last_trigger: Option<String>,
    pub direction: String,
    pub log_tail: Vec<String>,
    pub sync_running: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SystemStatus {
    pub hostname: String,
    pub kernel: String,
    pub uptime: String,
    pub boot_disk: String,
    pub boot_uuid: String,
    pub disks: Vec<DiskInfo>,
    pub snapper_configs: Vec<String>,
    pub snapshot_counts: Vec<SnapshotCount>,
    pub sync_status: SyncStatus,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SnapshotCount {
    pub config: String,
    pub count: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TimerConfig {
    pub enabled: bool,
    pub calendar: String,
    pub randomized_delay: String,
    pub last_trigger: Option<String>,
    pub service_result: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommandResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HealthCheck {
    pub primary_present: bool,
    pub backup_present: bool,
    pub snapper_installed: bool,
    pub rsync_installed: bool,
    pub btrfs_tools: bool,
    pub boot_disk: String,
    pub issues: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubvolumeInfo {
    pub id: String,
    pub gen: String,
    pub top_level: String,
    pub path: String,
}

// ─── Helpers ──────────────────────────────────────────────────

fn run_cmd(cmd: &str, args: &[&str]) -> CommandResult {
    match Command::new(cmd).args(args).output() {
        Ok(output) => CommandResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        },
        Err(e) => CommandResult {
            success: false,
            stdout: String::new(),
            stderr: format!("Failed to execute {}: {}", cmd, e),
            exit_code: -1,
        },
    }
}

fn run_privileged(cmd: &str, args: &[&str]) -> CommandResult {
    if is_root() {
        // Already root (e.g. systemd service) — run directly
        run_cmd(cmd, args)
    } else {
        let mut full_args = vec![cmd];
        full_args.extend_from_slice(args);
        run_cmd("pkexec", &full_args)
    }
}

fn is_root() -> bool {
    Command::new("id")
        .args(["-u"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false)
}

fn read_proc(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn get_boot_uuid() -> String {
    run_cmd("findmnt", &["/", "-o", "UUID", "-n"])
        .stdout
        .trim()
        .to_string()
}

fn cfg() -> AppConfig {
    config::load_config().unwrap_or_else(|_| config::auto_detect_config())
}

fn validate_config(config: &str) -> Result<(), String> {
    if config.is_empty() || config.len() > 64 {
        return Err("Ungültiger Config-Name: leer oder zu lang".to_string());
    }
    if !config
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Ungültiger Config-Name: '{}' — nur a-z, 0-9, -, _ erlaubt",
            config
        ));
    }
    Ok(())
}

fn validate_description(desc: &str) -> Result<(), String> {
    if desc.len() > 256 {
        return Err("Beschreibung zu lang (max 256 Zeichen)".to_string());
    }
    let forbidden = ['`', '$', '\\', '|', ';', '&', '<', '>', '\n', '\r', '\0'];
    if desc.chars().any(|c| forbidden.contains(&c)) {
        return Err("Beschreibung enthält ungültige Zeichen".to_string());
    }
    Ok(())
}

fn sync_log(log_path: &str, msg: &str) {
    let timestamp = chrono::Local::now().format("%F %T").to_string();
    let line = format!("[{}] {}\n", timestamp, msg);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

fn rotate_log(log_path: &str, max_lines: usize) {
    let content = match fs::read_to_string(log_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > max_lines {
        let keep = &lines[lines.len() - max_lines..];
        let new_content = keep.join("\n") + "\n";
        let _ = fs::write(log_path, new_content);
    }
}

fn emit_progress(app: &tauri::AppHandle, step: &str, detail: &str, pct: u8) {
    let _ = app.emit(
        "sync-progress",
        serde_json::json!({
            "step": step,
            "detail": detail,
            "percent": pct,
        }),
    );
}

fn format_duration(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

fn cmd_exists(cmd: &str) -> bool {
    run_cmd("which", &[cmd]).success
}

fn disk_label(uuid: &str, c: &AppConfig) -> String {
    if uuid == c.disks.primary_uuid {
        c.disks.primary_label.clone()
    } else if uuid == c.disks.backup_uuid {
        c.disks.backup_label.clone()
    } else {
        format!("Unknown ({})", uuid)
    }
}

// ─── Config Management Commands ───────────────────────────────

#[tauri::command]
pub fn get_config() -> Result<AppConfig, String> {
    config::load_config()
}

#[tauri::command]
pub fn save_config_cmd(new_config: AppConfig) -> Result<(), String> {
    config::save_config(&new_config)
}

#[tauri::command]
pub fn detect_disks() -> Result<Vec<config::DetectedDisk>, String> {
    Ok(config::detect_btrfs_disks())
}

#[tauri::command]
pub fn reset_config() -> Result<AppConfig, String> {
    let config = config::auto_detect_config();
    config::save_config(&config)?;
    Ok(config)
}

// ─── Health Check ─────────────────────────────────────────────

#[tauri::command]
pub fn get_health() -> Result<HealthCheck, String> {
    let c = cfg();
    let mut issues = Vec::new();

    let snapper_installed = cmd_exists("snapper");
    let rsync_installed = cmd_exists("rsync");
    let btrfs_tools = cmd_exists("btrfs");
    if !snapper_installed {
        issues.push("snapper nicht installiert".to_string());
    }
    if !rsync_installed {
        issues.push("rsync nicht installiert".to_string());
    }
    if !btrfs_tools {
        issues.push("btrfs-progs nicht installiert".to_string());
    }

    let primary_present = if c.disks.primary_uuid.is_empty() {
        issues.push("Primary Disk nicht konfiguriert".to_string());
        false
    } else {
        let r = run_cmd("blkid", &["-U", &c.disks.primary_uuid]).success;
        if !r {
            issues.push(format!("{} nicht erkannt", c.disks.primary_label));
        }
        r
    };

    let backup_present = if c.disks.backup_uuid.is_empty() {
        issues.push("Backup Disk nicht konfiguriert".to_string());
        false
    } else {
        let r = run_cmd("blkid", &["-U", &c.disks.backup_uuid]).success;
        if !r {
            issues.push(format!("{} nicht erkannt", c.disks.backup_label));
        }
        r
    };

    let boot_uuid = get_boot_uuid();
    let boot_disk = disk_label(&boot_uuid, &c);

    if !c.disks.backup_uuid.is_empty() && boot_uuid == c.disks.backup_uuid {
        issues.push("ACHTUNG: Gebootet von Backup-Disk!".to_string());
    }

    if snapper_installed {
        let configs = get_snapper_configs();
        for expected in &c.snapper.expected_configs {
            if !configs.contains(expected) {
                issues.push(format!("Snapper-Config '{}' fehlt", expected));
            }
        }
    }

    let timer = run_cmd("systemctl", &["is-active", &c.sync.timer_unit]);
    if timer.stdout.trim() != "active" {
        issues.push(format!("{} nicht aktiv", c.sync.timer_unit));
    }

    Ok(HealthCheck {
        primary_present,
        backup_present,
        snapper_installed,
        rsync_installed,
        btrfs_tools,
        boot_disk,
        issues,
    })
}

// ─── System Status ────────────────────────────────────────────

#[tauri::command]
pub fn get_system_status() -> Result<SystemStatus, String> {
    let c = cfg();
    let hostname = read_proc("/proc/sys/kernel/hostname");
    let kernel = read_proc("/proc/sys/kernel/osrelease");

    let uptime_raw = read_proc("/proc/uptime");
    let uptime_secs: f64 = uptime_raw
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let days = (uptime_secs / 86400.0) as u64;
    let hours = ((uptime_secs % 86400.0) / 3600.0) as u64;
    let mins = ((uptime_secs % 3600.0) / 60.0) as u64;
    let uptime = if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    };

    let boot_uuid = get_boot_uuid();
    let boot_disk = disk_label(&boot_uuid, &c);

    let disks = get_disk_info();
    let snapper_configs = get_snapper_configs();
    let mut snapshot_counts = Vec::new();
    for config in &snapper_configs {
        let count = get_snapshot_count(config);
        snapshot_counts.push(SnapshotCount {
            config: config.clone(),
            count,
        });
    }
    let sync_status = get_sync_status_internal(&c);

    Ok(SystemStatus {
        hostname,
        kernel,
        uptime,
        boot_disk,
        boot_uuid,
        disks,
        snapper_configs,
        snapshot_counts,
        sync_status,
    })
}

fn get_disk_info() -> Vec<DiskInfo> {
    let result = run_cmd(
        "df",
        &[
            "-h",
            "--output=source,fstype,size,used,avail,pcent,target",
            "-t", "btrfs",
            "-t", "vfat",
        ],
    );
    let uuid_result = run_cmd(
        "findmnt",
        &["-t", "btrfs,vfat", "-o", "TARGET,UUID", "-n", "-l"],
    );
    let uuid_map: HashMap<String, String> = uuid_result
        .stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                None
            }
        })
        .collect();

    let mut disks = Vec::new();
    for line in result.stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 7 {
            let mountpoint = parts[6..].join(" ");
            let uuid = uuid_map.get(&mountpoint).cloned().unwrap_or_default();
            disks.push(DiskInfo {
                name: parts[0].to_string(),
                fstype: parts[1].to_string(),
                size: parts[2].to_string(),
                used: parts[3].to_string(),
                avail: parts[4].to_string(),
                use_percent: parts[5].to_string(),
                mountpoint,
                uuid,
            });
        }
    }
    disks
}

fn get_snapper_configs() -> Vec<String> {
    let result = run_cmd("snapper", &["list-configs", "--columns", "config"]);
    result
        .stdout
        .lines()
        .skip(2)
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn get_snapshot_count(config: &str) -> u32 {
    let result = run_cmd("snapper", &["-c", config, "list", "--columns", "number"]);
    result
        .stdout
        .lines()
        .skip(2)
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && t != "0" && t.parse::<u32>().is_ok()
        })
        .count() as u32
}

fn get_sync_status_internal(c: &AppConfig) -> SyncStatus {
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
        .take(50)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|l| l.to_string())
        .collect();

    let last_sync = log_lines
        .iter()
        .rev()
        .find(|l| l.contains("Sync fertig"))
        .cloned();

    let last_duration = last_sync.as_ref().and_then(|line| {
        line.find("(Dauer: ").map(|start| {
            let rest = &line[start + 8..];
            rest.trim_end_matches(')')
                .trim_end_matches(" ===")
                .to_string()
        })
    });

    let boot_uuid = get_boot_uuid();
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

// ─── Btrfs Subvolumes ────────────────────────────────────────

#[tauri::command]
pub fn get_subvolumes() -> Result<Vec<SubvolumeInfo>, String> {
    let result = run_privileged("btrfs", &["subvolume", "list", "-t", "/"]);
    if !result.success {
        return Err(format!("btrfs subvolume list: {}", result.stderr));
    }

    let mut subvols = Vec::new();
    for line in result.stdout.lines().skip(2) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 4 {
            subvols.push(SubvolumeInfo {
                id: cols[0].to_string(),
                gen: cols[1].to_string(),
                top_level: cols[2].to_string(),
                path: cols[3..].join(" "),
            });
        }
    }
    Ok(subvols)
}

// ─── Snapshots ────────────────────────────────────────────────

#[tauri::command]
pub fn get_snapshots(config: String) -> Result<Vec<Snapshot>, String> {
    validate_config(&config)?;

    let result = run_cmd("snapper", &["-c", &config, "list", "--csvout"]);
    if !result.success {
        return Err(format!("snapper error: {}", result.stderr));
    }

    let mut snapshots = Vec::new();
    let mut lines = result.stdout.lines();
    let header = lines.next().unwrap_or("");
    let headers: Vec<&str> = header.split(',').collect();

    let idx = |name: &str| headers.iter().position(|h| h.trim() == name);
    let i_num = idx("number").or_else(|| idx("#")).unwrap_or(0);
    let i_type = idx("type").unwrap_or(1);
    let i_pre = idx("pre-number").unwrap_or(2);
    let i_date = idx("date").unwrap_or(3);
    let i_user = idx("user").unwrap_or(4);
    let i_cleanup = idx("cleanup").unwrap_or(5);
    let i_desc = idx("description").unwrap_or(6);

    for line in lines {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 4 {
            continue;
        }
        let get = |i: usize| cols.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
        let id = get(i_num).parse::<u32>().unwrap_or(0);
        if id == 0 {
            continue;
        }
        snapshots.push(Snapshot {
            id,
            snap_type: get(i_type),
            pre_id: get(i_pre).parse::<u32>().ok(),
            date: get(i_date),
            user: get(i_user),
            cleanup: get(i_cleanup),
            description: get(i_desc),
        });
    }
    Ok(snapshots)
}

#[tauri::command]
pub fn create_snapshot(config: String, description: String) -> Result<CommandResult, String> {
    validate_config(&config)?;
    validate_description(&description)?;
    Ok(run_cmd(
        "snapper",
        &["-c", &config, "create", "-d", &description],
    ))
}

#[tauri::command]
pub fn delete_snapshot(config: String, id: u32) -> Result<CommandResult, String> {
    validate_config(&config)?;
    let id_str = id.to_string();
    Ok(run_privileged(
        "snapper",
        &["-c", &config, "delete", &id_str],
    ))
}

#[tauri::command]
pub fn get_snapper_diff(config: String, id: u32) -> Result<String, String> {
    validate_config(&config)?;
    let id_str = id.to_string();
    let range = format!("0..{}", id_str);
    let result = run_cmd("snapper", &["-c", &config, "status", &range]);
    if result.success {
        Ok(result.stdout)
    } else {
        Err(result.stderr)
    }
}

// ─── NVMe Sync (dynamic, config-driven) ──────────────────────

struct SyncContext {
    #[allow(dead_code)]
    backup_uuid: String,
    backup_dev: String,
    mount_base: String,
    direction: String,
    is_primary_boot: bool,
}

fn detect_sync_direction(c: &AppConfig) -> Result<SyncContext, String> {
    if c.disks.primary_uuid.is_empty() || c.disks.backup_uuid.is_empty() {
        return Err(
            "Disks nicht konfiguriert. Bitte unter Einstellungen Primary und Backup Disk wählen."
                .to_string(),
        );
    }

    let current_uuid = get_boot_uuid();
    let is_primary_boot = current_uuid == c.disks.primary_uuid;

    let (backup_uuid, direction) = if is_primary_boot {
        (
            c.disks.backup_uuid.clone(),
            format!("{} -> {}", c.disks.primary_label, c.disks.backup_label),
        )
    } else if current_uuid == c.disks.backup_uuid {
        (
            c.disks.primary_uuid.clone(),
            format!("{} -> {}", c.disks.backup_label, c.disks.primary_label),
        )
    } else {
        return Err(format!(
            "Boot-UUID {} entspricht weder Primary ({}) noch Backup ({}). Bitte Einstellungen prüfen.",
            current_uuid, c.disks.primary_uuid, c.disks.backup_uuid
        ));
    };

    let blkid = run_cmd("blkid", &["-U", &backup_uuid]);
    if !blkid.success || blkid.stdout.trim().is_empty() {
        return Err(format!(
            "Backup-Disk nicht gefunden (UUID: {}). Ist sie eingebaut?",
            backup_uuid
        ));
    }
    let backup_dev = blkid.stdout.trim().to_string();

    Ok(SyncContext {
        backup_uuid,
        backup_dev,
        mount_base: c.sync.mount_base.clone(),
        direction,
        is_primary_boot,
    })
}

fn mount_subvol(dev: &str, mnt: &str, subvol: &str, mount_opts: &str) -> Result<(), String> {
    let _ = run_privileged("mkdir", &["-p", mnt]);

    let check = run_cmd("mountpoint", &["-q", mnt]);
    if check.success {
        return Ok(());
    }

    let opts = format!("subvol=/{},{}", subvol, mount_opts);
    let result = run_privileged("mount", &["-o", &opts, dev, mnt]);
    if !result.success {
        return Err(format!("mount {} -> {}: {}", dev, mnt, result.stderr));
    }
    Ok(())
}

fn safe_umount(mnt: &str) {
    let _ = run_privileged("umount", &["-l", mnt]);
    let _ = run_privileged("rmdir", &[mnt]);
}

fn run_rsync(
    src: &str,
    dst: &str,
    excludes: &[String],
    delete: bool,
) -> Result<CommandResult, String> {
    let mut args: Vec<String> = vec![
        "ionice".to_string(),
        "-c3".to_string(),
        "rsync".to_string(),
        "-aAX".to_string(),
        "--info=stats1".to_string(),
        "--no-inc-recursive".to_string(),
        "--numeric-ids".to_string(),
    ];
    if delete {
        args.push("--delete".to_string());
        args.push("--delete-excluded".to_string());
    }
    for exc in excludes {
        args.push(format!("--exclude={}", exc));
    }
    args.push(src.to_string());
    args.push(dst.to_string());

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let result = run_privileged(&args_ref[0], &args_ref[1..]);

    if !result.success && result.exit_code != 24 {
        return Err(format!(
            "rsync {} -> {}: exit={} {}",
            src, dst, result.exit_code, result.stderr
        ));
    }
    Ok(result)
}

#[tauri::command]
pub async fn run_sync(app: tauri::AppHandle) -> Result<CommandResult, String> {
    if SYNC_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Sync läuft bereits".to_string());
    }

    let result = tokio::task::spawn_blocking(move || do_sync(&app))
        .await
        .map_err(|e| {
            SYNC_RUNNING.store(false, Ordering::SeqCst);
            format!("Spawn error: {}", e)
        })?;

    SYNC_RUNNING.store(false, Ordering::SeqCst);
    result
}

fn do_sync(app: &tauri::AppHandle) -> Result<CommandResult, String> {
    let c = cfg();
    let start_time = Instant::now();
    rotate_log(&c.sync.log_path, c.sync.log_max_lines);

    emit_progress(app, "init", "Preflight-Check...", 0);

    if !cmd_exists("rsync") {
        return Err("rsync nicht installiert".to_string());
    }

    let ctx = detect_sync_direction(&c)?;
    sync_log(&c.sync.log_path, &format!("=== Sync Start: {} ===", ctx.direction));
    emit_progress(app, "init", &format!("Richtung: {}", ctx.direction), 5);

    let total_phases = c.sync.subvolumes.len() + if c.boot.sync_enabled { 1 } else { 0 };
    let pct_per_phase = if total_phases > 0 { 80 / total_phases as u8 } else { 80 };

    // ── Sync each configured subvolume ──
    for (i, sv) in c.sync.subvolumes.iter().enumerate() {
        let phase_num = i + 1;
        let pct_start = 10 + (i as u8 * pct_per_phase);
        let pct_end = 10 + ((i + 1) as u8 * pct_per_phase);
        let mnt = format!("{}-{}", ctx.mount_base, sv.name);

        emit_progress(
            app,
            &sv.name,
            &format!("{} ({}) synchronisieren...", sv.name, sv.source),
            pct_start,
        );
        sync_log(
            &c.sync.log_path,
            &format!("Phase {}/{}: Sync {} ...", phase_num, total_phases, sv.source),
        );

        mount_subvol(&ctx.backup_dev, &mnt, &sv.subvol, &c.sync.mount_options).map_err(|e| {
            sync_log(&c.sync.log_path, &format!("FEHLER mount {}: {}", sv.subvol, e));
            e
        })?;

        // Build excludes for this subvolume
        let excludes: Vec<String> = if sv.source == "/" {
            c.sync.system_excludes.clone()
        } else if sv.source == "/home/" || sv.source == "/home" {
            let mut exc = c.sync.home_excludes.clone();
            if c.sync.extra_excludes_on_primary && ctx.is_primary_boot {
                exc.extend(c.sync.home_extra_excludes.clone());
                sync_log(
                    &c.sync.log_path,
                    &format!("Phase {}/{}: {} (mit Extra-Excludes)", phase_num, total_phases, sv.source),
                );
            }
            exc
        } else {
            Vec::new()
        };

        let src = if sv.source.ends_with('/') {
            sv.source.clone()
        } else {
            format!("{}/", sv.source)
        };

        match run_rsync(&src, &format!("{}/", mnt), &excludes, sv.delete) {
            Ok(r) => {
                let stats: Vec<&str> = r
                    .stdout
                    .lines()
                    .filter(|l| l.contains("bytes") || l.contains("transferred"))
                    .collect();
                sync_log(
                    &c.sync.log_path,
                    &format!("{} OK. {}", sv.name, stats.join(" | ")),
                );
            }
            Err(e) => {
                safe_umount(&mnt);
                sync_log(&c.sync.log_path, &format!("FEHLER {}-Sync: {}", sv.name, e));
                return Err(e);
            }
        }
        safe_umount(&mnt);
        emit_progress(app, &sv.name, &format!("{} fertig", sv.name), pct_end);
    }

    // ── Boot sync (optional) ──
    if c.boot.sync_enabled {
        let boot_pct = 10 + (c.sync.subvolumes.len() as u8 * pct_per_phase);
        emit_progress(app, "boot", "Boot-Dateien synchronisieren...", boot_pct);
        sync_log(
            &c.sync.log_path,
            &format!("Phase {}/{}: Sync /boot ...", total_phases, total_phases),
        );

        let backup_efi = derive_efi_partition(&ctx.backup_dev);
        let boot_mnt = "/tmp/backsnap-boot";
        let _ = run_privileged("mkdir", &["-p", boot_mnt]);

        let mount_res = run_privileged("mount", &[&backup_efi, boot_mnt]);
        if mount_res.success {
            let boot_excludes = c.boot.excludes.clone();
            let boot_exc_refs: Vec<String> = boot_excludes;
            match run_rsync("/boot/", &format!("{}/", boot_mnt), &boot_exc_refs, false) {
                Ok(_) => sync_log(&c.sync.log_path, "Boot OK."),
                Err(e) => sync_log(&c.sync.log_path, &format!("WARNUNG Boot-Sync: {}", e)),
            }
        } else {
            sync_log(
                &c.sync.log_path,
                &format!("WARNUNG: Konnte Backup-EFI {} nicht mounten: {}", backup_efi, mount_res.stderr),
            );
        }
        safe_umount(boot_mnt);
    }

    // ── Done ──
    let elapsed = start_time.elapsed().as_secs();
    let duration_str = format_duration(elapsed);
    emit_progress(
        app,
        "done",
        &format!("Sync abgeschlossen in {}", duration_str),
        100,
    );
    sync_log(
        &c.sync.log_path,
        &format!("=== Sync fertig: {} (Dauer: {}) ===", ctx.direction, duration_str),
    );

    Ok(CommandResult {
        success: true,
        stdout: format!("Sync abgeschlossen: {} (Dauer: {})", ctx.direction, duration_str),
        stderr: String::new(),
        exit_code: 0,
    })
}

fn derive_efi_partition(btrfs_dev: &str) -> String {
    // Find parent disk, then locate EFI System Partition by GUID
    let parent = Command::new("lsblk")
        .args(["-nro", "PKNAME", btrfs_dev])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if !parent.is_empty() {
        // Look for EFI System Partition (GUID c12a7328-f81f-11d2-ba4b-00a0c93ec93b)
        let result = Command::new("lsblk")
            .args(["-nro", "NAME,PARTTYPE", &format!("/dev/{}", parent)])
            .output();
        if let Ok(o) = result {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let parttype = parts[1].to_lowercase();
                        if parttype == "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" {
                            return format!("/dev/{}", parts[0]);
                        }
                    }
                }
            }
        }
    }

    // Fallback: derive from device name (strip trailing digits, append 1)
    if let Some(pos) = btrfs_dev.rfind('p') {
        if btrfs_dev[pos + 1..].chars().all(|c| c.is_ascii_digit()) {
            return format!("{}1", &btrfs_dev[..=pos]);
        }
    }
    let base = btrfs_dev.trim_end_matches(|c: char| c.is_ascii_digit());
    format!("{}1", base)
}

// ─── Rollback (dynamic, config-driven) ───────────────────────

#[tauri::command]
pub async fn rollback_snapshot(
    app: tauri::AppHandle,
    config: String,
    id: u32,
) -> Result<CommandResult, String> {
    validate_config(&config)?;
    if config != "root" {
        return Err("Rollback nur für root-Config unterstützt".to_string());
    }

    tokio::task::spawn_blocking(move || do_rollback(&app, id))
        .await
        .map_err(|e| format!("Spawn error: {}", e))?
}

fn do_rollback(app: &tauri::AppHandle, snap_id: u32) -> Result<CommandResult, String> {
    let c = cfg();
    let boot_uuid = get_boot_uuid();

    emit_progress(
        app,
        "prepare",
        &format!("Rollback auf Snapshot #{} vorbereiten...", snap_id),
        10,
    );

    let tmpdir = "/tmp/backsnap-rollback";
    let _ = fs::create_dir_all(tmpdir);

    let dev_arg = format!("UUID={}", boot_uuid);
    let mount_opts = format!("subvolid=5,{}", c.sync.mount_options);
    let mount_res = run_privileged("mount", &["-o", &mount_opts, &dev_arg, tmpdir]);
    if !mount_res.success {
        return Err(format!(
            "Konnte Btrfs-Root nicht mounten: {}",
            mount_res.stderr
        ));
    }

    let result = do_rollback_inner(app, snap_id, tmpdir, &c);

    let _ = run_privileged("umount", &[tmpdir]);
    let _ = fs::remove_dir(tmpdir);

    result
}

fn do_rollback_inner(
    app: &tauri::AppHandle,
    snap_id: u32,
    tmpdir: &str,
    c: &AppConfig,
) -> Result<CommandResult, String> {
    let root_subvol = &c.rollback.root_subvol;
    let snap_path = format!("{}/.snapshots/{}/snapshot", tmpdir, snap_id);
    if !Path::new(&snap_path).exists() {
        return Err(format!(
            "Snapshot #{} nicht gefunden unter {}",
            snap_id, snap_path
        ));
    }

    let diff = run_cmd(
        "snapper",
        &["-c", "root", "status", &format!("{}..0", snap_id)],
    );
    let diff_count = diff.stdout.lines().count();
    emit_progress(
        app,
        "info",
        &format!("{} geänderte Dateien seit Snapshot #{}", diff_count, snap_id),
        25,
    );

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let broken_name = format!("{}.broken-{}", root_subvol, timestamp);
    let current_at = format!("{}/{}", tmpdir, root_subvol);
    let broken_path = format!("{}/{}", tmpdir, broken_name);

    if !Path::new(&current_at).exists() {
        return Err(format!(
            "Kein aktives {} Subvolume gefunden",
            root_subvol
        ));
    }

    // Cleanup old broken backups (keep last N)
    emit_progress(app, "cleanup", "Alte Backups aufräumen...", 30);
    let pattern = format!("{}/{}.broken-*", tmpdir, root_subvol);
    let ls = run_cmd("ls", &["-d", &pattern]);
    if ls.success {
        let mut broken_dirs: Vec<&str> = ls.stdout.lines().collect();
        broken_dirs.sort();
        while broken_dirs.len() > c.rollback.max_broken_backups {
            let old = broken_dirs.remove(0);
            sync_log(&c.sync.log_path, &format!("Cleanup: lösche altes Backup {}", old));
            let _ = run_privileged("btrfs", &["subvolume", "delete", old]);
        }
    }

    // Move current -> broken
    emit_progress(
        app,
        "backup",
        &format!("Aktuelles {} sichern als {} ...", root_subvol, broken_name),
        50,
    );
    sync_log(
        &c.sync.log_path,
        &format!("Rollback #{}: mv {} -> {}", snap_id, root_subvol, broken_name),
    );

    let mv_res = run_privileged("mv", &[&current_at, &broken_path]);
    if !mv_res.success {
        return Err(format!(
            "Konnte {} nicht verschieben: {}",
            root_subvol, mv_res.stderr
        ));
    }

    // Create writable snapshot as new root
    emit_progress(
        app,
        "snapshot",
        &format!("Snapshot #{} als neues {} erstellen...", snap_id, root_subvol),
        70,
    );
    let snap_res = run_privileged(
        "btrfs",
        &["subvolume", "snapshot", &snap_path, &current_at],
    );
    if !snap_res.success {
        sync_log(&c.sync.log_path, "FEHLER: Snapshot fehlgeschlagen, mache rename rückgängig");
        let _ = run_privileged("mv", &[&broken_path, &current_at]);
        return Err(format!(
            "Konnte Snapshot nicht erstellen: {}",
            snap_res.stderr
        ));
    }

    let snapshots_dir = format!("{}/.snapshots", current_at);
    let _ = run_privileged("mkdir", &["-p", &snapshots_dir]);

    sync_log(
        &c.sync.log_path,
        &format!("Rollback #{} erfolgreich. Backup: {}", snap_id, broken_name),
    );

    emit_progress(
        app,
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
        c.rollback.recovery_label,
        boot_uuid,
        root_subvol, root_subvol,
        broken_name, root_subvol
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

// ─── Sync Status + Log ────────────────────────────────────────

#[tauri::command]
pub fn get_sync_status() -> Result<SyncStatus, String> {
    let c = cfg();
    Ok(get_sync_status_internal(&c))
}

#[tauri::command]
pub fn get_sync_log() -> Result<Vec<String>, String> {
    let c = cfg();
    let content =
        fs::read_to_string(&c.sync.log_path).unwrap_or_else(|_| "Log nicht vorhanden".to_string());
    Ok(content.lines().map(|l| l.to_string()).collect())
}

// ─── Timer ────────────────────────────────────────────────────

#[tauri::command]
pub fn get_timer_config() -> Result<TimerConfig, String> {
    let c = cfg();
    let active = run_cmd("systemctl", &["is-active", &c.sync.timer_unit]);
    let enabled = active.stdout.trim() == "active";

    let props = run_cmd(
        "systemctl",
        &[
            "show",
            &c.sync.timer_unit,
            "--property=TimersCalendar,RandomizedDelayUSec,LastTriggerUSec",
            "--no-pager",
        ],
    );

    let mut calendar = "daily".to_string();
    let mut delay = "1h".to_string();
    let mut last_trigger = None;

    for line in props.stdout.lines() {
        if let Some(val) = line.strip_prefix("TimersCalendar=") {
            calendar = val.split_whitespace().last().unwrap_or("daily").to_string();
        } else if let Some(val) = line.strip_prefix("RandomizedDelayUSec=") {
            delay = val.to_string();
        } else if let Some(val) = line.strip_prefix("LastTriggerUSec=") {
            if !val.is_empty() {
                last_trigger = Some(val.to_string());
            }
        }
    }

    let svc = run_cmd(
        "systemctl",
        &[
            "show",
            &c.sync.service_unit,
            "--property=Result",
            "--no-pager",
        ],
    );
    let service_result = svc
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("Result="))
        .map(|s| s.to_string());

    Ok(TimerConfig {
        enabled,
        calendar,
        randomized_delay: delay,
        last_trigger,
        service_result,
    })
}

#[tauri::command]
pub fn set_timer_enabled(enabled: bool) -> Result<CommandResult, String> {
    let c = cfg();
    let action = if enabled { "enable" } else { "disable" };
    Ok(run_privileged(
        "systemctl",
        &[action, "--now", &c.sync.timer_unit],
    ))
}

// ─── Btrfs ────────────────────────────────────────────────────

#[tauri::command]
pub fn get_btrfs_usage() -> Result<String, String> {
    let result = run_privileged(
        "btrfs",
        &["filesystem", "usage", "/", "--human-readable"],
    );
    if result.success {
        Ok(result.stdout)
    } else {
        Err(result.stderr)
    }
}

#[tauri::command]
pub fn get_system_monitor() -> Result<sysmon::SystemMonitorData, String> {
    Ok(sysmon::read_system_monitor())
}

// ─── Systemd Timer Install/Uninstall ──────────────────────────

fn validate_timer_value(val: &str) -> Result<(), String> {
    if val.is_empty() || val.len() > 128 {
        return Err("Timer-Wert ungültig: leer oder zu lang".to_string());
    }
    let forbidden = ['`', '$', '\\', '|', ';', '&', '<', '>', '\n', '\r', '\0', '\'', '"'];
    if val.chars().any(|c| forbidden.contains(&c)) {
        return Err("Timer-Wert enthält ungültige Zeichen".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn install_timer(calendar: String, delay: String) -> Result<CommandResult, String> {
    validate_timer_value(&calendar)?;
    validate_timer_value(&delay)?;

    let c = cfg();

    // Determine binary path
    let exe = std::env::current_exe()
        .map_err(|e| format!("Binary-Pfad nicht ermittelt: {}", e))?
        .to_string_lossy()
        .to_string();

    let config_path = config::config_path().to_string_lossy().to_string();

    let service_content = format!(
        "[Unit]\n\
         Description=backsnap System Sync\n\
         After=local-fs.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={exe} --sync --config {config}\n\
         Nice=19\n\
         IOSchedulingClass=idle\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        exe = exe,
        config = config_path,
    );

    let timer_content = format!(
        "[Unit]\n\
         Description=backsnap Sync Timer\n\
         \n\
         [Timer]\n\
         OnCalendar={calendar}\n\
         RandomizedDelaySec={delay}\n\
         Persistent=true\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n",
        calendar = calendar,
        delay = delay,
    );

    // Write to temp files, then copy with pkexec
    let tmp_svc = "/tmp/backsnap-install.service";
    let tmp_tmr = "/tmp/backsnap-install.timer";
    fs::write(tmp_svc, &service_content)
        .map_err(|e| format!("Temp-Datei schreiben: {}", e))?;
    fs::write(tmp_tmr, &timer_content)
        .map_err(|e| format!("Temp-Datei schreiben: {}", e))?;

    let svc_path = format!("/etc/systemd/system/{}", c.sync.service_unit);
    let tmr_path = format!("/etc/systemd/system/{}", c.sync.timer_unit);

    let r = run_privileged("cp", &[tmp_svc, &svc_path]);
    let _ = fs::remove_file(tmp_svc);
    if !r.success {
        let _ = fs::remove_file(tmp_tmr);
        return Err(format!("Service installieren: {}", r.stderr));
    }

    let r = run_privileged("cp", &[tmp_tmr, &tmr_path]);
    let _ = fs::remove_file(tmp_tmr);
    if !r.success {
        return Err(format!("Timer installieren: {}", r.stderr));
    }

    let r = run_privileged("systemctl", &["daemon-reload"]);
    if !r.success {
        return Err(format!("daemon-reload: {}", r.stderr));
    }

    let r = run_privileged("systemctl", &["enable", "--now", &c.sync.timer_unit]);
    if !r.success {
        return Err(format!("Timer aktivieren: {}", r.stderr));
    }

    Ok(CommandResult {
        success: true,
        stdout: format!(
            "Timer {} installiert und aktiviert.\nIntervall: {}, Verzögerung: {}\nBinary: {}\nConfig: {}",
            c.sync.timer_unit, calendar, delay, exe, config_path
        ),
        stderr: String::new(),
        exit_code: 0,
    })
}

#[tauri::command]
pub fn uninstall_timer() -> Result<CommandResult, String> {
    let c = cfg();

    // Stop and disable
    let _ = run_privileged("systemctl", &["disable", "--now", &c.sync.timer_unit]);
    let _ = run_privileged("systemctl", &["stop", &c.sync.service_unit]);

    // Remove unit files
    let svc_path = format!("/etc/systemd/system/{}", c.sync.service_unit);
    let tmr_path = format!("/etc/systemd/system/{}", c.sync.timer_unit);
    let _ = run_privileged("rm", &["-f", &svc_path]);
    let _ = run_privileged("rm", &["-f", &tmr_path]);

    let _ = run_privileged("systemctl", &["daemon-reload"]);

    Ok(CommandResult {
        success: true,
        stdout: format!("Timer {} deinstalliert", c.sync.timer_unit),
        stderr: String::new(),
        exit_code: 0,
    })
}

// ─── Headless CLI Sync ────────────────────────────────────────

pub fn run_sync_headless(config_path_override: Option<String>) -> i32 {
    let c = if let Some(path) = config_path_override {
        match config::load_config_from(std::path::Path::new(&path)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("backsnap: Config-Fehler: {}", e);
                return 1;
            }
        }
    } else {
        cfg()
    };

    // Ensure log directory exists
    if let Some(parent) = std::path::Path::new(&c.sync.log_path).parent() {
        let _ = fs::create_dir_all(parent);
    }

    let start_time = Instant::now();
    rotate_log(&c.sync.log_path, c.sync.log_max_lines);

    println!("backsnap: Preflight-Check...");

    if !cmd_exists("rsync") {
        eprintln!("backsnap: rsync nicht installiert");
        return 1;
    }

    let ctx = match detect_sync_direction(&c) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("backsnap: {}", e);
            sync_log(&c.sync.log_path, &format!("CLI FEHLER: {}", e));
            return 1;
        }
    };

    sync_log(
        &c.sync.log_path,
        &format!("=== Sync Start (CLI): {} ===", ctx.direction),
    );
    println!("backsnap: Richtung: {}", ctx.direction);

    let total_phases = c.sync.subvolumes.len() + if c.boot.sync_enabled { 1 } else { 0 };

    // ── Sync subvolumes ──
    for (i, sv) in c.sync.subvolumes.iter().enumerate() {
        let phase = i + 1;
        let mnt = format!("{}-{}", ctx.mount_base, sv.name);
        println!(
            "backsnap: [{}/{}] {} ({}) synchronisieren...",
            phase, total_phases, sv.name, sv.source
        );
        sync_log(
            &c.sync.log_path,
            &format!("Phase {}/{}: Sync {} ...", phase, total_phases, sv.source),
        );

        if let Err(e) = mount_subvol(&ctx.backup_dev, &mnt, &sv.subvol, &c.sync.mount_options) {
            eprintln!("backsnap: Mount-Fehler {}: {}", sv.subvol, e);
            sync_log(
                &c.sync.log_path,
                &format!("FEHLER mount {}: {}", sv.subvol, e),
            );
            return 1;
        }

        let excludes: Vec<String> = if sv.source == "/" {
            c.sync.system_excludes.clone()
        } else if sv.source == "/home/" || sv.source == "/home" {
            let mut exc = c.sync.home_excludes.clone();
            if c.sync.extra_excludes_on_primary && ctx.is_primary_boot {
                exc.extend(c.sync.home_extra_excludes.clone());
            }
            exc
        } else {
            Vec::new()
        };

        let src = if sv.source.ends_with('/') {
            sv.source.clone()
        } else {
            format!("{}/", sv.source)
        };

        match run_rsync(&src, &format!("{}/", mnt), &excludes, sv.delete) {
            Ok(r) => {
                let stats: Vec<&str> = r
                    .stdout
                    .lines()
                    .filter(|l| l.contains("bytes") || l.contains("transferred"))
                    .collect();
                println!("backsnap: {} OK. {}", sv.name, stats.join(" | "));
                sync_log(
                    &c.sync.log_path,
                    &format!("{} OK. {}", sv.name, stats.join(" | ")),
                );
            }
            Err(e) => {
                safe_umount(&mnt);
                eprintln!("backsnap: Sync-Fehler {}: {}", sv.name, e);
                sync_log(
                    &c.sync.log_path,
                    &format!("FEHLER {}-Sync: {}", sv.name, e),
                );
                return 1;
            }
        }
        safe_umount(&mnt);
    }

    // ── Boot sync ──
    if c.boot.sync_enabled {
        println!(
            "backsnap: [{}/{}] Boot synchronisieren...",
            total_phases, total_phases
        );
        sync_log(
            &c.sync.log_path,
            &format!("Phase {}/{}: Sync /boot ...", total_phases, total_phases),
        );

        let backup_efi = derive_efi_partition(&ctx.backup_dev);
        let boot_mnt = "/tmp/backsnap-boot";
        let _ = run_privileged("mkdir", &["-p", boot_mnt]);

        let mount_res = run_privileged("mount", &[&backup_efi, boot_mnt]);
        if mount_res.success {
            match run_rsync("/boot/", &format!("{}/", boot_mnt), &c.boot.excludes, false) {
                Ok(_) => {
                    println!("backsnap: Boot OK.");
                    sync_log(&c.sync.log_path, "Boot OK.");
                }
                Err(e) => {
                    println!("backsnap: WARNUNG Boot-Sync: {}", e);
                    sync_log(
                        &c.sync.log_path,
                        &format!("WARNUNG Boot-Sync: {}", e),
                    );
                }
            }
        } else {
            println!(
                "backsnap: WARNUNG: Konnte EFI {} nicht mounten",
                backup_efi
            );
            sync_log(
                &c.sync.log_path,
                &format!(
                    "WARNUNG: Konnte Backup-EFI {} nicht mounten: {}",
                    backup_efi, mount_res.stderr
                ),
            );
        }
        safe_umount(boot_mnt);
    }

    let elapsed = start_time.elapsed().as_secs();
    let duration_str = format_duration(elapsed);
    println!(
        "backsnap: Sync fertig: {} (Dauer: {})",
        ctx.direction, duration_str
    );
    sync_log(
        &c.sync.log_path,
        &format!(
            "=== Sync fertig (CLI): {} (Dauer: {}) ===",
            ctx.direction, duration_str
        ),
    );

    0
}
