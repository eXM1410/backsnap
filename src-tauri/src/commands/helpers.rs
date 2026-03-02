//! Shared helpers for command modules: command execution, validation, logging, caches.

use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ─── Statics ──────────────────────────────────────────────────

pub static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);

// ─── Input Validation Limits ──────────────────────────────────

/// Maximum length for a snapper config name.
const MAX_CONFIG_NAME_LEN: usize = 64;
/// Maximum length for a snapshot description.
const MAX_DESCRIPTION_LEN: usize = 256;
/// Maximum length for a device path.
const MAX_DEVICE_PATH_LEN: usize = 128;
/// Maximum number of exclude rules allowed.
const MAX_EXCLUDE_ENTRIES: usize = 10_000;
/// Maximum length for a single exclude rule.
const MAX_EXCLUDE_RULE_LEN: usize = 512;
/// Polling interval in ms when waiting for the EFI lock.
const EFI_LOCK_POLL_MS: u64 = 50;

/// Generic TTL cache: stores a value + timestamp, thread-safe via Mutex.
pub struct TtlCache<T> {
    inner: Mutex<Option<(T, Instant)>>,
}

impl<T: Clone> TtlCache<T> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Return the cached value if it's younger than `ttl`, otherwise `None`.
    pub fn get(&self, ttl: Duration) -> Option<T> {
        let guard = self.inner.lock().ok()?;
        guard
            .as_ref()
            .filter(|(_, ts)| ts.elapsed() < ttl)
            .map(|(v, _)| v.clone())
    }

    /// Store a new value with the current timestamp.
    pub fn set(&self, value: T) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some((value, Instant::now()));
        }
    }

    /// Clear the cached value.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}

pub static BOOT_INFO_CACHE: TtlCache<super::boot::BootInfo> = TtlCache::new();
pub static BOOT_VALIDATION_CACHE: TtlCache<super::boot::BootValidation> = TtlCache::new();
pub static BTRFS_USAGE_CACHE: TtlCache<String> = TtlCache::new();

// ─── EFI Mount Lock (cross-process) ──────────────────────────

// Use a single global path that works across privilege boundaries.
// If root uses /run but the GUI user falls back to /tmp, serialization breaks.
pub const EFI_LOCK_PATH: &str = "/tmp/backsnap-efi.lock";

fn open_lockfile_for_flock(path: &str) -> Result<fs::File, String> {
    use std::os::unix::fs::PermissionsExt;

    // Prefer RW so we can optionally write debug info (pid). If that fails due
    // to permissions, fall back to read-only — flock works fine on an RO fd.
    match fs::OpenOptions::new().read(true).write(true).open(path) {
        Ok(f) => {
            // If we're root, ensure the file stays readable for non-root callers.
            if is_root() {
                if let Ok(meta) = f.metadata() {
                    let mode = meta.permissions().mode();
                    if (mode & 0o444) != 0o444 {
                        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o644));
                    }
                }
            }
            Ok(f)
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|e| format!("Lockfile {} öffnen fehlgeschlagen: {}", path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Create with RW so we can set permissions. Keep it world-readable
            // so non-root can also open it later.
            let f = fs::OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(path)
                .map_err(|e| format!("Lockfile {} erstellen fehlgeschlagen: {}", path, e))?;

            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o644));
            Ok(f)
        }
        Err(e) => Err(format!("Lockfile {} öffnen fehlgeschlagen: {}", path, e)),
    }
}

/// Cross-process file lock to serialize EFI mount/unmount operations.
///
/// This prevents races between:
/// - GUI dashboard boot-info (non-root)
/// - GUI sync (pkexec)
/// - headless/systemd sync (root)
pub struct EfiMountLock {
    _file: fs::File,
}

