//! Boot info & validation commands: EFI partition checks, entry parsing, kernel verification.
//!
//! Native Rust — no shell scripts.  Uses `AutoUmount` for RAII cleanup,
//! `crate::sysfs` for mount detection, and `fs::read_dir` / `fs::read_to_string`
//! for entry parsing.  Falls back to privileged reads when `/boot` is root-only.
//!
//! Entry classification is simple: entries from the Primary EFI → "Primary",
//! entries from the Backup EFI → "Backup".  No UUID-guessing.

use super::efi::{derive_efi_partition, detect_efi_arch, efi_arch_suffix};
use super::helpers::*;
use super::mount::AutoUmount;
use crate::config::{AppConfig, BootloaderType};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ─── Enums ────────────────────────────────────────────────────

impl BootloaderType {
    /// Read boot entries from a base path (mounted EFI or `/boot`).
    fn read_entries(self, base: &str, side: DiskSide) -> Vec<BootEntryInfo> {
        match self {
            Self::Grub => {
                let cfg = find_grub_cfg(base)
                    .map(|p| read_file_privileged(&p))
                    .unwrap_or_default();
                parse_grub_entries(&cfg, side)
            }
            Self::SystemdBoot => {
                read_loader_entries(&format!("{}/loader/entries", base), side)
            }
        }
    }

    /// Extract bootloader version string from a mounted EFI partition.
    fn extract_version_from_efi(self, mnt: &str, suffix: &str) -> Option<String> {
        let (path, prefix) = match self {
            Self::Grub => (format!("{}/EFI/grub/grub{}.efi", mnt, suffix), "GRUB "),
            Self::SystemdBoot => (
                format!("{}/EFI/systemd/systemd-boot{}.efi", mnt, suffix),
                "systemd-boot ",
            ),
        };
        extract_bl_version(&path, prefix)
    }
}

/// Which physical disk an entry or boot source belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiskSide {
    Primary,
    Backup,
    Unknown,
}

impl DiskSide {
    /// Resolve a UUID to Primary / Backup / Unknown.
    pub fn from_uuid(uuid: &str, c: &AppConfig) -> Self {
        if !uuid.is_empty() && uuid == c.disks.primary_uuid {
            Self::Primary
        } else if !uuid.is_empty() && uuid == c.disks.backup_uuid {
            Self::Backup
        } else {
            Self::Unknown
        }
    }

    /// Return the *other* side's UUID, or the primary if Unknown.
    pub fn other_uuid(self, c: &AppConfig) -> &str {
        match self {
            Self::Primary => &c.disks.backup_uuid,
            _ => &c.disks.primary_uuid,
        }
    }

    /// Human label for this side.
    pub fn label(self, c: &AppConfig) -> &str {
        match self {
            Self::Primary => &c.disks.primary_label,
            Self::Backup => &c.disks.backup_label,
            Self::Unknown => "Unknown",
        }
    }
}

/// Parsed fields from a boot loader entry file.
struct ParsedBootEntry {
    title: String,
    root_uuid: String,
    kernel: String,
    sort_key: String,
}

