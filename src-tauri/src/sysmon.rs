//! System monitor — ported from Systor/sysmon.c
//! Reads CPU, RAM, Swap, GPU, temperatures and power from /proc and /sys.

use crate::commands::helpers::read_sys_opt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

/// Initial sleep (ms) for CPU usage delta on first sysmon call.
const SYSMON_INITIAL_SAMPLE_MS: u64 = 100;

// ─── Data Structures ──────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SystemMonitorData {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub swap: SwapInfo,
    pub cpu_sensor: CpuSensor,
    pub gpu: GpuInfo,
    pub load: LoadAvg,
    pub uptime: UptimeInfo,
    pub battery: Option<BatteryInfo>,
    pub extra_power: ExtraPower,
    pub nvme_temps: Vec<NvmeTemp>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CpuInfo {
    pub model: String,
    pub cores: u32,
    pub threads: u32,
    pub usage_percent: f64,
    pub per_core_usage: Vec<f64>,
    pub frequency_mhz: Option<f64>,
    pub architecture: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MemoryInfo {
    pub total_mib: u64,
    pub used_mib: u64,
    pub available_mib: u64,
    pub percent: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SwapInfo {
    pub total_mib: u64,
    pub used_mib: u64,
    pub percent: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CpuSensor {
    pub temp_celsius: Option<f64>,
    pub power_watts: Option<f64>,
    pub power_no_permission: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GpuInfo {
    pub name: String,
    pub temp_celsius: Option<f64>,
    pub power_watts: Option<f64>,
    pub vram_total_mib: Option<u64>,
    pub vram_used_mib: Option<u64>,
    pub gpu_busy_percent: Option<u64>,
    pub gpu_clock_mhz: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LoadAvg {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct UptimeInfo {
    pub seconds: f64,
    pub formatted: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct BatteryInfo {
    pub power_watts: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ExtraPower {
    #[serde(rename = "dram_watts")]
    pub dram: Option<f64>,
    #[serde(rename = "platform_watts")]
    pub platform: Option<f64>,
    #[serde(rename = "total_system_watts")]
    pub total_system: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NvmeTemp {
    pub name: String,
    pub temp_celsius: f64,
}

// ─── Internal CPU State ───────────────────────────────────────

#[derive(Clone, Default)]
struct CpuTimes {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

#[derive(Default)]
struct RaplDomain {
    energy_uj_path: String,
    max_energy_uj_path: String,
    prev_energy_uj: u64,
    prev_time: Option<Instant>,
    no_permission: bool,
}

struct SensorPaths {
    cpu_temp_input: String,
    rapl: RaplDomain,
    rapl_dram: RaplDomain,
    rapl_psys: RaplDomain,
    gpu_power_path: String,
    gpu_temp_path: String,
    gpu_drm_dir: String,
    bat_power_path: String,
    bat_current_path: String,
    bat_voltage_path: String,
    use_nvidia_smi: bool,
}

struct CachedStatic {
    cpu_model: String,
    cores: u32,
    threads: u32,
    gpu_name: String,
}

struct MonitorState {
    prev_cpu_total: CpuTimes,
    prev_cpu_cores: Vec<CpuTimes>,
    sensors: SensorPaths,
    cached: CachedStatic,
}

static STATE: std::sync::OnceLock<Mutex<Option<MonitorState>>> = std::sync::OnceLock::new();

fn get_state() -> &'static Mutex<Option<MonitorState>> {
    STATE.get_or_init(|| Mutex::new(None))
}

// ─── Sensor Detection (mirrors sysmon.c logic) ───────────────

fn read_u64(path: &str) -> Option<u64> {
    read_sys_opt(path)?.parse().ok()
}

fn read_i64(path: &str) -> Option<i64> {
    read_sys_opt(path)?.parse().ok()
}

fn file_readable(path: &str) -> bool {
    Path::new(path).exists() && fs::read_to_string(path).is_ok()
}

fn detect_cpu_temp() -> String {
    let hwmon = Path::new("/sys/class/hwmon");
    if !hwmon.exists() {
        return String::new();
    }

    let Ok(entries) = fs::read_dir(hwmon) else {
        return String::new();
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("hwmon") {
            continue;
        }
        let base = entry.path();
        let sensor_name = read_sys_opt(&base.join("name").to_string_lossy());
        match sensor_name.as_deref() {
            Some("coretemp" | "k10temp" | "zenpower") => {}
            _ => continue,
        }

        let mut best_path = String::new();
        let mut best_score = -1i32;

        if let Ok(files) = fs::read_dir(&base) {
            for f in files.flatten() {
                let fname = f.file_name().to_string_lossy().into_owned();
                if let Some(rest) = fname.strip_prefix("temp") {
                    if let Some(idx_str) = rest.strip_suffix("_input") {
                        if let Ok(idx) = idx_str.parse::<i32>() {
                            let input_path = base.join(&fname).to_string_lossy().into_owned();
                            if !file_readable(&input_path) {
                                continue;
                            }

                            let mut score = 0i32;
                            let label_path = base
                                .join(format!("temp{}_label", idx))
                                .to_string_lossy()
                                .to_string();
                            if let Some(label) = read_sys_opt(&label_path) {
                                if label.contains("Package")
                                    || label.contains("Tdie")
                                    || label.contains("Tctl")
                                {
                                    score = 2;
                                } else {
                                    score = 1;
                                }
                            }

                            if score > best_score {
                                best_score = score;
                                best_path = input_path;
                            }
                        }
                    }
                }
            }
        }

        if !best_path.is_empty() {
            return best_path;
        }
    }

    String::new()
}

fn detect_rapl() -> (RaplDomain, RaplDomain, RaplDomain) {
    let mut cpu = RaplDomain::default();
    let mut dram = RaplDomain::default();
    let mut psys = RaplDomain::default();
    let pc = Path::new("/sys/class/powercap");
    if !pc.exists() {
        return (cpu, dram, psys);
    }

    if let Ok(entries) = fs::read_dir(pc) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("intel-rapl:")
                && !name.starts_with("amd-rapl:")
                && !name.starts_with("rapl:")
            {
                continue;
            }

            let base = entry.path();
            let energy = base.join("energy_uj");
            if !energy.exists() {
                continue;
            }

            // Check the domain name to classify
            let domain_name = read_sys_opt(&base.join("name").to_string_lossy())
                .unwrap_or_default()
                .to_lowercase();

            let is_subdomain = name.matches(':').count() >= 2;

            if domain_name == "dram" || domain_name.contains("dram") {
                dram.energy_uj_path = energy.to_string_lossy().into_owned();
                let max_e = base.join("max_energy_range_uj");
                if max_e.exists() {
                    dram.max_energy_uj_path = max_e.to_string_lossy().into_owned();
                }
            } else if domain_name == "psys" || domain_name.contains("psys") {
                psys.energy_uj_path = energy.to_string_lossy().into_owned();
                let max_e = base.join("max_energy_range_uj");
                if max_e.exists() {
                    psys.max_energy_uj_path = max_e.to_string_lossy().into_owned();
                }
            } else if !is_subdomain && cpu.energy_uj_path.is_empty() {
                // Top-level package domain (CPU)
                cpu.energy_uj_path = energy.to_string_lossy().into_owned();
                let max_e = base.join("max_energy_range_uj");
                if max_e.exists() {
                    cpu.max_energy_uj_path = max_e.to_string_lossy().into_owned();
                }
            }
        }
    }

    (cpu, dram, psys)
}

fn detect_amdgpu() -> (String, String, String, String) {
    // Returns: (hwmon_base, temp_path, power_path, drm_dir)
    let hwmon = Path::new("/sys/class/hwmon");
    if !hwmon.exists() {
        return (String::new(), String::new(), String::new(), String::new());
    }

    if let Ok(entries) = fs::read_dir(hwmon) {
        for entry in entries.flatten() {
            let base = entry.path();
            let sensor_name = read_sys_opt(&base.join("name").to_string_lossy());
            if sensor_name.as_deref() != Some("amdgpu") {
                continue;
            }

            let temp = base.join("temp1_input").to_string_lossy().into_owned();
            let temp_path = if file_readable(&temp) {
                temp
            } else {
                String::new()
            };

            let pavg = base.join("power1_average").to_string_lossy().into_owned();
            let pinp = base.join("power1_input").to_string_lossy().into_owned();
            let power_path = if file_readable(&pavg) {
                pavg
            } else if file_readable(&pinp) {
                pinp
            } else {
                String::new()
            };

            // Find DRM card directory for VRAM and busy%
            let hwmon_base = base.to_string_lossy().into_owned();
            let drm_dir = find_drm_card(&base);

            return (hwmon_base, temp_path, power_path, drm_dir);
        }
    }

    (String::new(), String::new(), String::new(), String::new())
}

fn find_drm_card(hwmon_path: &Path) -> String {
    // Traverse: hwmon -> device -> drm -> cardN
    let device = hwmon_path.join("device");
    let drm = device.join("drm");
    if !drm.exists() {
        return String::new();
    }
    if let Ok(entries) = fs::read_dir(&drm) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("card") && !name.contains('-') {
                let card_path = format!("/sys/class/drm/{}", name);
                let device_dir = format!("{}/device", card_path);
                if Path::new(&device_dir).exists() {
                    return device_dir;
                }
            }
        }
    }
    // Fallback: use device directly
    let dev_str = device.to_string_lossy().into_owned();
    if Path::new(&dev_str).exists() {
        return dev_str;
    }
    String::new()
}

fn detect_battery() -> (String, String, String) {
    // Returns: (power_path, current_path, voltage_path)
    let ps = Path::new("/sys/class/power_supply");
    if !ps.exists() {
        return (String::new(), String::new(), String::new());
    }

    if let Ok(entries) = fs::read_dir(ps) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("BAT") {
                continue;
            }
            let base = entry.path();

            let pnow = base.join("power_now").to_string_lossy().into_owned();
            if file_readable(&pnow) {
                return (pnow, String::new(), String::new());
            }

            let cnow = base.join("current_now").to_string_lossy().into_owned();
            let vnow = base.join("voltage_now").to_string_lossy().into_owned();
            if file_readable(&cnow) && file_readable(&vnow) {
                return (String::new(), cnow, vnow);
            }
        }
    }

    (String::new(), String::new(), String::new())
}

fn detect_sensors_all() -> SensorPaths {
    let cpu_temp_input = detect_cpu_temp();
    let (rapl, rapl_dram, rapl_psys) = detect_rapl();
    let (_gpu_hwmon_base, gpu_temp_path, gpu_power_path, gpu_drm_dir) = detect_amdgpu();
    let use_nvidia_smi = gpu_temp_path.is_empty() && gpu_power_path.is_empty();
    let (bat_power_path, bat_current_path, bat_voltage_path) = detect_battery();

    SensorPaths {
        cpu_temp_input,
        rapl,
        rapl_dram,
        rapl_psys,
        gpu_power_path,
        gpu_temp_path,
        gpu_drm_dir,
        bat_power_path,
        bat_current_path,
        bat_voltage_path,
        use_nvidia_smi,
    }
}

/// Detect NVMe drive temperatures from /sys/class/hwmon.
/// NVMe controllers expose hwmon sensors with name "nvme" or similar.
fn detect_nvme_temps() -> Vec<NvmeTemp> {
    let hwmon = Path::new("/sys/class/hwmon");
    if !hwmon.exists() {
        return Vec::new();
    }

    let mut temps = Vec::new();
    if let Ok(entries) = fs::read_dir(hwmon) {
        for entry in entries.flatten() {
            let base = entry.path();
            let sensor_name = read_sys_opt(&base.join("name").to_string_lossy());
            match sensor_name.as_deref() {
                Some(n) if n.starts_with("nvme") => {}
                _ => continue,
            }

            // Try temp1_input (composite temp)
            let temp_path = base.join("temp1_input").to_string_lossy().into_owned();
            if let Some(mdeg) = read_i64(&temp_path) {
                // Try to get the device model name
                let device_link = base.join("device");
                let model = if device_link.exists() {
                    let model_path = device_link.join("model");
                    read_sys_opt(&model_path.to_string_lossy()).unwrap_or_else(|| {
                        sensor_name.clone().unwrap_or_else(|| "NVMe".to_string())
                    })
                } else {
                    sensor_name.unwrap_or_else(|| "NVMe".to_string())
                };
                temps.push(NvmeTemp {
                    name: model.trim().to_string(),
                    #[allow(clippy::cast_precision_loss)] // millidegree sensor value, well within f64 range
                    temp_celsius: mdeg as f64 / 1000.0,
                });
            }
        }
    }
    temps
}

/// Read a RAPL domain's energy delta and compute instantaneous power in watts.
fn read_rapl_domain_power(domain: &mut RaplDomain) -> Option<f64> {
    if domain.energy_uj_path.is_empty() {
        return None;
    }
    let content = match fs::read_to_string(&domain.energy_uj_path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                domain.no_permission = true;
            }
            return None;
        }
    };
    let energy_uj: u64 = content.trim().parse().ok()?;
    let now = Instant::now();

    if let Some(prev_time) = domain.prev_time {
        let dt = now.duration_since(prev_time).as_secs_f64();
        if dt > 0.001 {
            let delta = if energy_uj >= domain.prev_energy_uj {
                energy_uj - domain.prev_energy_uj
            } else {
                let max = if domain.max_energy_uj_path.is_empty() {
                    0
                } else {
                    read_u64(&domain.max_energy_uj_path).unwrap_or_default()
                };
                if max > domain.prev_energy_uj {
                    (max - domain.prev_energy_uj) + energy_uj
                } else {
                    energy_uj
                }
            };
            // CAST-SAFETY: delta is a µJ energy counter diff; f64 has sufficient precision
            #[allow(clippy::cast_precision_loss)]
            let watts = (delta as f64 / 1e6) / dt;
            domain.prev_energy_uj = energy_uj;
            domain.prev_time = Some(now);
            return Some(watts.max(0.0));
        }
    }

    domain.prev_energy_uj = energy_uj;
    domain.prev_time = Some(now);
    None
}

// ─── Reading Values ───────────────────────────────────────────

fn read_cpu_times_all() -> Option<(CpuTimes, Vec<CpuTimes>)> {
    let content = fs::read_to_string("/proc/stat").ok()?;
    let mut total = CpuTimes::default();
    let mut cores = Vec::new();

    for line in content.lines() {
        if line.starts_with("cpu ") || line.starts_with("cpu") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 9 {
                continue;
            }

            let times = CpuTimes {
                user: parts[1].parse().unwrap_or_default(),
                nice: parts[2].parse().unwrap_or_default(),
                system: parts[3].parse().unwrap_or_default(),
                idle: parts[4].parse().unwrap_or_default(),
                iowait: parts[5].parse().unwrap_or_default(),
                irq: parts[6].parse().unwrap_or_default(),
                softirq: parts[7].parse().unwrap_or_default(),
                steal: parts[8].parse().unwrap_or_default(),
            };

            if parts[0] == "cpu" {
                total = times;
            } else if parts[0].starts_with("cpu") {
                cores.push(times);
            }
        }
    }

    Some((total, cores))
}