impl EfiMountLock {
    pub fn acquire() -> Result<Self, String> {
        use std::os::unix::io::AsRawFd;

        let file = open_lockfile_for_flock(EFI_LOCK_PATH)?;

        // Blocking exclusive lock: we prefer correctness over speed.
        let fd = file.as_raw_fd();
        // SAFETY: flock() on a valid fd — no memory risks.
        #[allow(unsafe_code)]
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!("EFI flock fehlgeschlagen: {}", err));
        }
        Ok(EfiMountLock { _file: file })
    }

    /// Acquire the EFI lock, but give up after `timeout`.
    ///
    /// Used for UI/status paths (boot dashboard) to avoid hanging when a
    /// privileged sync process holds the lock.
    pub fn acquire_timeout(timeout: Duration) -> Result<Self, String> {
        use std::os::unix::io::AsRawFd;

        let file = open_lockfile_for_flock(EFI_LOCK_PATH)?;

        let fd = file.as_raw_fd();
        let start = std::time::Instant::now();
        loop {
            // SAFETY: flock() on a valid fd — no memory risks.
            #[allow(unsafe_code)]
            let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            if ret == 0 {
                return Ok(EfiMountLock { _file: file });
            }

            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::WouldBlock {
                return Err(format!("EFI flock fehlgeschlagen: {}", err));
            }
            if start.elapsed() >= timeout {
                return Err("EFI busy (Lock gehalten)".to_string());
            }

            std::thread::sleep(Duration::from_millis(EFI_LOCK_POLL_MS));
        }
    }
}

// ─── Centralized Paths ────────────────────────────────────────

// Same reasoning as EFI_LOCK_PATH: this must be identical across root/non-root.
pub const LOCK_PATH: &str = "/tmp/backsnap-sync.lock";
pub const ROLLBACK_TMPDIR: &str = "/tmp/backsnap-rollback";
pub const BOOT_MOUNT: &str = "/tmp/backsnap-boot";

// ─── Home Mountpoint ──────────────────────────────────────────

/// Returns the base mountpoint for user home directories (e.g. "/home/" or "/var/home/").
pub fn get_home_mountpoint() -> String {
    const MOUNTS: &[(&str, &str)] = &[("/var/home", "/var/home/"), ("/Users", "/Users/")];
    if let Some(home) = dirs::home_dir() {
        for &(prefix, mount) in MOUNTS {
            if home.starts_with(prefix) { return mount.to_string(); }
        }
    }
    "/home/".to_string()
}

// ─── Sync Lock ────────────────────────────────────────────────

/// Cross-process file lock for sync operations.
pub struct SyncLock {
    _file: fs::File,
}

impl SyncLock {
    pub fn acquire() -> Result<Self, String> {
        use std::os::unix::io::AsRawFd;

        let file = open_lockfile_for_flock(LOCK_PATH)?;

        let fd = file.as_raw_fd();
        // SAFETY: flock() on a valid fd — no memory risks.
        #[allow(unsafe_code)]
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            return if err.kind() == std::io::ErrorKind::WouldBlock {
                Err("Sync läuft bereits (anderer Prozess hält Lock). \
                     Warte bis der laufende Sync beendet ist."
                    .to_string())
            } else {
                Err(format!("Lockfile flock fehlgeschlagen: {}", err))
            };
        }

        if let Ok(mut f) = file.try_clone() {
            let _ = f.write_all(format!("{}\n", std::process::id()).as_bytes());
        }

        Ok(SyncLock { _file: file })
    }
}

// ─── Cache Invalidation ──────────────────────────────────────

pub fn invalidate_caches() {
    BOOT_INFO_CACHE.clear();
    BOOT_VALIDATION_CACHE.clear();
    BTRFS_USAGE_CACHE.clear();
    crate::config::invalidate_config_cache();
}

// ─── Data Types (shared across commands) ─────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub model: String,
    pub role: String,
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
    pub snap_type: SnapType,
    pub pre_id: Option<u32>,
    pub date: String,
    pub user: String,
    pub cleanup: String,
    pub description: String,
}

/// Snapper snapshot type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapType {
    Single,
    Pre,
    Post,
}