// ─── Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootValidation {
    pub backup_efi_accessible: bool,
    pub bootloader_present: bool,
    pub entries_valid: bool,
    pub kernels_present: Vec<String>,
    pub kernels_missing: Vec<String>,
    pub entry_issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootInfo {
    pub current_entry: String,
    pub bootloader_version: String,
    pub entries: Vec<BootEntryInfo>,
    pub backup_bootable: bool,
    pub backup_bootloader_version: Option<String>,
    pub booted_from: DiskSide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootEntryInfo {
    pub title: String,
    pub id: String,
    pub root_uuid: String,
    pub kernel: String,
    /// Which EFI partition the entry lives on.
    pub disk: DiskSide,
}

// ─── Boot Entry Parser ───────────────────────────────────────

fn parse_boot_entry(content: &str) -> ParsedBootEntry {
    let mut e = ParsedBootEntry {
        title: String::new(), root_uuid: String::new(),
        kernel: String::new(), sort_key: String::new(),
    };

    /// Try to extract `root=UUID=…` or `root=/dev/disk/by-uuid/…` from a token.
    fn extract_root_uuid(token: &str) -> Option<&str> {
        token.strip_prefix("root=UUID=")
            .or_else(|| token.strip_prefix("root=/dev/disk/by-uuid/"))
    }

    for raw in content.lines() {
        let line = raw.trim();
        if let Some(val) = line.strip_prefix("title ") {
            e.title = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("sort-key ") {
            e.sort_key = val.trim().to_string();
        } else if line.starts_with("linux ") || line.starts_with("linuxefi ") || line.starts_with("linux16 ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 1 { e.kernel = parts[1].trim_start_matches('/').to_string(); }
            for p in &parts {
                if let Some(u) = extract_root_uuid(p) { e.root_uuid = u.to_string(); break; }
            }
        } else if let Some(val) = line.strip_prefix("options ") {
            for p in val.split_whitespace() {
                if let Some(u) = extract_root_uuid(p) { e.root_uuid = u.to_string(); break; }
            }
        } else if line.starts_with("menuentry ") {
            // GRUB: title is the first quoted string
            if let Some(start) = line.find('\'').or_else(|| line.find('"')) {
                let q = line.as_bytes()[start] as char;
                if let Some(end) = line[start + 1..].find(q) {
                    e.title = line[start + 1..start + 1 + end].to_string();
                }
            }
        }
    }
    e
}

// ─── Cached Accessors ─────────────────────────────────────────

pub(crate) fn get_cached_boot_validation(backup_efi_dev: &str, c: &AppConfig) -> BootValidation {
    const TTL: Duration = Duration::from_secs(10);

    if let Some(v) = BOOT_VALIDATION_CACHE.get(TTL) {
        return v;
    }

    let v = validate_backup_boot(backup_efi_dev, c);
    BOOT_VALIDATION_CACHE.set(v.clone());
    v
}

pub(crate) fn get_cached_boot_info(c: &AppConfig) -> BootInfo {
    const TTL: Duration = Duration::from_secs(10);

    if let Some(info) = BOOT_INFO_CACHE.get(TTL) {
        return info;
    }

    let info = gather_boot_info(c);
    BOOT_INFO_CACHE.set(info.clone());
    info
}

// ─── Helpers ──────────────────────────────────────────────────

/// Mount an EFI partition read-only, reusing an existing mount if detected.
fn mount_efi_ro(dev: &str, fallback: &str) -> Result<(String, Option<AutoUmount>), String> {
    if let Some(mnt) = crate::sysfs::find_device_mountpoint(dev) {
        return Ok((mnt, None));
    }
    if fs::create_dir_all(fallback).is_err() {
        let r = run_privileged("mkdir", &["-p", fallback]);
        if !r.success {
            return Err(format!("mkdir {}: {}", fallback, r.stderr.trim()));
        }
    }
    let r = run_privileged("mount", &["-o", "ro", dev, fallback]);
    if !r.success {
        return Err(format!("mount {} → {}: {}", dev, fallback, r.stderr.trim()));
    }
    Ok((fallback.to_string(), Some(AutoUmount(fallback.to_string()))))
}

fn unique_tmp_mount(prefix: &str) -> String {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = format!("/tmp/{}-{}-{}", prefix, pid, nanos);
    if fs::create_dir_all(&path).is_err() {
        let _ = run_privileged("mkdir", &["-p", &path]);
    }
    path
}

/// Read file contents.  Falls back to privileged `cat` if unreadable.
fn read_file_privileged(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| {
        let r = run_privileged("cat", &[path]);
        if r.success {
            r.stdout
        } else {
            String::new()
        }
    })
}

/// Read systemd-boot loader entries from a directory.
/// `disk` is passed through as-is ("Primary" / "Backup").
fn read_loader_entries(dir: &str, disk: DiskSide) -> Vec<BootEntryInfo> {
    #[derive(Clone)]
    struct Parsed {
        sort_key: String,
        title: String,
        id: String,
        info: BootEntryInfo,
    }

    let mut parsed: Vec<Parsed> = list_conf_files(dir)
        .iter()
        .map(|fname| {
            let id = fname.trim_end_matches(".conf").to_string();
            let full_path = format!("{}/{}", dir, fname);
            let content = read_file_privileged(&full_path);
            let parsed = parse_boot_entry(&content);
            let info = BootEntryInfo {
                title: parsed.title.clone(),
                id: id.clone(),
                root_uuid: parsed.root_uuid,
                kernel: parsed.kernel,
                disk,
            };
            Parsed {
                sort_key: parsed.sort_key,
                title: parsed.title,
                id,
                info,
            }
        })
        .collect();

    // systemd-boot supports `sort-key`; prefer it if present.
    // Fallback: title then id for stable ordering.
    parsed.sort_by(|a, b| {
        let a_empty = a.sort_key.trim().is_empty();
        let b_empty = b.sort_key.trim().is_empty();

        match (a_empty, b_empty) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => {
                let ak = a.sort_key.to_lowercase();
                let bk = b.sort_key.to_lowercase();
                let at = a.title.to_lowercase();
                let bt = b.title.to_lowercase();
                let ai = a.id.to_lowercase();
                let bi = b.id.to_lowercase();
                ak.cmp(&bk).then(at.cmp(&bt)).then(ai.cmp(&bi))
            }
        }
    });

    parsed.into_iter().map(|p| p.info).collect()
}