fn cpu_usage(prev: &CpuTimes, cur: &CpuTimes) -> f64 {
    let prev_idle = prev.idle + prev.iowait;
    let cur_idle = cur.idle + cur.iowait;

    let prev_total = prev.user
        + prev.nice
        + prev.system
        + prev.idle
        + prev.iowait
        + prev.irq
        + prev.softirq
        + prev.steal;
    let cur_total = cur.user
        + cur.nice
        + cur.system
        + cur.idle
        + cur.iowait
        + cur.irq
        + cur.softirq
        + cur.steal;

    let total_d = cur_total.saturating_sub(prev_total);
    let idle_d = cur_idle.saturating_sub(prev_idle);

    if total_d == 0 {
        return 0.0;
    }
    // CAST-SAFETY: CPU jiffies are small enough that f64 precision loss is negligible
    #[allow(clippy::cast_precision_loss)]
    let used = (total_d - idle_d) as f64 * 100.0 / total_d as f64;
    used.clamp(0.0, 100.0)
}

// CAST-SAFETY: memory sizes in KiB from /proc/meminfo; f64 precision loss is negligible for percentages
#[allow(clippy::cast_precision_loss)]
fn read_meminfo() -> (MemoryInfo, SwapInfo) {
    let mut mem_total = 0u64;
    let mut mem_avail = 0u64;
    let mut swap_total = 0u64;
    let mut swap_free = 0u64;

    if let Ok(content) = fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let val: u64 = parts[1].parse().unwrap_or_default();
            match parts[0] {
                "MemTotal:" => mem_total = val,
                "MemAvailable:" => mem_avail = val,
                "SwapTotal:" => swap_total = val,
                "SwapFree:" => swap_free = val,
                _ => {}
            }
        }
    }

    let mem_used = mem_total.saturating_sub(mem_avail);
    let swap_used = swap_total.saturating_sub(swap_free);

    (
        MemoryInfo {
            total_mib: mem_total / 1024,
            used_mib: mem_used / 1024,
            available_mib: mem_avail / 1024,
            percent: if mem_total > 0 {
                mem_used as f64 * 100.0 / mem_total as f64
            } else {
                0.0
            },
        },
        SwapInfo {
            total_mib: swap_total / 1024,
            used_mib: swap_used / 1024,
            percent: if swap_total > 0 {
                swap_used as f64 * 100.0 / swap_total as f64
            } else {
                0.0
            },
        },
    )
}