impl SnapType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "single" => Some(Self::Single),
            "pre" => Some(Self::Pre),
            "post" => Some(Self::Post),
            _ => None,
        }
    }
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
    pub backup_disk: String,
    pub boot_uuid: String,
    pub disks: Vec<DiskInfo>,
    pub snapper_configs: Vec<String>,
    pub snapshot_counts: Vec<SnapshotCount>,
    pub sync_status: SyncStatus,
    pub boot_info: Option<super::boot::BootInfo>,
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
pub struct SubvolumeInfo {
    pub id: String,
    pub gen: String,
    pub top_level: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BackupCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BackupVerifyResult {
    pub backup_dev: String,
    pub overall_ok: bool,
    pub checks: Vec<BackupCheck>,
}

// ─── Command Execution ───────────────────────────────────────

pub fn run_cmd(cmd: &str, args: &[&str]) -> CommandResult {
    match Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(output) => CommandResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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

pub fn run_privileged(cmd: &str, args: &[&str]) -> CommandResult {
    if is_root() {
        run_cmd(cmd, args)
    } else {
        let mut full_args = vec![cmd];
        full_args.extend_from_slice(args);
        run_cmd("pkexec", &full_args)
    }
}

// ─── Native File Ops (batch via --file-ops CLI) ──────────────

/// A single privileged file operation, serialized as JSON for `--file-ops` CLI mode.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum FileOp {
    #[serde(rename = "write")]
    Write { path: String, content: String },
    #[serde(rename = "copy")]
    Copy { src: String, dst: String },
    #[serde(rename = "delete")]
    Delete { path: String },
    #[serde(rename = "mkdir")]
    Mkdir { path: String },
    #[serde(rename = "chmod")]
    Chmod { path: String, mode: u32 },
}

/// Execute a batch of privileged file operations with a single pkexec call.
/// When already root, executes directly. Otherwise spawns `pkexec backsnap --file-ops <json>`.
pub fn run_file_ops_batch(ops: &[FileOp]) -> Result<(), String> {
    if ops.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_string(ops).map_err(|e| format!("JSON-Serialisierung: {}", e))?;
    if is_root() {
        let ret = crate::run_file_ops(&json);
        if ret == 0 {
            Ok(())
        } else {
            Err("file-ops fehlgeschlagen".to_string())
        }
    } else {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Binary-Pfad: {}", e))?
            .to_string_lossy()
            .to_string();
        let r = run_cmd("pkexec", &[&exe, "--file-ops", &json]);
        if r.success {
            Ok(())
        } else {
            Err(r.stderr.trim().to_string())
        }
    }
}

pub fn is_root() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    // SAFETY: geteuid() is a read-only syscall with no arguments — always safe.
    #[allow(unsafe_code)]
    *CACHED.get_or_init(|| unsafe { libc::geteuid() == 0 })
}

pub fn read_sys(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Like [`read_sys`] but returns `None` for missing / empty files.
pub fn read_sys_opt(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn get_boot_uuid() -> String {
    crate::sysfs::mount_uuid("/").unwrap_or_default()
}

pub fn cfg() -> AppConfig {
    crate::config::load_config().unwrap_or_else(|_| crate::config::auto_detect_config())
}

// ─── Boot Conf Listing ───────────────────────────────────────

/// List `.conf` filenames in a directory (e.g. `"linux.conf"`).
///
/// Tries `fs::read_dir` first, falls back to `run_privileged("ls")` for
/// root-owned directories like `/boot/loader/entries`.
pub fn list_conf_files(dir: &str) -> Vec<String> {
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut files: Vec<String> = rd
            .flatten()
            .filter_map(|e| {
                let is_conf = e.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("conf"));
                is_conf.then(|| e.file_name().to_string_lossy().into_owned())
            })
            .collect();
        if !files.is_empty() {
            files.sort();
            return files;
        }
    }

    let r = run_privileged("ls", &["-1", dir]);
    if r.success {
        let mut files: Vec<String> = r
            .stdout
            .lines()
            .map(str::trim)
            .filter(|l| std::path::Path::new(l).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("conf")))
            .map(String::from)
            .collect();
        files.sort();
        return files;
    }

    Vec::new()
}

// ─── Validation ──────────────────────────────────────────────