/// Parse a GRUB config into boot entries.
fn parse_grub_entries(grub_cfg: &str, disk: DiskSide) -> Vec<BootEntryInfo> {
    let mut entries = Vec::new();
    let mut block = String::new();
    let mut in_entry = false;
    let mut idx = 0u32;
    for line in grub_cfg.lines() {
        if line.starts_with("menuentry ") {
            if in_entry && !block.is_empty() {
                push_grub_entry(&block, disk, &mut idx, &mut entries);
            }
            in_entry = true;
            block.clear();
            block.push_str(line);
            block.push('\n');
        } else if in_entry {
            if line.trim() == "}" {
                push_grub_entry(&block, disk, &mut idx, &mut entries);
                in_entry = false;
            } else {
                block.push_str(line);
                block.push('\n');
            }
        }
    }
    if in_entry && !block.is_empty() {
        push_grub_entry(&block, disk, &mut idx, &mut entries);
    }
    entries
}

fn push_grub_entry(block: &str, disk: DiskSide, idx: &mut u32, out: &mut Vec<BootEntryInfo>) {
    let parsed = parse_boot_entry(block);
    let id = if parsed.title.is_empty() {
        format!("grub-entry-{}", idx)
    } else {
        parsed.title.replace(' ', "-").to_lowercase()
    };
    out.push(BootEntryInfo {
        title: parsed.title,
        id,
        root_uuid: parsed.root_uuid,
        kernel: parsed.kernel,
        disk,
    });
    *idx += 1;
}