fn get_cpu_model() -> String {
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        for line in content.lines() {
            if line.starts_with("model name") {
                if let Some(val) = line.split(':').nth(1) {
                    return val.trim().to_string();
                }
            }
        }
    }
    "Unknown CPU".to_string()
}

fn get_cpu_arch() -> String {
    std::process::Command::new("uname")
        .arg("-m")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "unknown".to_string(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        )
}

fn get_cpu_count() -> (u32, u32) {
    // Returns (physical cores, logical threads)
    let mut threads = 0u32;
    let mut core_ids = std::collections::HashSet::new();
    let mut physical_ids = std::collections::HashSet::new();
    let mut current_physical = String::new();
    let mut current_core;

    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        for line in content.lines() {
            if line.starts_with("processor") {
                threads += 1;
            } else if line.starts_with("physical id") {
                if let Some(val) = line.split(':').nth(1) {
                    current_physical = val.trim().to_string();
                    physical_ids.insert(current_physical.clone());
                }
            } else if line.starts_with("core id") {
                if let Some(val) = line.split(':').nth(1) {
                    current_core = val.trim().to_string();
                    core_ids.insert(format!("{}:{}", current_physical, current_core));
                }
            }
        }
    }

    let cores = if core_ids.is_empty() {
        threads
    } else {
        // CAST-SAFETY: CPU physical core count always fits u32
        #[allow(clippy::cast_possible_truncation)]
        {
            core_ids.len() as u32
        }
    };

    (cores, threads)
}