pub fn validate_config(config: &str) -> Result<(), String> {
    if config.is_empty() || config.len() > MAX_CONFIG_NAME_LEN {
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

pub fn validate_description(desc: &str) -> Result<(), String> {
    if desc.len() > MAX_DESCRIPTION_LEN {
        return Err("Beschreibung zu lang (max 256 Zeichen)".to_string());
    }
    if desc.chars().any(|c| SHELL_DANGEROUS.contains(&c)) {
        return Err("Beschreibung enthält ungültige Zeichen".to_string());
    }
    Ok(())
}

pub const SHELL_DANGEROUS: &[char] = &[
    '`', '$', '\\', '|', ';', '&', '<', '>', '\'', '"', '\n', '\r', '\0', '(', ')',
];

pub fn validate_safe_path(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{}: darf nicht leer sein", label));
    }
    if value.chars().any(|c| SHELL_DANGEROUS.contains(&c)) {
        return Err(format!(
            "{}: enthält ungültige Zeichen (Shell-Injection verhindert): '{}'",
            label, value
        ));
    }
    Ok(())
}

pub fn validate_device_path(dev: &str) -> bool {
    !dev.is_empty()
        && dev.starts_with("/dev/")
        && dev.len() < MAX_DEVICE_PATH_LEN
        && !dev.chars().any(|c| SHELL_DANGEROUS.contains(&c))
}

pub fn validate_sync_config(c: &AppConfig) -> Result<(), String> {
    validate_safe_path(&c.sync.mount_base, "mount_base")?;
    validate_safe_path(&c.sync.mount_options, "mount_options")?;
    for sv in &c.sync.subvolumes {
        validate_safe_path(&sv.subvol, &format!("subvol '{}'", sv.name))?;
        validate_safe_path(&sv.source, &format!("source '{}'", sv.name))?;
        validate_safe_path(&sv.name, &format!("name '{}'", sv.name))?;
    }
    for (uuid, label) in [
        (&c.disks.primary_uuid, "primary_uuid"),
        (&c.disks.backup_uuid, "backup_uuid"),
    ] {
        if !uuid.is_empty() && !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(format!("{}: ungültiges UUID-Format '{}'", label, uuid));
        }
    }
    if !c.disks.primary_uuid.is_empty()
        && !c.disks.backup_uuid.is_empty()
        && c.disks.primary_uuid == c.disks.backup_uuid
    {
        return Err("Primary und Backup UUID sind identisch! \
             Bitte unter Einstellungen zwei verschiedene Disks wählen."
            .to_string());
    }

    validate_exclude_list(&c.sync.system_excludes, "system_excludes")?;
    validate_exclude_list(&c.sync.home_excludes, "home_excludes")?;
    validate_exclude_list(&c.sync.home_extra_excludes, "home_extra_excludes")?;
    validate_exclude_list(&c.boot.excludes, "boot.excludes")?;

    Ok(())
}

fn validate_exclude_list(excludes: &[String], label: &str) -> Result<(), String> {
    if excludes.len() > MAX_EXCLUDE_ENTRIES {
        return Err(format!(
            "{}: zu viele Einträge ({}) — max 10000",
            label,
            excludes.len()
        ));
    }

    for (idx, raw) in excludes.iter().enumerate() {
        let rule = raw.trim();
        if rule.is_empty() || rule.starts_with('#') {
            continue;
        }
        if rule.len() > MAX_EXCLUDE_RULE_LEN {
            return Err(format!(
                "{}[{}]: zu lang ({} Zeichen, max 512)",
                label,
                idx,
                rule.len()
            ));
        }
        if rule.chars().any(|c| SHELL_DANGEROUS.contains(&c)) {
            return Err(format!(
                "{}[{}]: enthält ungültige Zeichen: '{}'",
                label, idx, rule
            ));
        }
    }

    Ok(())
}

/// Normalize excludes before passing them to rsync.
///
/// Rules:
/// - trim whitespace
/// - ignore empty entries and comment entries (`# ...`)
/// - preserve order
/// - deduplicate exact duplicates
pub fn sanitize_excludes(excludes: &[String]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    for raw in excludes {
        let cleaned = raw.trim();
        if cleaned.is_empty() || cleaned.starts_with('#') {
            continue;
        }

        let candidate = cleaned.to_string();
        if seen.insert(candidate.clone()) {
            out.push(candidate);
        }
    }

    out
}

// ─── Logging ─────────────────────────────────────────────────

pub fn activity_log_path() -> PathBuf {
    dirs::data_local_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("backsnap")
        .join("activity.log")
}

pub fn read_activity_log_lines(max_lines: usize) -> Vec<String> {
    let path = activity_log_path();
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = content.lines().map(std::string::ToString::to_string).collect();
    if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }
    lines
}

fn activity_log_line(source: &str, msg: &str) -> String {
    let timestamp = chrono::Local::now().format("%F %T").to_string();
    format!("[{}] [{}] {}", timestamp, source, msg)
}

pub fn log_activity(source: &str, msg: &str) {
    let path = activity_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let line = activity_log_line(source, msg);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(format!("{}\n", line).as_bytes()));
}

pub fn log_activity_with_app(app: &tauri::AppHandle, source: &str, msg: &str) {
    use tauri::Emitter;

    log_activity(source, msg);
    let line = activity_log_line(source, msg);
    let _ = app.emit("activity-log-line", line);
}