/// Extract bootloader version from an EFI binary via `strings`.
fn extract_bl_version(efi_path: &str, prefix: &str) -> Option<String> {
    if !Path::new(efi_path).exists() {
        return None;
    }
    let r = run_cmd("strings", &[efi_path]);
    if !r.success {
        return None;
    }
    for line in r.stdout.lines() {
        if let Some(pos) = line.find(prefix) {
            let ver: String = line[pos + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if !ver.is_empty() {
                return Some(format!("{}{}", prefix, ver));
            }
        }
    }
    None
}

/// Locate grub.cfg under a mount point.  Falls back to privileged `test`.
fn find_grub_cfg(base: &str) -> Option<String> {
    let candidates = [
        format!("{}/grub/grub.cfg", base),
        format!("{}/boot/grub/grub.cfg", base),
        format!("{}/grub2/grub.cfg", base),
        format!("{}/boot/grub2/grub.cfg", base),
    ];
    if let Some(p) = candidates.iter().find(|p| Path::new(p.as_str()).exists()) {
        return Some(p.clone());
    }
    for p in &candidates {
        if run_privileged("test", &["-f", p]).success {
            return Some(p.clone());
        }
    }
    None
}

/// Collect kernel / initramfs filenames from a mounted partition.
fn collect_kernel_files(mnt: &str) -> Vec<String> {
    fs::read_dir(mnt)
        .into_iter()
        .flat_map(std::iter::Iterator::flatten)
        .filter_map(|de| {
            let name = de.file_name().to_string_lossy().into_owned();
            if name.starts_with("vmlinuz-") || name.starts_with("initramfs-") {
                Some(name)
            } else {
                None
            }
        })
        .collect()
}

/// Detect whether a bootloader EFI binary is present on a mounted partition.
fn bootloader_present_on(mnt: &str, bl: BootloaderType, suffix: &str) -> bool {
    match bl {
        BootloaderType::Grub => {
            let su = suffix.to_uppercase();
            Path::new(&format!("{}/EFI/BOOT/BOOT{}.EFI", mnt, su)).exists()
                || Path::new(&format!("{}/EFI/grub/grub{}.efi", mnt, suffix)).exists()
                || Path::new(&format!("{}/grub", mnt)).is_dir()
        }
        BootloaderType::SystemdBoot => Path::new(&format!("{}/EFI/systemd/systemd-boot{}.efi", mnt, suffix)).exists(),
    }
}

// ─── Boot Validation ──────────────────────────────────────────

fn boot_validation_err(msg: String) -> BootValidation {
    BootValidation {
        backup_efi_accessible: false, bootloader_present: false,
        entries_valid: false, kernels_present: Vec::new(), kernels_missing: Vec::new(),
        entry_issues: vec![msg],
    }
}

fn validate_backup_boot(backup_efi_dev: &str, c: &AppConfig) -> BootValidation {
    if !validate_device_path(backup_efi_dev) {
        return boot_validation_err(format!("Ungültiger EFI-Device-Pfad: '{}'", backup_efi_dev));
    }

    let _efi_lock = match EfiMountLock::acquire_timeout(Duration::from_secs(2)) {
        Ok(g) => g,
        Err(e) => return boot_validation_err(format!("EFI-Lock fehlgeschlagen: {}", e)),
    };

    let tmp_mnt = unique_tmp_mount("backsnap-boot-validate");
    let (mnt, _guard) = match mount_efi_ro(backup_efi_dev, &tmp_mnt) {
        Ok(v) => v,
        Err(e) => return boot_validation_err(format!("Backup-EFI {} nicht mountbar: {}", backup_efi_dev, e)),
    };

    let bl = c.boot.bootloader_type;
    let arch = detect_efi_arch();
    let suffix = efi_arch_suffix(&arch);
    let bl_found = bootloader_present_on(&mnt, bl, suffix);

    let raw_entries = bl.read_entries(&mnt, DiskSide::Backup);

    let available_files = collect_kernel_files(&mnt);
    let mut issues = Vec::new();
    let mut kernels_present = Vec::new();
    let mut kernels_missing = Vec::new();

    let boot_side = DiskSide::from_uuid(&get_boot_uuid(), c);
    let backup_uuid = boot_side.other_uuid(c);

    for e in &raw_entries {
        if !e.root_uuid.is_empty() && e.root_uuid != *backup_uuid {
            issues.push(format!(
                "{}: UUID {} zeigt nicht auf Backup-Disk (erwartet: {})",
                e.id, e.root_uuid, backup_uuid
            ));
        }
        if !e.kernel.is_empty() {
            let fname = e.kernel.trim_start_matches('/');
            if available_files.iter().any(|f| f == fname) {
                kernels_present.push(e.kernel.clone());
            } else {
                kernels_missing.push(e.kernel.clone());
                issues.push(format!(
                    "{}: Kernel '{}' fehlt auf Backup-EFI",
                    e.id, e.kernel
                ));
            }
        }
    }

    if !bl_found {
        issues.push(format!("{} Bootloader fehlt auf Backup-EFI", bl));
    }
    if raw_entries.is_empty() {
        issues.push("Keine Boot-Entries auf Backup-EFI gefunden".to_string());
    }

    BootValidation {
        backup_efi_accessible: true,
        bootloader_present: bl_found,
        entries_valid: issues.is_empty(),
        kernels_present,
        kernels_missing,
        entry_issues: issues,
    }
}

// ─── Boot Info Gathering ──────────────────────────────────────

/// Inspect a backup EFI partition: mount, read entries, determine bootability.
fn inspect_backup_efi(
    efi_dev: &str,
    bl: BootloaderType,
    suffix: &str,
    expected_backup_uuid: &str,
) -> (bool, Option<String>, Vec<BootEntryInfo>) {
    let efi_lock = match EfiMountLock::acquire_timeout(Duration::from_secs(2)) {
        Ok(g) => Some(g),
        Err(e) => {
            log::warn!("boot-info: EFI lock failed; skipping backup inspection: {}", e);
            return (false, None, Vec::new());
        }
    };

    if efi_lock.is_none() {
        return (false, None, Vec::new());
    }

    let tmp_mnt = unique_tmp_mount("backsnap-boot-check");
    let Ok((mnt, _guard)) = mount_efi_ro(efi_dev, &tmp_mnt) else {
        return (false, None, Vec::new());
    };

    let has_bl = bootloader_present_on(&mnt, bl, suffix);
    let bl_version = bl.extract_version_from_efi(&mnt, suffix);
    let backup_entries: Vec<_> = bl
        .read_entries(&mnt, DiskSide::Backup)
        .into_iter()
        .filter(|e| e.root_uuid.is_empty() || e.root_uuid == *expected_backup_uuid)
        .collect();
    let bootable = has_bl && !backup_entries.is_empty();

    (bootable, bl_version, backup_entries)
}

/// Normalize backup entry titles to show the configured backup label.
fn normalize_backup_titles(entries: &mut [BootEntryInfo], backup_label: &str) {
    if backup_label.is_empty() {
        return;
    }
    for e in entries.iter_mut().filter(|e| e.disk == DiskSide::Backup) {
        if let (Some(start), Some(end)) = (e.title.find('('), e.title.rfind(')')) {
            if end > start {
                e.title = format!("{}({}){}", &e.title[..start], backup_label, &e.title[end + 1..]);
                continue;
            }
        }
        e.title = format!("{} ({})", e.title, backup_label);
    }
}

fn gather_boot_info(c: &AppConfig) -> BootInfo {
    let boot_uuid = get_boot_uuid();
    let booted_from = DiskSide::from_uuid(&boot_uuid, c);

    let backup_efi = if c.disks.backup_uuid.is_empty() {
        None
    } else {
        let other = booted_from.other_uuid(c);
        crate::sysfs::resolve_uuid(other).map(|dev| derive_efi_partition(&dev))
    };
    let backup_efi = backup_efi.filter(|dev| validate_device_path(dev));

    let bl = c.boot.bootloader_type;
    let arch = detect_efi_arch();
    let suffix = efi_arch_suffix(&arch);

    // ── Primary EFI entries ─────────────────────────────────

    let mut current_entry = String::new();
    let mut bootloader_version = String::new();
    let mut entries: Vec<BootEntryInfo> = Vec::new();

    match bl {
        BootloaderType::Grub => {
            let ver = run_cmd("grub-install", &["--version"]);
            bootloader_version = if ver.success && !ver.stdout.trim().is_empty() {
                ver.stdout.trim().to_string()
            } else {
                run_cmd("grub2-install", &["--version"])
                    .stdout
                    .trim()
                    .to_string()
            };
        }
        BootloaderType::SystemdBoot => {
            let status = run_privileged("bootctl", &["status"]);
            for line in status.stdout.lines() {
                let t = line.trim();
                if current_entry.is_empty() {
                    if let Some(v) = t.strip_prefix("Current Entry:") {
                        current_entry = v.trim().to_string();
                    }
                }
                if bootloader_version.is_empty() {
                    if let Some(v) = t.strip_prefix("Product:") {
                        bootloader_version = v.trim().to_string();
                    }
                }
            }
        }
    }
    entries.extend(bl.read_entries("/boot", DiskSide::Primary));
    if entries.is_empty() && matches!(bl, BootloaderType::Grub) {
        entries.extend(bl.read_entries("/", DiskSide::Primary));
    }

    entries.retain(|e| e.root_uuid.is_empty() || e.root_uuid == boot_uuid);

    if current_entry.is_empty() {
        current_entry = fs::read_to_string("/proc/cmdline")
            .unwrap_or_default()
            .split_whitespace()
            .find(|p| p.starts_with("BOOT_IMAGE=")).map_or_else(|| "unknown".to_string(), |p| p.trim_start_matches("BOOT_IMAGE=").to_string());
    }

    // ── Backup EFI entries ──────────────────────────────────

    let mut backup_bootable = false;
    let mut backup_bl_version: Option<String> = None;

    if let Some(ref efi_dev) = backup_efi {
        let expected_backup_uuid = booted_from.other_uuid(c);
        let (bootable, bl_ver, backup_entries) =
            inspect_backup_efi(efi_dev, bl, suffix, expected_backup_uuid);
        backup_bootable = bootable;
        backup_bl_version = bl_ver;
        entries.extend(backup_entries);
    }

    normalize_backup_titles(&mut entries, c.disks.backup_label.trim());

    BootInfo {
        current_entry,
        bootloader_version,
        entries,
        backup_bootable,
        backup_bootloader_version: backup_bl_version,
        booted_from,
    }
}

// ─── Tauri Command ────────────────────────────────────────────

#[tauri::command]
pub async fn get_boot_info() -> Result<BootInfo, String> {
    tokio::task::spawn_blocking(|| {
        let c = cfg();
        Ok(get_cached_boot_info(&c))
    })
    .await
    .map_err(|e| format!("Boot-Info thread panicked: {}", e))?
}
