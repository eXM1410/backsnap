use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tauri::command;

use super::helpers::{
    is_root, read_sys, run_cmd, run_file_ops_batch, run_privileged,
    CommandResult as CmdResult, FileOp,
};

// ─── Types ────────────────────────────────────────────────────

/// What kind of UI control this tweak uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TweakControl {
    Toggle,
    Select,
    Slider,
    Info,
}

/// Logical category for grouping tweaks in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::upper_case_acronyms)]
pub enum TweakCategory {
    #[serde(rename = "I/O")]
    Io,
    Memory,
    #[serde(rename = "Netzwerk")]
    Network,
    GPU,
    #[serde(rename = "Dienste")]
    Services,
    #[serde(rename = "Dateisystem")]
    Filesystem,
    System,
}

/// GPU fan control mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FanMode {
    #[default]
    Auto,
    Manual,
}

impl FanMode {
    fn from_pwm_enable(val: u32) -> Self {
        if val == 1 { Self::Manual } else { Self::Auto }
    }

    fn pwm_enable_str(self) -> &'static str {
        match self { Self::Manual => "1", Self::Auto => "2" }
    }
}

impl std::fmt::Display for FanMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

// ─── GPU Sub-Structs (flattened into GpuOcStatus) ─────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuClockInfo {
    pub sclk_min: u32,
    pub sclk_max: u32,
    pub sclk_range_min: u32,
    pub sclk_range_max: u32,
    pub mclk_max: u32,
    pub mclk_range_min: u32,
    pub mclk_range_max: u32,
    pub current_sclk_mhz: u32,
    pub current_mclk_mhz: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuVoltageInfo {
    #[serde(rename = "voltage_offset")]
    pub offset: i32,
    #[serde(rename = "voltage_min")]
    pub min: i32,
    #[serde(rename = "voltage_max")]
    pub max: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[allow(clippy::struct_field_names)]
pub struct GpuPowerInfo {
    #[serde(rename = "power_cap_w")]
    pub cap_w: u32,
    #[serde(rename = "power_default_w")]
    pub default_w: u32,
    #[serde(rename = "power_max_w")]
    pub max_w: u32,
    #[serde(rename = "power_current_w")]
    pub current_w: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuTempInfo {
    #[serde(rename = "temp_edge")]
    pub edge: f32,
    #[serde(rename = "temp_junction")]
    pub junction: f32,
    #[serde(rename = "temp_mem")]
    pub mem: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuFanInfo {
    #[serde(rename = "fan_rpm")]
    pub rpm: u32,
    #[serde(rename = "fan_max_rpm")]
    pub max_rpm: u32,
    #[serde(rename = "fan_pwm")]
    pub pwm: u32,
    #[serde(rename = "fan_mode")]
    pub mode: FanMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningStatus {
    pub tweaks: Vec<TweakInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TweakInfo {
    pub id: String,
    pub category: TweakCategory,
    pub name: String,
    pub description: String,
    /// Current raw value
    pub current: String,
    /// Human-readable status
    pub status: String,
    /// Whether this tweak is currently "enabled" / "optimal"
    pub active: bool,
    /// Recommended value
    pub recommended: String,
    /// Available options (if applicable)
    pub options: Vec<String>,
    pub control: TweakControl,
    /// For sliders: min value
    pub min: Option<i64>,
    /// For sliders: max value
    pub max: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningApplyResult {
    pub success: bool,
    pub message: String,
    pub new_value: String,
}

// ─── GPU Overclock Types ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuOcStatus {
    pub available: bool,
    pub gpu_name: String,
    pub vram_mb: u64,
    #[serde(flatten)]
    pub clocks: GpuClockInfo,
    #[serde(flatten)]
    pub voltage: GpuVoltageInfo,
    #[serde(flatten)]
    pub power: GpuPowerInfo,
    #[serde(flatten)]
    pub temps: GpuTempInfo,
    #[serde(flatten)]
    pub fan: GpuFanInfo,
    pub gpu_busy_percent: u32,
}

#[allow(clippy::too_many_arguments)]
impl TweakInfo {
    fn info(id: &str, cat: TweakCategory, name: &str, desc: &str, current: &str, status: &str, active: bool, recommended: &str) -> Self {
        Self {
            id: id.into(), category: cat, name: name.into(), description: desc.into(),
            current: current.into(), status: status.into(), active, recommended: recommended.into(),
            options: vec![], control: TweakControl::Info, min: None, max: None,
        }
    }
    fn select(id: &str, cat: TweakCategory, name: &str, desc: &str, current: &str, status: &str, active: bool, recommended: &str, options: Vec<String>) -> Self {
        Self { options, control: TweakControl::Select, ..Self::info(id, cat, name, desc, current, status, active, recommended) }
    }
    fn toggle(id: &str, cat: TweakCategory, name: &str, desc: &str, current: &str, status: &str, active: bool, recommended: &str) -> Self {
        Self { control: TweakControl::Toggle, ..Self::info(id, cat, name, desc, current, status, active, recommended) }
    }
    fn slider(id: &str, cat: TweakCategory, name: &str, desc: &str, current: &str, status: &str, active: bool, recommended: &str, min: i64, max: i64) -> Self {
        Self { min: Some(min), max: Some(max), control: TweakControl::Slider, ..Self::info(id, cat, name, desc, current, status, active, recommended) }
    }
}

/// Build a TweakInfo for a systemd service (installed/active/inactive pattern).
fn service_tweak(id: &str, name: &str, desc: &str, pkg: &str, unit: &str, user: bool) -> TweakInfo {
    let installed = is_package_installed(pkg);
    let active = systemctl_is_active(unit, user);
    let (current, status) = if active {
        ("active", "Aktiv ✓")
    } else if installed {
        ("installed", "Installiert, aber inaktiv")
    } else {
        ("not_installed", "Nicht installiert")
    };
    TweakInfo::toggle(id, TweakCategory::Services, name, desc, current, status, active, "active")
}

// ─── GPU OC Profile (persisted to disk) ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuOcProfile {
    pub sclk_max: u32,
    pub mclk_max: u32,
    pub voltage_offset: i32,
    pub power_cap_w: u32,
    pub performance_level: String,
    pub power_profile_index: u32,
    pub fan_mode: FanMode,
    pub fan_pwm: u32,
}

const OC_PROFILE_PATH: &str = "/etc/backsnap/gpu-oc.json";
const OC_APPLY_SERVICE: &str = "backsnap-gpu-oc.service";

fn load_oc_profile() -> Option<GpuOcProfile> {
    let content = fs::read_to_string(OC_PROFILE_PATH).ok()?;
    serde_json::from_str(&content).ok()
}

// ─── Helper: sysctl reader ───────────────────────────────────

fn read_sysctl(key: &str) -> String {
    let path = format!("/proc/sys/{}", key.replace('.', "/"));
    read_sys(&path)
}

/// Strip trailing "Mhz"/"MHz"/"mV"/"mv" and parse to number.
fn parse_unit<T: std::str::FromStr + Default>(s: &str) -> T {
    s.trim_end_matches("Mhz")
        .trim_end_matches("MHz")
        .trim_end_matches("mV")
        .trim_end_matches("mv")
        .parse()
        .unwrap_or_default()
}

fn systemctl_check(unit: &str, check: &str, user: bool) -> bool {
    let mut cmd = std::process::Command::new("systemctl");
    if user { cmd.arg("--user"); }
    cmd.args([check, "--quiet", unit])
        .status()
        .is_ok_and(|s| s.success())
}

fn systemctl_is_active(unit: &str, user: bool) -> bool { systemctl_check(unit, "is-active", user) }
fn systemctl_is_enabled(unit: &str, user: bool) -> bool { systemctl_check(unit, "is-enabled", user) }

fn is_package_installed(pkg: &str) -> bool {
    std::process::Command::new("pacman")
        .args(["-Qi", pkg])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ─── Read current GPU profile ─────────────────────────────────

/// Cached AMD GPU card detection — scanned once, reused for all calls.
static AMD_GPU_CARD: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

fn find_amd_gpu_card() -> Option<String> {
    AMD_GPU_CARD.get_or_init(find_amd_gpu_card_inner).clone()
}

fn find_amd_gpu_card_inner() -> Option<String> {
    // Prefer the "real" AMD GPU card in multi-GPU setups.
    // Strategy:
    // - only consider cards that expose pp_power_profile_mode (amdgpu)
    // - prefer cards that also expose pp_od_clk_voltage (OC interface)
    // - otherwise pick the one with the largest VRAM
    #[derive(Debug)]
    struct Candidate {
        name: String,
        has_od: bool,
        vram_bytes: u64,
    }

    let mut candidates: Vec<Candidate> = Vec::new();

    for entry in fs::read_dir("/sys/class/drm/").ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }

        let profile_path = format!("/sys/class/drm/{}/device/pp_power_profile_mode", name);
        if !Path::new(&profile_path).exists() {
            continue;
        }

        let od_path = format!("/sys/class/drm/{}/device/pp_od_clk_voltage", name);
        let has_od = Path::new(&od_path).exists();

        let vram_bytes: u64 = read_sys(&format!(
            "/sys/class/drm/{}/device/mem_info_vram_total",
            name
        ))
        .parse()
        .unwrap_or_default();

        candidates.push(Candidate {
            name,
            has_od,
            vram_bytes,
        });
    }

    candidates.sort_by(|a, b| {
        b.has_od
            .cmp(&a.has_od)
            .then_with(|| b.vram_bytes.cmp(&a.vram_bytes))
    });

    candidates.first().map(|c| c.name.clone())
}

fn read_gpu_profile() -> (String, Vec<String>) {
    let Some(card) = find_amd_gpu_card() else { return ("N/A".into(), vec![]) };
    let path = format!("/sys/class/drm/{}/device/pp_power_profile_mode", card);
    let content = read_sys(&path);
    let mut current = String::new();
    let mut options = vec![];

    for line in content.lines().skip(1) {
        // Lines look like: " 1 3D_FULL_SCREEN*:"
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            let name_raw = parts[1].trim_end_matches(':').trim_end_matches('*');
            let is_active = parts[1].contains('*');
            options.push(name_raw.to_string());
            if is_active {
                current = name_raw.to_string();
            }
        }
    }

    (current, options)
}

fn read_gpu_power_level() -> String {
    let Some(card) = find_amd_gpu_card() else { return "N/A".into() };
    let path = format!(
        "/sys/class/drm/{}/device/power_dpm_force_performance_level",
        card
    );
    read_sys(&path)
}

// ─── IO Scheduler ─────────────────────────────────────────────

fn read_io_scheduler(device: &str) -> (String, Vec<String>) {
    let path = format!("/sys/block/{}/queue/scheduler", device);
    let content = read_sys(&path);
    let mut current = String::new();
    let mut options = vec![];

    for part in content.split_whitespace() {
        if part.starts_with('[') && part.ends_with(']') {
            let name = part
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            current.clone_from(&name);
            options.push(name);
        } else {
            options.push(part.to_string());
        }
    }
    (current, options)
}

// ─── Commands ─────────────────────────────────────────────────

#[command]
pub async fn get_tuning_status() -> Result<TuningStatus, String> {
    tokio::task::spawn_blocking(|| Ok(get_tuning_status_inner()))
        .await
        .map_err(|e| format!("spawn_blocking: {}", e))?
}

fn get_tuning_status_inner() -> TuningStatus {
    let mut tweaks = Vec::new();
    collect_io_tweaks(&mut tweaks);
    collect_memory_tweaks(&mut tweaks);
    collect_network_tweaks(&mut tweaks);
    collect_gpu_tweaks(&mut tweaks);
    collect_service_tweaks(&mut tweaks);
    collect_filesystem_tweaks(&mut tweaks);
    collect_system_tweaks(&mut tweaks);
    TuningStatus { tweaks }
}

// ─── Per-Category Tweak Collectors ────────────────────────────

fn collect_io_tweaks(tweaks: &mut Vec<TweakInfo>) {
    // I/O Scheduler (NVMe)
    let nvme_devices: Vec<String> = fs::read_dir("/sys/block/")
        .map(|entries| {
            entries
                .filter_map(std::result::Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("nvme"))
                .collect()
        })
        .unwrap_or_default();
    let first_nvme = nvme_devices.first().cloned().unwrap_or_else(|| "nvme0n1".into());
    let (io_current, io_options) = read_io_scheduler(&first_nvme);
    let io_optimal = io_current == "none";
    tweaks.push(TweakInfo::select(
        "io_scheduler", TweakCategory::Io, "I/O Scheduler",
        "NVMe-SSDs brauchen keinen I/O-Scheduler — 'none' ist optimal.",
        &io_current,
        &if io_optimal { "Optimal (none)".into() } else { format!("Suboptimal ({})", io_current) },
        io_optimal, "none", io_options,
    ));

    // Dirty Writeback
    let dirty_writeback: i64 = read_sysctl("vm.dirty_writeback_centisecs")
        .parse()
        .unwrap_or(500);
    let dirty_expire: i64 = read_sysctl("vm.dirty_expire_centisecs")
        .parse()
        .unwrap_or(3000);
    tweaks.push(TweakInfo::info(
        "dirty_writeback", TweakCategory::Io, "Dirty Writeback",
        &format!(
            "Wie oft schmutzige Pages auf Disk geschrieben werden. Writeback: {}ms, Expire: {}ms. Höhere Werte = weniger I/O, aber mehr Datenverlust-Risiko bei Crash.",
            dirty_writeback * 10, dirty_expire * 10
        ),
        &format!("{}/{}", dirty_writeback, dirty_expire),
        #[allow(clippy::cast_precision_loss)]
        &format!("Writeback {}s / Expire {}s", dirty_writeback as f64 / 100.0, dirty_expire as f64 / 100.0),
        true, "1500/3000",
    ));

    // fstrim.timer
    let fstrim_enabled = systemctl_is_enabled("fstrim.timer", false);
    tweaks.push(TweakInfo::toggle(
        "fstrim_timer", TweakCategory::Io, "TRIM Timer",
        "Wöchentlicher SSD-TRIM für konstante Schreibgeschwindigkeit. Ergänzt discard=async.",
        if fstrim_enabled { "enabled" } else { "disabled" },
        if fstrim_enabled { "Aktiv ✓" } else { "Inaktiv" },
        fstrim_enabled, "enabled",
    ));
}

fn collect_memory_tweaks(tweaks: &mut Vec<TweakInfo>) {
    // Swappiness
    let swappiness: i64 = read_sysctl("vm.swappiness").parse().unwrap_or(60);
    let has_zram = Path::new("/dev/zram0").exists();
    let swap_optimal = if has_zram { swappiness >= 100 } else { swappiness <= 20 };
    tweaks.push(TweakInfo::slider(
        "swappiness", TweakCategory::Memory, "Swappiness",
        if has_zram { "Mit ZRAM ist ein hoher Wert (100-200) optimal — ZRAM-Swap ist schneller als RAM-Komprimierung." }
        else { "Ohne ZRAM sollte der Wert niedrig sein (10-20), damit RAM bevorzugt wird." },
        &swappiness.to_string(),
        &format!("{}{}", swappiness, if swap_optimal { " ✓" } else { " — nicht optimal" }),
        swap_optimal, if has_zram { "150" } else { "10" }, 0, 200,
    ));

    // ZRAM Status
    let zram_active = Path::new("/dev/zram0").exists();
    let zram_info = if zram_active {
        let output = std::process::Command::new("zramctl")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let line = output.lines().nth(1).unwrap_or_default().to_string();
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 {
            format!("{} Disk, {} komprimiert, Algo: {}", parts[2], parts[4], parts[1])
        } else {
            "Aktiv".into()
        }
    } else {
        "Nicht aktiv".into()
    };
    tweaks.push(TweakInfo::info(
        "zram", TweakCategory::Memory, "ZRAM Swap", "Komprimierter RAM-Swap — viel schneller als Disk-Swap.",
        if zram_active { "active" } else { "inactive" }, &zram_info, zram_active, "active",
    ));

    // Transparent Hugepages
    let thp_raw = read_sys("/sys/kernel/mm/transparent_hugepage/enabled");
    let thp_current: String = if thp_raw.contains("[always]") {
        "always".into()
    } else if thp_raw.contains("[madvise]") {
        "madvise".into()
    } else {
        "never".into()
    };
    let thp_optimal = thp_current == "madvise";
    tweaks.push(TweakInfo::select(
        "thp", TweakCategory::Memory, "Transparent Hugepages",
        "madvise = nur wenn Apps es anfordern (optimal). always = kann zu Latenzen führen. never = deaktiviert.",
        &thp_current,
        &format!("{}{}", thp_current, if thp_optimal { " ✓" } else { "" }),
        thp_optimal, "madvise",
        vec!["always".into(), "madvise".into(), "never".into()],
    ));
}

fn collect_network_tweaks(tweaks: &mut Vec<TweakInfo>) {
    let tcp_cc = read_sysctl("net.ipv4.tcp_congestion_control");
    let tcp_available: Vec<String> = read_sysctl("net.ipv4.tcp_available_congestion_control")
        .split_whitespace()
        .map(std::string::ToString::to_string)
        .collect();
    let bbr_active = tcp_cc == "bbr";
    tweaks.push(TweakInfo::select(
        "tcp_bbr", TweakCategory::Network, "TCP BBR",
        "Googles Congestion Control — besserer Durchsatz und geringere Latenz als Cubic.",
        &tcp_cc,
        &if bbr_active { "BBR aktiv ✓".into() } else { format!("Aktuell: {}", tcp_cc) },
        bbr_active, "bbr", tcp_available,
    ));
}

fn collect_gpu_tweaks(tweaks: &mut Vec<TweakInfo>) {
    let (gpu_profile, gpu_profile_opts) = read_gpu_profile();
    if !gpu_profile_opts.is_empty() {
        tweaks.push(TweakInfo::select(
            "gpu_profile", TweakCategory::GPU, "GPU Power-Profil",
            "AMD GPU-Profil: 3D_FULL_SCREEN für Gaming, COMPUTE für LLM/KI, POWER_SAVING für Idle.",
            &gpu_profile, &format!("Profil: {}", gpu_profile), true, "3D_FULL_SCREEN", gpu_profile_opts,
        ));

        let gpu_power = read_gpu_power_level();
        tweaks.push(TweakInfo::select(
            "gpu_power_level", TweakCategory::GPU, "GPU Power-Level",
            "auto = dynamisch, high = max Performance, low = Stromsparen, manual = benutzerdefiniert.",
            &gpu_power, &format!("Level: {}", gpu_power),
            gpu_power == "auto" || gpu_power == "manual", "auto",
            vec!["auto".into(), "low".into(), "high".into(), "manual".into()],
        ));
    }

    // RADV_PERFTEST (env vars for AMD Vulkan)
    let env_content = fs::read_to_string("/etc/environment").unwrap_or_default();
    let radv_current = env_content
        .lines()
        .find(|l| l.starts_with("RADV_PERFTEST=")).map_or_else(|| "(nicht gesetzt)".into(), |l| l.trim_start_matches("RADV_PERFTEST=").to_string());
    let has_sam = radv_current.contains("sam");
    let radv_status = if has_sam {
        format!("{} (sam ist redundant ⚠)", radv_current)
    } else {
        format!("{} ✓", radv_current)
    };
    tweaks.push(TweakInfo::select(
        "radv_perftest", TweakCategory::GPU, "RADV_PERFTEST",
        &format!(
            "Vulkan-Treiber Features. 'gpl' = parallele Pipeline-Kompilierung (weniger Stutter). \
             'sam' ist seit Mesa 22.x Standard und nicht mehr nötig. Aktuell: {}",
            radv_current
        ),
        &radv_current, &radv_status,
        !has_sam, "gpl",
        vec!["gpl".into(), "gpl,sam".into(), "sam".into(), "(leer)".into()],
    ));
}

fn collect_service_tweaks(tweaks: &mut Vec<TweakInfo>) {
    for (id, name, desc, pkg, unit, user) in [
        ("earlyoom", "earlyoom",
         "Killt automatisch Speicherfresser bevor das System einfriert. Schützt vor OOM-Lockups.",
         "earlyoom", "earlyoom", false),
        ("psd", "Profile Sync Daemon",
         "Verschiebt Browser-Profile in tmpfs (RAM) — schnelleres Browsing, weniger SSD-Writes.",
         "profile-sync-daemon", "psd.service", true),
        ("ananicy", "ananicy-cpp",
         "Automatische Prozess-Priorisierung — gibt Games/Desktop Vorrang vor Hintergrund-Tasks.",
         "ananicy-cpp", "ananicy-cpp.service", false),
    ] {
        tweaks.push(service_tweak(id, name, desc, pkg, unit, user));
    }
}

fn collect_filesystem_tweaks(tweaks: &mut Vec<TweakInfo>) {
    let btrfs_opts = crate::sysfs::mount_options("/").unwrap_or_default();
    let compress = btrfs_opts
        .split(',')
        .find(|o| o.starts_with("compress"))
        .unwrap_or("keine")
        .to_string();
    let commit = btrfs_opts
        .split(',')
        .find(|o| o.starts_with("commit"))
        .unwrap_or("default")
        .to_string();
    tweaks.push(TweakInfo::info(
        "btrfs", TweakCategory::Filesystem, "Btrfs Mount-Optionen",
        "Komprimierung und Commit-Intervall für das Root-Dateisystem.",
        &format!("{}, {}", compress, commit),
        &format!("{}, {}", compress, commit),
        compress.contains("zstd") && commit.contains("120"), "compress=zstd:3, commit=120",
    ));
}

fn collect_system_tweaks(tweaks: &mut Vec<TweakInfo>) {
    // Kernel info
    let kernel = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let cachyos = kernel.contains("cachyos");
    tweaks.push(TweakInfo::info(
        "kernel", TweakCategory::System, "Kernel",
        if cachyos {
            "CachyOS-Kernel mit BORE Scheduler, PDS und weiteren Performance-Patches."
        } else {
            "Standard-Kernel. Für bessere Performance: CachyOS-Kernel installieren."
        },
        &kernel,
        &format!("{}{}", kernel, if cachyos { " (optimiert ✓)" } else { "" }),
        cachyos, "cachyos",
    ));

    // PCIe ASPM
    let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let has_pcie_aspm_off = cmdline.contains("pcie_aspm=off");
    let has_amdgpu_aspm = cmdline.contains("amdgpu.aspm=");
    let aspm_status = if has_pcie_aspm_off {
        "pcie_aspm=off (global — deaktiviert ASPM für alle Geräte)"
    } else if has_amdgpu_aspm {
        "amdgpu.aspm=0 (nur GPU — optimal)"
    } else {
        "Standard (ASPM aktiv)"
    };
    tweaks.push(TweakInfo::info(
        "pcie_aspm", TweakCategory::System, "PCIe ASPM",
        "pcie_aspm=off deaktiviert Stromsparen für ALLE PCIe-Geräte (WLAN, NVMe etc.). \
         Besser: amdgpu.aspm=0 deaktiviert es nur für die GPU. \
         Änderung erfordert Boot-Entry und Neustart.",
        aspm_status,
        if has_pcie_aspm_off { "Global deaktiviert ⚠" }
        else if has_amdgpu_aspm { "GPU-only ✓" }
        else { "Standard" },
        !has_pcie_aspm_off, "amdgpu.aspm=0",
    ));
}

/// Execute a batch of sysfs/procfs writes natively via backsnap --sysfs-write
/// JSON: [{"path": "/sys/...", "value": "123"}, ...]
fn run_sysfs_batch(json: &str) -> CmdResult {
    if is_root() {
        run_cmd("backsnap", &["--sysfs-write", json])
    } else {
        run_cmd("pkexec", &["backsnap", "--sysfs-write", json])
    }
}

/// Write a single value to a sysfs/procfs file via privileged helper
fn write_sys(path: &str, value: &str) -> CmdResult {
    let json = serde_json::json!([{"path": path, "value": value}]).to_string();
    run_sysfs_batch(&json)
}

/// Require a privileged command to succeed, or return Err
fn require_ok(res: &CmdResult, context: &str) -> Result<(), String> {
    if res.success {
        Ok(())
    } else {
        Err(format!("{}: {}", context, res.stderr.trim()))
    }
}

fn ok_result(msg: impl Into<String>, val: impl Into<String>) -> TuningApplyResult {
    let m = msg.into();
    let v = val.into();
    TuningApplyResult { success: true, message: m, new_value: v }
}

/// Toggle a systemd service: install package if needed, enable/disable.
fn apply_service_toggle(name: &str, pkg: &str, unit: &str, user: bool, value: &str) -> Result<TuningApplyResult, String> {
    let enable = value == "active" || value == "true";
    let action = if enable { "enable" } else { "disable" };
    if enable && !is_package_installed(pkg) {
        let res = run_privileged("pacman", &["-S", "--noconfirm", pkg]);
        require_ok(&res, &format!("{} installieren", pkg))?;
    }
    if user {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", action, "--now", unit])
            .status();
    } else {
        let res = run_privileged("systemctl", &[action, "--now", unit]);
        require_ok(&res, &format!("{} {}", name, action))?;
    }
    let (new_val, verb) = if enable { ("active", "aktiviert") } else { ("inactive", "deaktiviert") };
    Ok(ok_result(format!("{} {}", name, verb), new_val))
}

/// Read a hwmon sensor file and convert microwatts → watts (u32).
fn read_hwmon_watts(hwmon: &str, file: &str) -> u32 {
    u32::try_from(
        read_sys(&format!("{}/{}", hwmon, file))
            .parse::<u64>()
            .unwrap_or_default()
            / 1_000_000,
    )
    .unwrap_or_default()
}

/// Read a hwmon temperature sensor and convert millidegrees → °C.
fn read_hwmon_temp(hwmon: &str, file: &str) -> f32 {
    read_sys(&format!("{}/{}", hwmon, file))
        .parse::<f32>()
        .unwrap_or(0.0)
        / 1000.0
}

const BACKSNAP_SYSCTL_PATH: &str = "/etc/sysctl.d/99-backsnap.conf";

/// Persist a sysctl key=value into /etc/sysctl.d/99-backsnap.conf.
/// Updates the key if it already exists, appends if not.
fn persist_sysctl(key: &str, value: &str) -> Result<(), String> {
    let content = fs::read_to_string(BACKSNAP_SYSCTL_PATH).unwrap_or_else(|_| {
        "# Managed by backsnap — do not edit manually\n".to_string()
    });
    let mut found = false;
    let new_content: String = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('#') && trimmed.starts_with(key) {
                // Check it's the exact key (not a prefix match)
                if let Some(rest) = trimmed.strip_prefix(key) {
                    let rest = rest.trim_start();
                    if rest.starts_with('=') {
                        found = true;
                        return format!("{} = {}", key, value);
                    }
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let final_content = if found {
        format!("{}\n", new_content.trim_end())
    } else {
        format!("{}\n{} = {}\n", new_content.trim_end(), key, value)
    };
    let json = serde_json::json!([{"path": BACKSNAP_SYSCTL_PATH, "value": final_content}]).to_string();
    let res = run_sysfs_batch(&json);
    if res.success {
        Ok(())
    } else {
        Err(format!("Sysctl persist: {}", res.stderr.trim()))
    }
}

#[command(rename_all = "snake_case")]
pub async fn apply_tuning(tweak_id: String, value: String) -> Result<TuningApplyResult, String> {
    match tweak_id.as_str() {
        "io_scheduler" => apply_io_scheduler(&value),
        "swappiness" => apply_sysctl("vm.swappiness", "Swappiness", &value),
        "tcp_bbr" => apply_sysctl("net.ipv4.tcp_congestion_control", "TCP Congestion Control", &value),
        "gpu_profile" => apply_gpu_profile(&value),
        "gpu_power_level" => apply_gpu_power_level(&value),
        "thp" => {
            let res = write_sys("/sys/kernel/mm/transparent_hugepage/enabled", &value);
            require_ok(&res, "THP")?;
            Ok(ok_result(format!("THP auf '{}' gesetzt", value), &value))
        }
        "fstrim_timer" => apply_fstrim_timer(&value),
        "earlyoom" => apply_service_toggle("earlyoom", "earlyoom", "earlyoom", false, &value),
        "psd" => apply_service_toggle("Profile Sync Daemon", "profile-sync-daemon", "psd.service", true, &value),
        "ananicy" => apply_service_toggle("ananicy-cpp", "ananicy-cpp", "ananicy-cpp", false, &value),
        "radv_perftest" => apply_radv_perftest(value),
        _ => Err(format!("Unbekannter Tweak: {}", tweak_id)),
    }
}

fn apply_io_scheduler(value: &str) -> Result<TuningApplyResult, String> {
    let devices: Vec<String> = fs::read_dir("/sys/block/")
        .map_err(|e| e.to_string())?
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("nvme"))
        .collect();
    for dev in &devices {
        let path = format!("/sys/block/{}/queue/scheduler", dev);
        let res = write_sys(&path, value);
        require_ok(&res, &format!("I/O-Scheduler für {}", dev))?;
    }
    Ok(TuningApplyResult {
        success: true,
        message: format!("I/O-Scheduler auf '{}' gesetzt", value),
        new_value: value.to_string(),
    })
}

fn apply_sysctl(key: &str, label: &str, value: &str) -> Result<TuningApplyResult, String> {
    let arg = format!("{}={}", key, value);
    let res = run_privileged("sysctl", &["-w", &arg]);
    require_ok(&res, label)?;
    persist_sysctl(key, value)?;
    Ok(ok_result(format!("{} auf '{}' gesetzt (persistent)", label, value), value))
}

fn apply_gpu_profile(value: &str) -> Result<TuningApplyResult, String> {
    let card = find_amd_gpu_card().ok_or("Keine AMD GPU gefunden")?;
    let path = format!("/sys/class/drm/{}/device/pp_power_profile_mode", card);
    let content = read_sys(&path);
    let found_idx = content.lines().skip(1).find_map(|line| {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let name = parts[1].trim_end_matches(':').trim_end_matches('*');
            if name == value { return Some(parts[0].to_string()); }
        }
        None
    });
    let idx = found_idx.ok_or_else(|| format!("GPU-Profil '{}' nicht gefunden", value))?;
    let res = write_sys(&path, &idx);
    require_ok(&res, "GPU-Profil")?;
    Ok(ok_result(format!("GPU-Profil auf '{}' gesetzt", value), value))
}

fn apply_gpu_power_level(value: &str) -> Result<TuningApplyResult, String> {
    let card = find_amd_gpu_card().ok_or("Keine AMD GPU gefunden")?;
    let path = format!("/sys/class/drm/{}/device/power_dpm_force_performance_level", card);
    let res = write_sys(&path, value);
    require_ok(&res, "GPU Power-Level")?;
    Ok(ok_result(format!("GPU Power-Level auf '{}' gesetzt", value), value))
}

fn apply_fstrim_timer(value: &str) -> Result<TuningApplyResult, String> {
    let enable = value == "enabled" || value == "true";
    let action = if enable { "enable" } else { "disable" };
    let res = run_privileged("systemctl", &[action, "--now", "fstrim.timer"]);
    require_ok(&res, "fstrim.timer")?;
    let (msg, val) = if enable { ("fstrim.timer aktiviert", "enabled") } else { ("fstrim.timer deaktiviert", "disabled") };
    Ok(ok_result(msg, val))
}

fn apply_radv_perftest(value: String) -> Result<TuningApplyResult, String> {
    let env_path = "/etc/environment";
    let content = fs::read_to_string(env_path).unwrap_or_default();
    let new_val = if value == "(leer)" { String::new() } else { value };
    let new_content: String = if new_val.is_empty() {
        content
            .lines()
            .filter(|l| !l.starts_with("RADV_PERFTEST="))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    } else if content.contains("RADV_PERFTEST=") {
        content
            .lines()
            .map(|l| {
                if l.starts_with("RADV_PERFTEST=") {
                    format!("RADV_PERFTEST={}", new_val)
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    } else {
        format!("{}RADV_PERFTEST={}\n", content, new_val)
    };
    let json = serde_json::json!([{"path": env_path, "value": new_content}]).to_string();
    let res = run_sysfs_batch(&json);
    require_ok(&res, "RADV_PERFTEST")?;
    Ok(TuningApplyResult {
        success: true,
        message: format!(
            "RADV_PERFTEST auf '{}' gesetzt (wirkt nach Neustart/Re-Login)",
            if new_val.is_empty() { "(entfernt)" } else { &new_val }
        ),
        new_value: if new_val.is_empty() { "(nicht gesetzt)".into() } else { new_val },
    })
}

// ─── GPU Overclock Helpers ────────────────────────────────────

fn find_hwmon(card: &str) -> Option<String> {
    let hwmon_dir = format!("/sys/class/drm/{}/device/hwmon", card);
    let entry = fs::read_dir(&hwmon_dir).ok()?.next()?.ok()?;
    Some(entry.path().to_string_lossy().into_owned())
}

fn parse_od_clk_voltage(card: &str) -> (GpuClockInfo, GpuVoltageInfo) {
    let path = format!("/sys/class/drm/{}/device/pp_od_clk_voltage", card);
    let content = read_sys(&path);

    let mut clocks = GpuClockInfo::default();
    let mut voltage = GpuVoltageInfo::default();

    let mut section = "";
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("OD_SCLK:") {
            section = "sclk";
            continue;
        } else if trimmed.starts_with("OD_MCLK:") {
            section = "mclk";
            continue;
        } else if trimmed.starts_with("OD_VDDGFX_OFFSET:") {
            section = "volt";
            continue;
        } else if trimmed.starts_with("OD_RANGE:") {
            section = "range";
            continue;
        }

        match section {
            "sclk" => {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let mhz: u32 = parse_unit(parts[1]);
                    if parts[0].starts_with("0:") || parts[0] == "0:" {
                        clocks.sclk_min = mhz;
                    } else {
                        clocks.sclk_max = mhz;
                    }
                }
            }
            "mclk" => {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let mhz: u32 = parse_unit(parts[1]);
                    if !parts[0].starts_with("0:") {
                        clocks.mclk_max = mhz;
                    }
                }
            }
            "volt" => {
                voltage.offset = parse_unit(trimmed);
                section = "";
            }
            "range" => {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 3 {
                    let key = parts[0].trim_end_matches(':');
                    let min_val: i64 = parse_unit(parts[1]);
                    let max_val: i64 = parse_unit(parts[2]);
                    match key {
                        "SCLK" => {
                            clocks.sclk_range_min = u32::try_from(min_val).unwrap_or_default();
                            clocks.sclk_range_max = u32::try_from(max_val).unwrap_or_default();
                        }
                        "MCLK" => {
                            clocks.mclk_range_min = u32::try_from(min_val).unwrap_or_default();
                            clocks.mclk_range_max = u32::try_from(max_val).unwrap_or_default();
                        }
                        "VDDGFX_OFFSET" => {
                            voltage.min = i32::try_from(min_val).unwrap_or_default();
                            voltage.max = i32::try_from(max_val).unwrap_or_default();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    (clocks, voltage)
}

fn parse_active_clock(card: &str, file: &str) -> u32 {
    let path = format!("/sys/class/drm/{}/device/{}", card, file);
    let content = read_sys(&path);
    for line in content.lines() {
        if line.contains('*') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                return parse_unit(parts[1]);
            }
        }
    }
    0
}

// ─── GPU OC Commands ──────────────────────────────────────────

#[command]
pub async fn get_gpu_oc_status() -> Result<GpuOcStatus, String> {
    tokio::task::spawn_blocking(|| {
        let Some(card) = find_amd_gpu_card() else {
            return Ok(GpuOcStatus {
                gpu_name: "Keine AMD GPU".into(),
                ..GpuOcStatus::default()
            })
        };

    let od_path = format!("/sys/class/drm/{}/device/pp_od_clk_voltage", card);
    let available = Path::new(&od_path).exists();

    let hwmon = find_hwmon(&card).unwrap_or_default();

    // GPU name from lspci
    let gpu_name = {
        let out = std::process::Command::new("lspci")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        out.lines()
            .find(|l| l.contains("VGA")).map_or_else(|| "AMD GPU".into(), |l| l.rsplit(':').next().unwrap_or(l).trim().to_string())
    };

    // VRAM
    let vram_bytes: u64 = read_sys(&format!(
        "/sys/class/drm/{}/device/mem_info_vram_total",
        card
    ))
    .parse()
    .unwrap_or_default();
    let vram_mb = vram_bytes / 1024 / 1024;

    // OD clocks + voltage
    let (mut clocks, voltage) = parse_od_clk_voltage(&card);

    // Current active clocks
    clocks.current_sclk_mhz = parse_active_clock(&card, "pp_dpm_sclk");
    clocks.current_mclk_mhz = parse_active_clock(&card, "pp_dpm_mclk");

    // Power
    let power = GpuPowerInfo {
        cap_w: read_hwmon_watts(&hwmon, "power1_cap"),
        default_w: read_hwmon_watts(&hwmon, "power1_cap_default"),
        max_w: read_hwmon_watts(&hwmon, "power1_cap_max"),
        current_w: read_hwmon_watts(&hwmon, "power1_average"),
    };

    // Temps
    let temps = GpuTempInfo {
        edge: read_hwmon_temp(&hwmon, "temp1_input"),
        junction: read_hwmon_temp(&hwmon, "temp2_input"),
        mem: read_hwmon_temp(&hwmon, "temp3_input"),
    };

    // Fan
    let fan_enable: u32 = read_sys(&format!("{}/pwm1_enable", hwmon))
        .parse()
        .unwrap_or(2);
    let fan = GpuFanInfo {
        rpm: read_sys(&format!("{}/fan1_input", hwmon)).parse().unwrap_or_default(),
        max_rpm: read_sys(&format!("{}/fan1_max", hwmon)).parse().unwrap_or(3200),
        pwm: read_sys(&format!("{}/pwm1", hwmon)).parse().unwrap_or_default(),
        mode: FanMode::from_pwm_enable(fan_enable),
    };

    // GPU busy
    let gpu_busy_percent = read_sys(&format!("/sys/class/drm/{}/device/gpu_busy_percent", card))
        .parse()
        .unwrap_or_default();

    Ok(GpuOcStatus {
        available,
        gpu_name,
        vram_mb,
        clocks,
        voltage,
        power,
        temps,
        fan,
        gpu_busy_percent,
    })
    }).await.map_err(|e| format!("spawn_blocking: {}", e))?
}

#[command(rename_all = "snake_case")]
pub async fn apply_gpu_oc(
    sclk_max: Option<u32>,
    mclk_max: Option<u32>,
    voltage_offset: Option<i32>,
    power_cap_w: Option<u32>,
    fan_mode: Option<FanMode>,
    fan_pwm: Option<u32>,
) -> Result<TuningApplyResult, String> {
    let card = find_amd_gpu_card().ok_or("Keine AMD GPU gefunden")?;
    let od_path = format!("/sys/class/drm/{}/device/pp_od_clk_voltage", card);
    let hwmon = find_hwmon(&card).ok_or("Kein hwmon gefunden")?;

    let mut changes = Vec::new();
    let mut writes: Vec<serde_json::Value> = Vec::new();

    // Collect clock / voltage changes via pp_od_clk_voltage
    let mut need_commit = false;

    if let Some(sclk) = sclk_max {
        writes.push(serde_json::json!({"path": &od_path, "value": format!("s 1 {}", sclk)}));
        need_commit = true;
        changes.push(format!("GPU Clock → {} MHz", sclk));
    }

    if let Some(mclk) = mclk_max {
        writes.push(serde_json::json!({"path": &od_path, "value": format!("m 1 {}", mclk)}));
        need_commit = true;
        changes.push(format!("VRAM Clock → {} MHz", mclk));
    }

    if let Some(vo) = voltage_offset {
        writes.push(serde_json::json!({"path": &od_path, "value": format!("vo {}", vo)}));
        need_commit = true;
        changes.push(format!("Voltage Offset → {} mV", vo));
    }

    if need_commit {
        writes.push(serde_json::json!({"path": &od_path, "value": "c"}));
    }

    // Power cap (in microwatts)
    if let Some(watts) = power_cap_w {
        let microwatts = u64::from(watts) * 1_000_000;
        let power_path = format!("{}/power1_cap", hwmon);
        writes.push(serde_json::json!({"path": power_path, "value": microwatts.to_string()}));
        changes.push(format!("Power Limit → {} W", watts));
    }

    // Fan control
    if let Some(mode) = fan_mode {
        let pwm_enable_path = format!("{}/pwm1_enable", hwmon);
        writes.push(serde_json::json!({"path": &pwm_enable_path, "value": mode.pwm_enable_str()}));
        match mode {
            FanMode::Manual => {
                if let Some(pwm) = fan_pwm {
                    let pwm_path = format!("{}/pwm1", hwmon);
                    writes.push(serde_json::json!({"path": pwm_path, "value": pwm.to_string()}));
                    changes.push(format!("Lüfter → Manuell ({}%)", pwm * 100 / 255));
                } else {
                    changes.push("Lüfter → Manuell".into());
                }
            }
            FanMode::Auto => {
                changes.push("Lüfter → Automatisch".into());
            }
        }
    } else if let Some(pwm) = fan_pwm {
        let pwm_path = format!("{}/pwm1", hwmon);
        writes.push(serde_json::json!({"path": pwm_path, "value": pwm.to_string()}));
        changes.push(format!("Lüfter-PWM → {}%", pwm * 100 / 255));
    }

    // Execute all writes + auto-save profile in a single privileged call
    if !writes.is_empty() {
        // Read current status to build profile (reads are unprivileged)
        let pre_status = get_gpu_oc_status_inner(&card, &hwmon);

        // Build profile from intended values (use param if set, otherwise current)
        let profile = GpuOcProfile {
            sclk_max: sclk_max.unwrap_or(pre_status.clocks.sclk_max),
            mclk_max: mclk_max.unwrap_or(pre_status.clocks.mclk_max),
            voltage_offset: voltage_offset.unwrap_or(pre_status.voltage.offset),
            power_cap_w: power_cap_w.unwrap_or(pre_status.power.cap_w),
            performance_level: read_gpu_power_level(),
            power_profile_index: get_current_profile_index(&card),
            fan_mode: fan_mode.unwrap_or(pre_status.fan.mode),
            fan_pwm: fan_pwm.unwrap_or(pre_status.fan.pwm),
        };
        let profile_json = serde_json::to_string_pretty(&profile).unwrap_or_default();
        writes.push(serde_json::json!({"path": OC_PROFILE_PATH, "value": profile_json}));

        let json = serde_json::Value::Array(writes).to_string();
        let res = run_sysfs_batch(&json);
        require_ok(&res, "GPU OC anwenden")?;
    }

    let msg = if changes.is_empty() {
        "Keine Änderungen".into()
    } else {
        changes.join(", ")
    };

    Ok(TuningApplyResult {
        success: true,
        message: msg.clone(),
        new_value: msg,
    })
}

#[command]
pub async fn reset_gpu_oc() -> Result<TuningApplyResult, String> {
    let card = find_amd_gpu_card().ok_or("Keine AMD GPU gefunden")?;
    let od_path = format!("/sys/class/drm/{}/device/pp_od_clk_voltage", card);
    let hwmon = find_hwmon(&card).ok_or("Kein hwmon gefunden")?;

    // Collect all reset writes
    let mut writes: Vec<serde_json::Value> = vec![
        serde_json::json!({"path": &od_path, "value": "r"}),
        serde_json::json!({"path": &od_path, "value": "c"}),
    ];

    // Reset power cap to default
    let default_power = read_sys(&format!("{}/power1_cap_default", hwmon));
    if !default_power.is_empty() {
        writes.push(
            serde_json::json!({"path": format!("{}/power1_cap", hwmon), "value": &default_power}),
        );
    }

    // Reset fan to auto
    writes.push(serde_json::json!({"path": format!("{}/pwm1_enable", hwmon), "value": FanMode::Auto.pwm_enable_str()}));

    // Delete saved profile
    writes.push(serde_json::json!({"path": OC_PROFILE_PATH, "value": "__DELETE__"}));

    let json = serde_json::Value::Array(writes).to_string();
    let res = run_sysfs_batch(&json);
    require_ok(&res, "GPU Reset")?;

    Ok(TuningApplyResult {
        success: true,
        message: "GPU auf Werkseinstellungen zurückgesetzt".into(),
        new_value: "default".into(),
    })
}

// ─── Internal helper for reading OC status ────────────────────

fn get_gpu_oc_status_inner(card: &str, hwmon: &str) -> GpuOcStatus {
    let (clocks, voltage) = parse_od_clk_voltage(card);
    let power_cap_w = read_hwmon_watts(hwmon, "power1_cap");
    let fan_pwm: u32 = read_sys(&format!("{}/pwm1", hwmon)).parse().unwrap_or_default();
    let fan_enable: u32 = read_sys(&format!("{}/pwm1_enable", hwmon))
        .parse()
        .unwrap_or(2);
    GpuOcStatus {
        available: true,
        clocks,
        voltage,
        power: GpuPowerInfo { cap_w: power_cap_w, ..Default::default() },
        fan: GpuFanInfo { pwm: fan_pwm, mode: FanMode::from_pwm_enable(fan_enable), ..Default::default() },
        ..Default::default()
    }
}

fn get_current_profile_index(card: &str) -> u32 {
    let path = format!("/sys/class/drm/{}/device/pp_power_profile_mode", card);
    let content = read_sys(&path);
    for line in content.lines().skip(1) {
        if line.contains('*') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(idx) = parts.first() {
                return idx.parse().unwrap_or_default();
            }
        }
    }
    0
}

// ─── GPU OC Boot Service ──────────────────────────────────────

#[command]
pub async fn get_gpu_oc_profile() -> Result<Option<GpuOcProfile>, String> {
    Ok(load_oc_profile())
}

#[command]
pub async fn install_gpu_oc_service() -> Result<TuningApplyResult, String> {
    let _profile =
        load_oc_profile().ok_or("Kein GPU-Profil gespeichert. Erst OC-Werte anwenden.")?;
    // Verify GPU exists at install time, but script detects dynamically at boot
    let _card = find_amd_gpu_card().ok_or("Keine AMD GPU gefunden")?;

    // Fully dynamic boot script — detects card/hwmon at runtime
    let script = include_str!("../scripts/apply-gpu-oc.sh.tmpl")
        .replace("@@PROFILE_PATH@@", OC_PROFILE_PATH);

    let script_path = "/etc/backsnap/apply-gpu-oc.sh";
    let service_content = format!(
        "[Unit]
Description=Apply Backsnap GPU OC settings at boot
After=systemd-modules-load.service
After=dev-dri-card1.device
Wants=dev-dri-card1.device

[Service]
Type=oneshot
ExecStart=/bin/bash {}
RemainAfterExit=yes

[Install]
WantedBy=graphical.target
",
        script_path
    );

    // Udev rule: re-apply GPU OC after a GPU reset
    let udev_rule = "ACTION==\"change\", SUBSYSTEM==\"drm\", KERNEL==\"card[0-9]*\", \
        ENV{RESET}==\"1\", RUN+=\"/bin/bash /etc/backsnap/apply-gpu-oc.sh\"".to_string();

    // Single privileged batch: mkdir, write script + chmod, write service + udev
    let script_path_str = script_path.to_string();
    let service_path = format!("/etc/systemd/system/{}", OC_APPLY_SERVICE);
    let udev_path = "/etc/udev/rules.d/99-backsnap-gpu-reset.rules".to_string();
    let ops = vec![
        FileOp::Mkdir {
            path: "/etc/backsnap".into(),
        },
        FileOp::Write {
            path: script_path_str.clone(),
            content: script,
        },
        FileOp::Chmod {
            path: script_path_str,
            mode: 0o755,
        },
        FileOp::Write {
            path: service_path,
            content: service_content,
        },
        FileOp::Write {
            path: udev_path,
            content: udev_rule,
        },
    ];
    run_file_ops_batch(&ops).map_err(|e| format!("GPU OC Service installieren: {}", e))?;

    // Enable
    let res = run_privileged("systemctl", &["daemon-reload"]);
    require_ok(&res, "daemon-reload")?;
    let res = run_privileged("systemctl", &["enable", OC_APPLY_SERVICE]);
    require_ok(&res, "Service aktivieren")?;

    // Reload udev rules so the GPU-reset rule takes effect immediately
    let _ = run_privileged("udevadm", &["control", "--reload-rules"]);

    Ok(TuningApplyResult {
        success: true,
        message: "GPU OC Boot-Service + GPU-Reset Udev-Regel installiert und aktiviert".into(),
        new_value: "enabled".into(),
    })
}

#[command]
pub async fn uninstall_gpu_oc_service() -> Result<TuningApplyResult, String> {
    let service_path = format!("/etc/systemd/system/{}", OC_APPLY_SERVICE);
    let _ = run_privileged("systemctl", &["disable", "--now", OC_APPLY_SERVICE]);
    let ops = vec![
        FileOp::Delete { path: service_path },
        FileOp::Delete {
            path: "/etc/backsnap/apply-gpu-oc.sh".into(),
        },
        FileOp::Delete {
            path: "/etc/udev/rules.d/99-backsnap-gpu-reset.rules".into(),
        },
    ];
    let _ = run_file_ops_batch(&ops);
    let _ = run_privileged("systemctl", &["daemon-reload"]);
    let _ = run_privileged("udevadm", &["control", "--reload-rules"]);

    Ok(TuningApplyResult {
        success: true,
        message: "GPU OC Boot-Service deinstalliert".into(),
        new_value: "disabled".into(),
    })
}

#[command]
pub async fn get_gpu_oc_service_status() -> Result<bool, String> {
    Ok(systemctl_is_enabled(OC_APPLY_SERVICE, false))
}