pub fn sync_log(log_path: &str, msg: &str) {
    let timestamp = chrono::Local::now().format("%F %T").to_string();
    let line = format!("[{}] {}\n", timestamp, msg);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
    log_activity("sync", msg);
}

pub fn rotate_log(log_path: &str, max_lines: usize) {
    let Ok(content) = fs::read_to_string(log_path) else { return };
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > max_lines {
        let keep = &lines[lines.len() - max_lines..];
        let new_content = keep.join("\n") + "\n";
        let _ = fs::write(log_path, new_content);
    }
}

// ─── Progress reporting ──────────────────────────────────────

/// Emit a sync progress event as a JSON line on stdout.
/// Used by the elevated subprocess; the GUI reads these and relays as Tauri events.
#[allow(clippy::print_stdout)]
pub fn emit_sync_progress(step: &str, detail: &str, pct: u8) {
    let json = serde_json::json!({
        "type": "progress",
        "step": step,
        "detail": detail,
        "percent": pct,
    });
    println!("{}", json);
}

/// Emit rsync byte-level progress as a JSON line on stdout.
#[allow(clippy::print_stdout)]
pub fn emit_sync_bytes(phase: &str, bytes: u64, pct: u8, speed: &str) {
    let json = serde_json::json!({
        "type": "bytes",
        "phase": phase,
        "bytes": bytes,
        "pct": pct,
        "speed": speed,
    });
    println!("{}", json);
}

// ─── Elevated Subprocess Helpers ─────────────────────────────

/// Pre-load a config from an override path into the config cache.
/// Used by CLI entry points that run as root via pkexec.
pub fn preload_cli_config(path: Option<&str>) -> Result<(), String> {
    if let Some(p) = path {
        let c = crate::config::load_config_from(std::path::Path::new(p))
            .map_err(|e| format!("Config-Fehler: {}", e))?;
        if let Some(parent) = std::path::Path::new(&c.sync.log_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        crate::config::set_config_cache(c);
    }
    Ok(())
}

/// Spawn `pkexec backsnap <args> --config <user-config>` and relay JSON progress
/// lines as Tauri events.  Used by sync and rollback elevated wrappers.
pub fn relay_elevated_subprocess(
    app: &tauri::AppHandle, extra_args: &[&str],
) -> Result<CommandResult, String> {
    use tauri::Emitter;

    let exe = std::env::current_exe()
        .map_err(|e| format!("current_exe: {}", e))?
        .to_string_lossy().into_owned();
    let user_config = crate::config::config_path();
    let config_str = user_config.to_string_lossy().into_owned();

    let mut cli: Vec<String> = vec![exe];
    cli.extend(extra_args.iter().map(|s| (*s).to_string()));
    cli.extend(["--config".to_string(), config_str]);

    let mut child = Command::new("pkexec")
        .args(&cli)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("pkexec spawn: {}", e))?;

    let stdout = child.stdout.take().ok_or("elevated: no stdout")?;
    let stderr_pipe = child.stderr.take().ok_or("elevated: no stderr")?;
    let app_clone = app.clone();

    let relay_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut final_result: Option<CommandResult> = None;
        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed) else { continue };
            match obj.get("type").and_then(|t| t.as_str()) {
                Some("progress") => {
                    let _ = app_clone.emit("sync-progress", serde_json::json!({
                        "step": obj["step"].as_str().unwrap_or_default(),
                        "detail": obj["detail"].as_str().unwrap_or_default(),
                        "percent": obj["percent"].as_u64().unwrap_or_default(),
                    }));
                }
                Some("bytes") => {
                    let _ = app_clone.emit("rsync-bytes-progress", serde_json::json!({
                        "phase": obj["phase"].as_str().unwrap_or_default(),
                        "bytes": obj["bytes"].as_u64().unwrap_or_default(),
                        "pct": obj["pct"].as_u64().unwrap_or_default(),
                        "speed": obj["speed"].as_str().unwrap_or_default(),
                    }));
                }
                Some("result") => {
                    final_result = Some(CommandResult {
                        success: obj["success"].as_bool().unwrap_or_default(),
                        stdout: obj["stdout"].as_str().unwrap_or_default().to_string(),
                        stderr: obj["stderr"].as_str().unwrap_or_default().to_string(),
                        exit_code: i32::try_from(obj["exit_code"].as_i64().unwrap_or(-1)).unwrap_or(-1),
                    });
                }
                _ => {}
            }
        }
        final_result
    });

    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        BufReader::new(stderr_pipe).read_to_string(&mut buf).ok();
        buf
    });

    let status = child.wait().map_err(|e| format!("elevated wait: {}", e))?;
    let final_result = relay_thread.join().unwrap_or(None);
    let stderr_str = stderr_thread.join().unwrap_or_default();

    match final_result {
        Some(r) => Ok(r),
        None if status.success() => Ok(CommandResult {
            success: true, stdout: "Abgeschlossen".to_string(), stderr: stderr_str, exit_code: 0,
        }),
        None => Err(format!("elevated exit={}: {}",
            status.code().unwrap_or(-1),
            stderr_str.lines().take(10).collect::<Vec<_>>().join("\n")
        )),
    }
}