fn get_cpu_freq() -> Option<f64> {
    // Try scaling_cur_freq first (more accurate)
    let cpufreq = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq");
    if let Some(khz) = read_u64(&cpufreq.to_string_lossy()) {
        // CAST-SAFETY: CPU frequency in kHz; f64 precision is more than sufficient
        #[allow(clippy::cast_precision_loss)]
        let freq = khz as f64 / 1000.0;
        return Some(freq);
    }
    // Fallback: average from /proc/cpuinfo
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        let mut total = 0.0f64;
        let mut count = 0u32;
        for line in content.lines() {
            if line.starts_with("cpu MHz") {
                if let Some(val) = line.split(':').nth(1) {
                    if let Ok(mhz) = val.trim().parse::<f64>() {
                        total += mhz;
                        count += 1;
                    }
                }
            }
        }
        if count > 0 {
            return Some(total / f64::from(count));
        }
    }
    None
}

fn read_gpu_name(drm_dir: &str) -> String {
    if drm_dir.is_empty() {
        return String::new();
    }

    // Try marketing name via /sys/.../product_name or parse from uevent
    let product = format!("{}/product_name", drm_dir);
    if let Some(name) = read_sys_opt(&product) {
        return name;
    }

    // Parse vendor/device from uevent
    let uevent = format!("{}/uevent", drm_dir);
    if let Ok(content) = fs::read_to_string(&uevent) {
        for line in content.lines() {
            if let Some(stripped) = line.strip_prefix("PCI_ID=") {
                return format!("GPU ({})", stripped);
            }
        }
    }

    // Fallback: lspci-style
    let lspci = std::process::Command::new("lspci")
        .args(["-mm", "-d", "::0300"])
        .output();
    if let Ok(o) = lspci {
        if o.status.success() {
            let out = String::from_utf8_lossy(&o.stdout);
            if let Some(line) = out.lines().next() {
                // Extract device name from lspci output
                let parts: Vec<&str> = line.split('"').collect();
                if parts.len() >= 6 {
                    return parts[5].to_string();
                }
            }
        }
    }

    "Unknown GPU".to_string()
}