/// Emit a CLI result as JSON to stdout — shared by elevated subprocess entry points.
#[allow(clippy::print_stdout)]
pub fn emit_cli_result(result: Result<CommandResult, String>, error_label: &str) -> i32 {
    match result {
        Ok(r) => {
            println!("{}", serde_json::json!({
                "type": "result", "success": r.success,
                "stdout": r.stdout, "stderr": r.stderr, "exit_code": r.exit_code,
            }));
            0
        }
        Err(e) => {
            let c = cfg();
            sync_log(&c.sync.log_path, &format!("{}: {}", error_label, e));
            println!("{}", serde_json::json!({
                "type": "result", "success": false,
                "stdout": "", "stderr": e, "exit_code": 1,
            }));
            1
        }
    }
}

// ─── Formatting Helpers ──────────────────────────────────────

/// Format raw byte count string into human-readable (e.g. "1.5G", "100.0M").
fn format_bytes_human(raw: &str) -> String {
    let bytes: f64 = match raw.parse() {
        Ok(v) => v,
        Err(_) => return "0B".to_string(),
    };
    const UNITS: &[(f64, &str)] = &[
        (1_099_511_627_776.0, "T"), (1_073_741_824.0, "G"), (1_048_576.0, "M"),
    ];
    for &(threshold, suffix) in UNITS {
        if bytes >= threshold { return format!("{:.1}{}", bytes / threshold, suffix); }
    }
    format!("{bytes}B")
}

/// Like `format_bytes_human` but returns "—" unchanged and "0" as "0B".
fn format_disk_bytes(raw: &str) -> String {
    match raw {
        "—" => "—".to_string(),
        "0" => "0B".to_string(),
        other => format_bytes_human(other),
    }
}