fn read_gpu_vram(drm_dir: &str) -> (Option<u64>, Option<u64>) {
    if drm_dir.is_empty() {
        return (None, None);
    }

    // amdgpu: mem_info_vram_total / mem_info_vram_used (bytes)
    let total_path = format!("{}/mem_info_vram_total", drm_dir);
    let used_path = format!("{}/mem_info_vram_used", drm_dir);

    let total = read_u64(&total_path).map(|b| b / (1024 * 1024));
    let used = read_u64(&used_path).map(|b| b / (1024 * 1024));

    (total, used)
}

fn read_gpu_busy(drm_dir: &str) -> Option<u64> {
    if drm_dir.is_empty() {
        return None;
    }
    let busy_path = format!("{}/gpu_busy_percent", drm_dir);
    read_u64(&busy_path)
}

fn read_gpu_clock(drm_dir: &str) -> Option<u32> {
    if drm_dir.is_empty() {
        return None;
    }
    let sclk_path = format!("{}/pp_dpm_sclk", drm_dir);
    if let Ok(content) = fs::read_to_string(&sclk_path) {
        for line in content.lines() {
            if line.contains('*') {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    // e.g. "1: 1900Mhz *" -> parse "1900Mhz" -> 1900
                    let s = parts[1].trim_end_matches(|c: char| !c.is_ascii_digit());
                    if let Ok(mhz) = s.parse::<u32>() {
                        return Some(mhz);
                    }
                }
            }
        }
    }
    None
}

/// Query nvidia-smi for GPU info (temp, power, utilization, vram, name)
/// Returns (name, temp_c, power_w, vram_total_mib, vram_used_mib, gpu_busy_pct)
type NvidiaSmiInfo = (
    String,
    Option<f64>,
    Option<f64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u32>,
);

fn read_nvidia_smi() -> Option<NvidiaSmiInfo> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,temperature.gpu,power.draw,memory.total,memory.used,utilization.gpu,clocks.current.graphics",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = line.trim().split(", ").collect();
    if parts.len() < 6 {
        return None;
    }
    let name = parts[0].to_string();
    let temp = parts[1].parse::<f64>().ok();
    let power = parts[2].parse::<f64>().ok();
    let vram_total = parts[3].parse::<u64>().ok();
    let vram_used = parts[4].parse::<u64>().ok();
    let gpu_busy = parts[5].parse::<u64>().ok();
    let clock = parts.get(6).and_then(|s| s.parse::<u32>().ok());
    Some((name, temp, power, vram_total, vram_used, gpu_busy, clock))
}

// CAST-SAFETY: µW / µA / µV sensor values from sysfs; f64 precision loss is negligible
#[allow(clippy::cast_precision_loss)]
fn read_battery_from_paths(
    power_path: &str,
    current_path: &str,
    voltage_path: &str,
) -> Option<BatteryInfo> {
    if !power_path.is_empty() {
        if let Some(uw) = read_i64(power_path) {
            return Some(BatteryInfo {
                power_watts: uw as f64 / 1e6,
            });
        }
    } else if !current_path.is_empty() && !voltage_path.is_empty() {
        if let (Some(ua), Some(uv)) = (read_i64(current_path), read_i64(voltage_path)) {
            return Some(BatteryInfo {
                power_watts: (ua as f64 * uv as f64) / 1e12,
            });
        }
    }
    None
}

// ─── Main Read Function ──────────────────────────────────────