pub fn format_duration(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

pub fn cmd_exists(cmd: &str) -> bool {
    run_cmd("which", &[cmd]).success
}

pub fn disk_label(uuid: &str, c: &AppConfig) -> String {
    use super::boot::DiskSide;
    let side = DiskSide::from_uuid(uuid, c);
    match side {
        DiskSide::Unknown => format!("Unknown ({})", uuid),
        _ => side.label(c).to_string(),
    }
}

// ─── Disk Info ───────────────────────────────────────────────

/// Temporarily mount an unmounted partition read-only to get usage info.
/// Uses sudo with NOPASSWD rules for the specific mount point.
/// Returns (used_bytes, avail_bytes, use_percent) as strings, or None on failure.
fn probe_unmounted_usage(dev: &str) -> Option<(String, String, String)> {
    let mnt = "/tmp/backsnap-efi-check";
    let _ = fs::create_dir_all(mnt);
    let mount_res = run_cmd("sudo", &["mount", "-o", "ro", dev, mnt]);
    if !mount_res.success {
        return None;
    }
    let df_res = run_cmd("df", &["-B1", "--output=used,avail,pcent", mnt]);
    let _ = run_cmd("sudo", &["umount", mnt]);
    if !df_res.success {
        return None;
    }
    // Parse second line of df output (first is header)
    let line = df_res.stdout.lines().nth(1)?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 3 {
        Some((
            parts[0].to_string(),
            parts[1].to_string(),
            parts[2].to_string(),
        ))
    } else {
        None
    }
}

const RELEVANT_FSTYPES: &[&str] = &["btrfs", "vfat", "ext4", "xfs"];

/// Extract a trimmed string from a JSON object, defaulting to `""`.
fn json_str(obj: &serde_json::Value, key: &str) -> String {
    obj.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Shortest non-null mountpoint from the lsblk-style JSON array.
fn shortest_mountpoint(val: &serde_json::Value) -> String {
    val.get("mountpoints")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .min_by_key(|s| s.len())
        .map_or("Nicht gemountet".to_string(), str::to_string)
}

/// Classify a partition by its role in the Backsnap setup.
fn partition_role(uuid: &str, mountpoint: &str, c: &AppConfig) -> &'static str {
    use super::boot::DiskSide;
    match DiskSide::from_uuid(uuid, c) {
        DiskSide::Primary => "System Disk",
        DiskSide::Backup => "Backup Disk",
        DiskSide::Unknown => match mountpoint {
            "/boot" | "/boot/efi" => "Boot Partition",
            _ => "Datenlaufwerk",
        },
    }
}

/// Build a map of device-name → (used, avail, percent) from `df` output.
fn build_df_map() -> std::collections::HashMap<String, (String, String, String)> {
    let df = run_cmd(
        "df",
        &["-B1", "--output=source,used,avail,pcent",
          "-t", "btrfs", "-t", "vfat", "-t", "ext4", "-t", "xfs"],
    );
    df.stdout
        .lines()
        .skip(1)
        .filter_map(|line| {
            let p: Vec<&str> = line.split_whitespace().collect();
            if p.len() >= 4 {
                let name = p[0].trim_start_matches("/dev/").to_string();
                Some((name, (p[1].to_string(), p[2].to_string(), p[3].to_string())))
            } else {
                None
            }
        })
        .collect()
}

pub fn get_disk_info() -> Vec<DiskInfo> {
    let c = cfg();
    let df_map = build_df_map();
    let lsblk = run_cmd(
        "lsblk",
        &["-b", "-J", "-o", "NAME,MODEL,PKNAME,UUID,MOUNTPOINTS,FSTYPE,SIZE"],
    );
    let mut disks: Vec<DiskInfo> = Vec::new();

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&lsblk.stdout) else {
        return disks;
    };
    let Some(devices) = parsed.get("blockdevices").and_then(|v| v.as_array()) else {
        return disks;
    };

    for dev in devices {
        let model = dev.get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("Unbekanntes Laufwerk")
            .trim()
            .to_string();

        let Some(children) = dev.get("children").and_then(|v| v.as_array()) else {
            continue;
        };

        for child in children {
            let name = json_str(child, "name");
            let fstype = json_str(child, "fstype");
            let uuid = json_str(child, "uuid");

            if !RELEVANT_FSTYPES.contains(&fstype.as_str()) {
                continue;
            }

            let mountpoint = shortest_mountpoint(child);
            let size_bytes = child.get("size").and_then(serde_json::Value::as_u64).unwrap_or_default();
            let is_mounted = mountpoint != "Nicht gemountet";

            let (used_bytes, avail_bytes, use_percent) = if is_mounted {
                df_map.get(&name).cloned().unwrap_or(("0".into(), "0".into(), "0%".into()))
            } else {
                probe_unmounted_usage(&format!("/dev/{}", name))
                    .unwrap_or(("—".into(), "—".into(), "—".into()))
            };

            disks.push(DiskInfo {
                name: format!("/dev/{}", name),
                model: model.clone(),
                role: partition_role(&uuid, &mountpoint, &c).to_string(),
                fstype,
                size: format_bytes_human(&size_bytes.to_string()),
                used: format_disk_bytes(&used_bytes),
                avail: format_disk_bytes(&avail_bytes),
                use_percent,
                mountpoint,
                uuid,
            });
        }
    }

    disks
}

pub fn get_snapper_configs() -> Vec<String> {
    let result = run_cmd("snapper", &["list-configs", "--columns", "config"]);
    result
        .stdout
        .lines()
        .skip(2)
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

pub fn get_snapshot_count(config: &str) -> u32 {
    let result = run_cmd("snapper", &["-c", config, "list", "--columns", "number"]);
    let count = result
        .stdout
        .lines()
        // No header skip: parse filter already rejects non-numeric lines (headers,
        // warnings, empty lines) and the snapshot-0 pre-snapshot baseline.
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && t != "0" && t.parse::<u32>().is_ok()
        })
        .count();
    // CAST-SAFETY: snapshot count is always far below u32::MAX (practical max ~1000)
    #[allow(clippy::cast_possible_truncation)]
    let count = count as u32;
    count
}