pub(crate) fn read_system_monitor() -> SystemMonitorData {
    let state_mutex = get_state();
    let Ok(mut guard) = state_mutex.lock() else {
        return SystemMonitorData::default();
    };

    // Initialize on first call — detect sensors and cache static info
    if guard.is_none() {
        let sensors = detect_sensors_all();
        let (total, cores) = read_cpu_times_all().unwrap_or_default();
        let (cores_count, threads) = get_cpu_count();
        let gpu_name = read_gpu_name(&sensors.gpu_drm_dir);
        let cpu_model = get_cpu_model();

        // Brief sleep + second read to get a meaningful initial CPU usage delta
        // (without this, the first call always returns 0%)
        std::thread::sleep(std::time::Duration::from_millis(SYSMON_INITIAL_SAMPLE_MS));
        let (total2, cores2) = read_cpu_times_all().unwrap_or_default();
        let initial_usage = cpu_usage(&total, &total2);
        let initial_per_core: Vec<f64> = cores
            .iter()
            .zip(cores2.iter())
            .map(|(prev, cur)| cpu_usage(prev, cur))
            .collect();

        *guard = Some(MonitorState {
            prev_cpu_total: total2,
            prev_cpu_cores: cores2,
            sensors,
            cached: CachedStatic {
                cpu_model: cpu_model.clone(),
                cores: cores_count,
                threads,
                gpu_name: gpu_name.clone(),
            },
        });

        // Release lock before returning
        drop(guard);

        let (memory, swap) = read_meminfo();
        return SystemMonitorData {
            cpu: CpuInfo {
                model: cpu_model,
                cores: cores_count,
                threads,
                usage_percent: initial_usage,
                per_core_usage: initial_per_core,
                frequency_mhz: get_cpu_freq(),
                architecture: get_cpu_arch(),
            },
            memory,
            swap,
            cpu_sensor: CpuSensor::default(),
            gpu: GpuInfo {
                name: gpu_name,
                ..Default::default()
            },
            load: read_loadavg(),
            uptime: read_uptime_info(),
            battery: None,
            extra_power: ExtraPower::default(),
            nvme_temps: detect_nvme_temps(),
        };
    }

    let Some(state) = guard.as_mut() else {
        return SystemMonitorData::default();
    };

    // Read CPU delta (requires prev state)
    let (cur_total, cur_cores) = read_cpu_times_all().unwrap_or_default();
    let cpu_pct = cpu_usage(&state.prev_cpu_total, &cur_total);

    let per_core: Vec<f64> = state
        .prev_cpu_cores
        .iter()
        .zip(cur_cores.iter())
        .map(|(prev, cur)| cpu_usage(prev, cur))
        .collect();

    state.prev_cpu_total = cur_total;
    state.prev_cpu_cores = cur_cores;

    // RAPL power (needs mutable state for prev_energy)
    let cpu_power = read_rapl_domain_power(&mut state.sensors.rapl);
    let rapl_no_perm = state.sensors.rapl.no_permission;
    let dram_power = read_rapl_domain_power(&mut state.sensors.rapl_dram);
    let psys_power = read_rapl_domain_power(&mut state.sensors.rapl_psys);

    // Copy out what we need from state, then DROP the lock
    let cpu_temp_path = state.sensors.cpu_temp_input.clone();
    let gpu_temp_path = state.sensors.gpu_temp_path.clone();
    let gpu_power_path = state.sensors.gpu_power_path.clone();
    let gpu_drm_dir = state.sensors.gpu_drm_dir.clone();
    let use_nvidia = state.sensors.use_nvidia_smi;
    let cached = CachedStatic {
        cpu_model: state.cached.cpu_model.clone(),
        cores: state.cached.cores,
        threads: state.cached.threads,
        gpu_name: state.cached.gpu_name.clone(),
    };
    let bat_sensors_clone = (
        state.sensors.bat_power_path.clone(),
        state.sensors.bat_current_path.clone(),
        state.sensors.bat_voltage_path.clone(),
    );
    drop(guard);

    // --- Everything below runs WITHOUT the lock held ---

    let (memory, swap) = read_meminfo();

    // CPU sensor
    let cpu_temp = read_temp_celsius(&cpu_temp_path);

    // GPU
    let (gpu_name, gpu_temp, gpu_power, vram_total, vram_used, gpu_busy, gpu_clock) =
        read_gpu_sensors(
            use_nvidia,
            &cached.gpu_name,
            &gpu_temp_path,
            &gpu_power_path,
            &gpu_drm_dir,
        );

    // Battery (use cloned paths)
    let battery = read_battery_from_paths(
        &bat_sensors_clone.0,
        &bat_sensors_clone.1,
        &bat_sensors_clone.2,
    );

    let nvme_temps = detect_nvme_temps();
    let total_system_watts = compute_total_power(psys_power, cpu_power, gpu_power, dram_power);

    SystemMonitorData {
        cpu: CpuInfo {
            model: cached.cpu_model,
            cores: cached.cores,
            threads: cached.threads,
            usage_percent: cpu_pct,
            per_core_usage: per_core,
            frequency_mhz: get_cpu_freq(),
            architecture: get_cpu_arch(),
        },
        memory,
        swap,
        cpu_sensor: CpuSensor {
            temp_celsius: cpu_temp,
            power_watts: cpu_power,
            power_no_permission: rapl_no_perm,
        },
        gpu: GpuInfo {
            name: gpu_name,
            temp_celsius: gpu_temp,
            power_watts: gpu_power,
            vram_total_mib: vram_total,
            vram_used_mib: vram_used,
            gpu_busy_percent: gpu_busy,
            gpu_clock_mhz: gpu_clock,
        },
        load: read_loadavg(),
        uptime: read_uptime_info(),
        battery,
        extra_power: ExtraPower {
            dram: dram_power,
            platform: psys_power,
            total_system: total_system_watts,
        },
        nvme_temps,
    }
}

// ─── Sensor Helpers ───────────────────────────────────────────

/// Read millidegree temperature from sysfs and convert to °C.
// CAST-SAFETY: millidegrees from sysfs sensor; f64 precision is more than sufficient
#[allow(clippy::cast_precision_loss)]
fn read_temp_celsius(path: &str) -> Option<f64> {
    if path.is_empty() {
        return None;
    }
    read_i64(path).map(|mdeg| mdeg as f64 / 1000.0)
}

/// Returns (name, temp_c, power_w, vram_total_mib, vram_used_mib, gpu_busy_pct)
type GpuSensorInfo = (
    String,
    Option<f64>,
    Option<f64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u32>,
);

/// Read GPU sensors from AMD sysfs or nvidia-smi.
fn read_gpu_sensors(
    use_nvidia: bool,
    fallback_name: &str,
    temp_path: &str,
    power_path: &str,
    drm_dir: &str,
) -> GpuSensorInfo {
    if use_nvidia {
        return read_nvidia_smi().unwrap_or_else(|| {
            (
                fallback_name.to_string(),
                None,
                None,
                None,
                None,
                None,
                None,
            )
        });
    }
    let temp = read_temp_celsius(temp_path);
    // CAST-SAFETY: microwatts from sysfs sensor; f64 precision is sufficient
    #[allow(clippy::cast_precision_loss)]
    let power = if power_path.is_empty() {
        None
    } else {
        read_u64(power_path).map(|uw| uw as f64 / 1e6)
    };
    let (vt, vu) = read_gpu_vram(drm_dir);
    let busy = read_gpu_busy(drm_dir);
    let clock = read_gpu_clock(drm_dir);
    (fallback_name.to_string(), temp, power, vt, vu, busy, clock)
}

/// Compute total system power: prefer platform (psys), else sum known sources.
fn compute_total_power(
    psys: Option<f64>,
    cpu: Option<f64>,
    gpu: Option<f64>,
    dram: Option<f64>,
) -> Option<f64> {
    if psys.is_some() {
        return psys;
    }
    let sources = [cpu, gpu, dram];
    let sum: f64 = sources.iter().filter_map(|s| *s).sum();
    if sources.iter().any(std::option::Option::is_some) {
        Some(sum)
    } else {
        None
    }
}

fn read_loadavg() -> LoadAvg {
    if let Ok(content) = fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 3 {
            return LoadAvg {
                one: parts[0].parse().unwrap_or(0.0),
                five: parts[1].parse().unwrap_or(0.0),
                fifteen: parts[2].parse().unwrap_or(0.0),
            };
        }
    }
    LoadAvg::default()
}

fn read_uptime_info() -> UptimeInfo {
    if let Ok(content) = fs::read_to_string("/proc/uptime") {
        if let Some(val) = content.split_whitespace().next() {
            if let Ok(secs) = val.parse::<f64>() {
                // CAST-SAFETY: /proc/uptime is always ≥0; value safely fits u64
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let total = secs as u64;
                let days = total / 86400;
                let hours = (total % 86400) / 3600;
                let mins = (total % 3600) / 60;
                let formatted = if days > 0 {
                    format!("{}d {}h {}m", days, hours, mins)
                } else if hours > 0 {
                    format!("{}h {}m", hours, mins)
                } else {
                    format!("{}m", mins)
                };
                return UptimeInfo {
                    seconds: secs,
                    formatted,
                };
            }
        }
    }
    UptimeInfo::default()
}
