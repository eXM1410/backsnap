//! Pi Remote — manage Raspberry Pi devices over SSH/NFS from within arclight.
//!
//! Pis are stored in the app config (`pi_remote.devices`). A setup wizard
//! in the frontend lets users add/remove/edit devices. Status is gathered
//! via SSH, and a remote desktop can be launched via xfreerdp/Remmina.

use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::helpers::cmd_exists;
use crate::config::{
    invalidate_config_cache, load_config, save_config, PiDeviceConfig, RemoteProtocol,
};
use crate::util::safe_cmd_timeout;

// ─── SSH Section Markers ──────────────────────────────────────
// Keep the command builder and parser in sync via shared constants.

const SEC_CPU1: &str = "---CPU1---";
const SEC_HOSTNAME: &str = "---HOSTNAME---";
const SEC_KERNEL: &str = "---KERNEL---";
const SEC_UPTIME: &str = "---UPTIME---";
const SEC_TEMP: &str = "---TEMP---";
const SEC_THROTTLED: &str = "---THROTTLED---";
const SEC_MEM: &str = "---MEM---";
const SEC_DISK: &str = "---DISK---";
const SEC_CPUFREQ: &str = "---CPUFREQ---";
const SEC_CPU2: &str = "---CPU2---";
const SEC_SERVICES: &str = "---SERVICES---";

// ─── Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiDevice {
    pub id: String,
    pub label: String,
    pub model: String,
    pub ip: String,
    pub user: String,
    pub ssh_key: String,
    pub mount_point: String,
    pub remote_protocol: RemoteProtocol,
    pub remote_port: u16,
    pub rdp_password: String,
    pub watch_services: Vec<String>,
}

impl From<PiDeviceConfig> for PiDevice {
    fn from(c: PiDeviceConfig) -> Self {
        Self {
            id: c.id,
            label: c.label,
            model: c.model,
            ip: c.ip,
            user: c.user,
            ssh_key: c.ssh_key,
            mount_point: c.mount_point,
            remote_protocol: c.remote_protocol,
            remote_port: c.remote_port,
            rdp_password: c.rdp_password,
            watch_services: c.watch_services,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiStatus {
    pub id: String,
    pub label: String,
    pub model: String,
    pub ip: String,
    pub online: bool,
    pub uptime: Option<String>,
    pub cpu_temp: Option<f64>,
    pub cpu_usage: Option<f64>,
    pub cpu_freq_mhz: Option<u64>,
    pub mem_total_mb: Option<u64>,
    pub mem_used_mb: Option<u64>,
    pub disk_total_gb: Option<f64>,
    pub disk_used_gb: Option<f64>,
    pub disk_percent: Option<u8>,
    pub hostname: Option<String>,
    pub kernel: Option<String>,
    pub throttled: Option<String>,
    pub nfs_mounted: bool,
    pub services: Vec<PiService>,
    pub remote_protocol: RemoteProtocol,
    pub remote_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiService {
    pub name: String,
    pub active: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiActionResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiTestResult {
    pub reachable: bool,
    pub ssh_ok: bool,
    pub hostname: Option<String>,
    pub model: Option<String>,
    pub kernel: Option<String>,
    pub error: Option<String>,
}

// ─── Config Helpers ───────────────────────────────────────────

fn configured_pis() -> Vec<PiDeviceConfig> {
    load_config()
        .map(|c| c.pi_remote.devices)
        .unwrap_or_default()
}

fn find_pi(id: &str) -> Result<PiDeviceConfig, String> {
    configured_pis()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("Unbekannter Pi: {}", id))
}

// ─── SSH Helper ───────────────────────────────────────────────

fn user_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| {
        #[allow(unsafe_code)]
        let uid = unsafe { libc::getuid() };
        #[allow(unsafe_code)]
        let pw = unsafe { libc::getpwuid(uid) };
        if pw.is_null() {
            "/root".into()
        } else {
            #[allow(unsafe_code)]
            let dir = unsafe { std::ffi::CStr::from_ptr((*pw).pw_dir) };
            dir.to_string_lossy().into_owned()
        }
    })
}

fn resolve_key(ssh_key: &str) -> String {
    if ssh_key.is_empty() {
        format!("{}/.ssh/id_ed25519", user_home())
    } else if ssh_key.starts_with('~') {
        ssh_key.replacen('~', &user_home(), 1)
    } else {
        ssh_key.to_string()
    }
}

/// Run SSH with an explicit key. Returns (success, output_text).
fn ssh_run(user: &str, ip: &str, key: &str, remote_cmd: &str, timeout: Duration) -> (bool, String) {
    let home = user_home();
    let key_path = resolve_key(key);
    let target = format!("{}@{}", user, ip);

    let child = Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=5",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "BatchMode=yes",
            "-o",
            "PasswordAuthentication=no",
            "-i",
            &key_path,
            &target,
            remote_cmd,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", &home)
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (false, format!("SSH spawn: {}", e)),
    };

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let out = child.wait_with_output().ok();
                return match out {
                    Some(o) => {
                        let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        if o.status.success() {
                            (true, stdout)
                        } else if !stderr.is_empty() {
                            (
                                false,
                                format!(
                                    "SSH error (exit {}): {}",
                                    o.status.code().unwrap_or(-1),
                                    stderr
                                ),
                            )
                        } else if !stdout.is_empty() {
                            (false, stdout)
                        } else {
                            (false, format!("SSH exit {}", o.status.code().unwrap_or(-1)))
                        }
                    }
                    None => (false, "SSH output unlesbar".into()),
                };
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (false, format!("SSH timeout ({}s)", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return (false, format!("SSH error: {}", e));
            }
        }
    }
}

fn ssh_cmd(pi: &PiDeviceConfig, remote_cmd: &str, timeout: Duration) -> Option<String> {
    let (ok, out) = ssh_run(&pi.user, &pi.ip, &pi.ssh_key, remote_cmd, timeout);
    if ok {
        Some(out)
    } else {
        None
    }
}

fn ssh_cmd_full(pi: &PiDeviceConfig, remote_cmd: &str, timeout: Duration) -> (bool, String) {
    ssh_run(&pi.user, &pi.ip, &pi.ssh_key, remote_cmd, timeout)
}

fn is_reachable(ip: &str) -> bool {
    safe_cmd_timeout("ping", &["-c1", "-W1", ip], Duration::from_secs(2))
        .is_some_and(|o| o.status.success())
}

fn is_nfs_mounted(mount_point: &str) -> bool {
    if mount_point.is_empty() {
        return false;
    }
    std::fs::read_to_string("/proc/mounts")
        .unwrap_or_default()
        .lines()
        .any(|l| l.contains(mount_point) && l.contains("nfs"))
}

// ─── Status Gathering ─────────────────────────────────────────

fn gather_pi_status(pi: &PiDeviceConfig) -> PiStatus {
    let online = is_reachable(&pi.ip);
    let nfs_mounted = is_nfs_mounted(&pi.mount_point);

    if !online {
        return PiStatus {
            id: pi.id.clone(),
            label: pi.label.clone(),
            model: pi.model.clone(),
            ip: pi.ip.clone(),
            online: false,
            uptime: None,
            cpu_temp: None,
            cpu_usage: None,
            cpu_freq_mhz: None,
            mem_total_mb: None,
            mem_used_mb: None,
            disk_total_gb: None,
            disk_used_gb: None,
            disk_percent: None,
            hostname: None,
            kernel: None,
            throttled: None,
            nfs_mounted,
            services: vec![],
            remote_protocol: pi.remote_protocol,
            remote_port: pi.remote_port,
        };
    }

    let timeout = Duration::from_secs(8);

    // Build service check dynamically
    let svc_script = if pi.watch_services.is_empty() {
        String::new()
    } else {
        let svcs = pi.watch_services.join(" ");
        format!(
            "echo '{SEC_SERVICES}' && for svc in {svcs}; do \
             active=$(systemctl is-active $svc 2>/dev/null); \
             enabled=$(systemctl is-enabled $svc 2>/dev/null); \
             echo \"$svc:$active:$enabled\"; done &&"
        )
    };

    let batch_cmd = format!(
        "echo '{SEC_CPU1}' && head -1 /proc/stat && \
         echo '{SEC_HOSTNAME}' && hostname && \
         echo '{SEC_KERNEL}' && uname -r && \
         echo '{SEC_UPTIME}' && uptime -p && \
         echo '{SEC_TEMP}' && vcgencmd measure_temp 2>/dev/null && \
         echo '{SEC_THROTTLED}' && vcgencmd get_throttled 2>/dev/null && \
         echo '{SEC_MEM}' && free -m | grep Mem && \
         echo '{SEC_DISK}' && df -BG / | tail -1 && \
         echo '{SEC_CPUFREQ}' && cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq 2>/dev/null && \
         {} \
         sleep 0.5 && echo '{SEC_CPU2}' && head -1 /proc/stat",
        svc_script
    );

    let batch = ssh_cmd(pi, &batch_cmd, timeout).unwrap_or_default();

    let mut hostname = None;
    let mut kernel = None;
    let mut uptime = None;
    let mut cpu_temp = None;
    let mut throttled = None;
    let mut mem_total_mb = None;
    let mut mem_used_mb = None;
    let mut disk_total_gb = None;
    let mut disk_used_gb = None;
    let mut disk_percent = None;
    let mut cpu_freq_mhz = None;
    let mut cpu1_line = None;
    let mut cpu2_line = None;
    let mut services = Vec::new();

    let mut section = "";
    for line in batch.lines() {
        let l = line.trim();
        if l.starts_with("---") && l.ends_with("---") {
            section = l;
            continue;
        }
        match section {
            SEC_HOSTNAME => hostname = Some(l.to_string()),
            SEC_KERNEL => kernel = Some(l.to_string()),
            SEC_UPTIME => uptime = Some(l.to_string()),
            SEC_TEMP => {
                if let Some(val) = l.strip_prefix("temp=") {
                    cpu_temp = val.trim_end_matches("'C").parse::<f64>().ok();
                }
            }
            SEC_THROTTLED => throttled = Some(l.to_string()),
            SEC_MEM => {
                let p: Vec<&str> = l.split_whitespace().collect();
                if p.len() >= 3 {
                    mem_total_mb = p.get(1).and_then(|v| v.parse().ok());
                    mem_used_mb = p.get(2).and_then(|v| v.parse().ok());
                }
            }
            SEC_DISK => {
                let p: Vec<&str> = l.split_whitespace().collect();
                if p.len() >= 5 {
                    disk_total_gb = p
                        .get(1)
                        .and_then(|v| v.trim_end_matches('G').parse::<f64>().ok());
                    disk_used_gb = p
                        .get(2)
                        .and_then(|v| v.trim_end_matches('G').parse::<f64>().ok());
                    disk_percent = p
                        .get(4)
                        .and_then(|v| v.trim_end_matches('%').parse::<u8>().ok());
                }
            }
            SEC_SERVICES => {
                let p: Vec<&str> = l.splitn(3, ':').collect();
                if p.len() == 3 {
                    services.push(PiService {
                        name: p[0].to_string(),
                        active: p[1] == "active",
                        enabled: p[2] == "enabled",
                    });
                }
            }
            SEC_CPUFREQ => {
                cpu_freq_mhz = l.parse::<u64>().ok().map(|khz| khz / 1000);
            }
            SEC_CPU1 => {
                if l.starts_with("cpu ") {
                    cpu1_line = Some(l.to_string());
                }
            }
            SEC_CPU2 => {
                if l.starts_with("cpu ") {
                    cpu2_line = Some(l.to_string());
                }
            }
            _ => {}
        }
    }

    let cpu_usage = match (cpu1_line.as_deref(), cpu2_line.as_deref()) {
        (Some(l1), Some(l2)) => calc_cpu_usage(l1, l2),
        _ => None,
    };

    PiStatus {
        id: pi.id.clone(),
        label: pi.label.clone(),
        model: pi.model.clone(),
        ip: pi.ip.clone(),
        online: true,
        uptime,
        cpu_temp,
        cpu_usage,
        cpu_freq_mhz,
        mem_total_mb,
        mem_used_mb,
        disk_total_gb,
        disk_used_gb,
        disk_percent,
        hostname,
        kernel,
        throttled,
        nfs_mounted,
        services,
        remote_protocol: pi.remote_protocol,
        remote_port: pi.remote_port,
    }
}

fn calc_cpu_usage(l1: &str, l2: &str) -> Option<f64> {
    fn parse(line: &str) -> Option<(u64, u64)> {
        let p: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|v| v.parse().ok())
            .collect();
        if p.len() >= 4 {
            Some((p.iter().sum(), p[3]))
        } else {
            None
        }
    }
    let (t1, i1) = parse(l1)?;
    let (t2, i2) = parse(l2)?;
    let dt = t2.saturating_sub(t1);
    let di = i2.saturating_sub(i1);
    if dt == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    Some(((dt - di) as f64 / dt as f64) * 100.0)
}

// ═══════════════════════════════════════════════════════════════
//                     TAURI COMMANDS
// ═══════════════════════════════════════════════════════════════

// ─── CRUD (Wizard) ────────────────────────────────────────────

#[tauri::command]
pub async fn get_pi_devices() -> Result<Vec<PiDevice>, String> {
    Ok(configured_pis().into_iter().map(PiDevice::from).collect())
}

/// Test SSH connection during wizard setup (before device is saved).
#[tauri::command]
pub async fn test_pi_connection(
    ip: String,
    user: String,
    ssh_key: String,
) -> Result<PiTestResult, String> {
    tokio::task::spawn_blocking(move || {
        let reachable = is_reachable(&ip);
        if !reachable {
            return Ok(PiTestResult {
                reachable: false, ssh_ok: false, hostname: None, model: None,
                kernel: None, error: Some(format!("{} nicht erreichbar", ip)),
            });
        }
        let (ssh_ok, out) = ssh_run(
            &user, &ip, &ssh_key,
            "echo '---OK---' && hostname && uname -r && cat /proc/device-tree/model 2>/dev/null || echo unknown",
            Duration::from_secs(8),
        );
        if !ssh_ok {
            return Ok(PiTestResult {
                reachable: true, ssh_ok: false, hostname: None, model: None,
                kernel: None, error: Some(out),
            });
        }
        let lines: Vec<&str> = out.lines().collect();
        Ok(PiTestResult {
            reachable: true,
            ssh_ok: true,
            hostname: lines.get(1).map(|s| s.trim().to_string()),
            kernel: lines.get(2).map(|s| s.trim().to_string()),
            model: lines.get(3).map(|s| s.trim().trim_end_matches('\0').to_string()).filter(|m| m != "unknown" && !m.is_empty()),
            error: None,
        })
    }).await.map_err(|e| format!("Thread error: {}", e))?
}

/// Add or update a Pi device.
#[tauri::command]
pub async fn add_pi_device(device: PiDevice) -> Result<PiActionResult, String> {
    let mut config = load_config().map_err(|e| format!("Config laden: {}", e))?;
    if let Some(existing) = config
        .pi_remote
        .devices
        .iter_mut()
        .find(|d| d.id == device.id)
    {
        existing.label = device.label;
        existing.model = device.model;
        existing.ip = device.ip;
        existing.user = device.user;
        existing.ssh_key = device.ssh_key;
        existing.mount_point = device.mount_point;
        existing.remote_protocol = device.remote_protocol;
        existing.remote_port = device.remote_port;
        existing.rdp_password = device.rdp_password;
        existing.watch_services = device.watch_services;
    } else {
        config.pi_remote.devices.push(PiDeviceConfig {
            id: device.id.clone(),
            label: device.label,
            model: device.model,
            ip: device.ip,
            user: device.user,
            ssh_key: device.ssh_key,
            mount_point: device.mount_point,
            remote_protocol: device.remote_protocol,
            remote_port: device.remote_port,
            rdp_password: device.rdp_password,
            watch_services: device.watch_services,
        });
    }
    save_config(&config).map_err(|e| format!("Speichern: {}", e))?;
    invalidate_config_cache();
    Ok(PiActionResult {
        success: true,
        message: format!("Pi '{}' gespeichert", device.id),
    })
}

/// Remove a Pi device.
#[tauri::command]
pub async fn remove_pi_device(id: String) -> Result<PiActionResult, String> {
    let mut config = load_config().map_err(|e| format!("Config laden: {}", e))?;
    let before = config.pi_remote.devices.len();
    config.pi_remote.devices.retain(|d| d.id != id);
    if config.pi_remote.devices.len() == before {
        return Err(format!("Pi '{}' nicht gefunden", id));
    }
    save_config(&config).map_err(|e| format!("Speichern: {}", e))?;
    invalidate_config_cache();
    Ok(PiActionResult {
        success: true,
        message: format!("Pi '{}' entfernt", id),
    })
}

// ─── Status ───────────────────────────────────────────────────

#[tauri::command]
pub async fn get_pi_status_all() -> Result<Vec<PiStatus>, String> {
    let pis = configured_pis();
    if pis.is_empty() {
        return Ok(vec![]);
    }
    let handles: Vec<_> = pis
        .into_iter()
        .map(|pi| tokio::task::spawn_blocking(move || gather_pi_status(&pi)))
        .collect();
    let mut out = Vec::new();
    for h in handles {
        match h.await {
            Ok(s) => out.push(s),
            Err(e) => return Err(format!("Thread panicked: {}", e)),
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_pi_status(id: String) -> Result<PiStatus, String> {
    let pi = find_pi(&id)?;
    tokio::task::spawn_blocking(move || Ok(gather_pi_status(&pi)))
        .await
        .map_err(|e| format!("Thread: {}", e))?
}

// ─── Actions ──────────────────────────────────────────────────

#[tauri::command]
pub async fn pi_reboot(id: String) -> Result<PiActionResult, String> {
    let pi = find_pi(&id)?;
    tokio::task::spawn_blocking(move || {
        let _ = ssh_cmd(&pi, "sudo reboot", Duration::from_secs(5));
        Ok(PiActionResult {
            success: true,
            message: "Reboot ausgelöst".into(),
        })
    })
    .await
    .map_err(|e| format!("Thread: {}", e))?
}

#[tauri::command]
pub async fn pi_shutdown(id: String) -> Result<PiActionResult, String> {
    let pi = find_pi(&id)?;
    tokio::task::spawn_blocking(move || {
        let _ = ssh_cmd(&pi, "sudo shutdown -h now", Duration::from_secs(5));
        Ok(PiActionResult {
            success: true,
            message: "Shutdown ausgelöst".into(),
        })
    })
    .await
    .map_err(|e| format!("Thread: {}", e))?
}

#[tauri::command]
pub async fn pi_run_command(id: String, command: String) -> Result<PiActionResult, String> {
    let pi = find_pi(&id)?;
    tokio::task::spawn_blocking(move || {
        let (success, message) = ssh_cmd_full(&pi, &command, Duration::from_secs(15));
        Ok(PiActionResult { success, message })
    })
    .await
    .map_err(|e| format!("Thread: {}", e))?
}

// ─── Remote Desktop ───────────────────────────────────────────

#[tauri::command]
pub async fn open_pi_remote(id: String) -> Result<PiActionResult, String> {
    let pi = find_pi(&id)?;
    if pi.remote_port == 0 {
        return Err("Remote Desktop nicht konfiguriert (Port = 0)".into());
    }
    let ip = pi.ip.clone();
    let user = pi.user.clone();
    let password = pi.rdp_password.clone();
    let port = pi.remote_port;
    let protocol = pi.remote_protocol;

    tokio::task::spawn_blocking(move || {
        // Ensure DISPLAY is set for X11 apps launched from Tauri
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();

        match protocol {
            RemoteProtocol::Vnc => Ok(open_vnc(&ip, port, &password, &display, &wayland)),
            RemoteProtocol::Rdp => Ok(open_rdp(&ip, port, &user, &password, &display, &wayland)),
        }
    })
    .await
    .map_err(|e| format!("Thread: {}", e))?
}

fn open_vnc(ip: &str, port: u16, _password: &str, display: &str, wayland: &str) -> PiActionResult {
    let xdg = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
    let xauth = std::env::var("XAUTHORITY").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        format!("{}/.Xauthority", home)
    });

    // Prefer remmina — has proper client-side scaling (fit to window)
    if cmd_exists("remmina") {
        // Create a temporary .remmina connection file with scaling enabled
        let tmp = format!("/tmp/arclight-vnc-{}-{}.remmina", ip, port);
        let content = format!(
            "[remmina]\n\
             name=Pi {ip}:{port}\n\
             server={ip}:{port}\n\
             protocol=VNC\n\
             quality=2\n\
             viewmode=1\n\
             scale=1\n\
             window_maximize=0\n\
             window_width=1024\n\
             window_height=768\n",
            ip = ip,
            port = port,
        );
        let _ = std::fs::write(&tmp, &content);
        match Command::new("remmina")
            .args(["-c", &tmp])
            .env("DISPLAY", display)
            .env("WAYLAND_DISPLAY", wayland)
            .env("XDG_RUNTIME_DIR", &xdg)
            .env("XAUTHORITY", &xauth)
            .stdin(Stdio::null())
            .spawn()
        {
            Ok(_) => {
                return PiActionResult {
                    success: true,
                    message: format!("Remmina VNC → {}:{}", ip, port),
                }
            }
            Err(e) => {
                return PiActionResult {
                    success: false,
                    message: format!("Remmina Fehler: {}", e),
                }
            }
        }
    }

    // Fallback to vncviewer (TigerVNC) — no client-side scaling
    if cmd_exists("vncviewer") {
        let target = format!("{}:{}", ip, port);
        match Command::new("vncviewer")
            .arg(&target)
            .args([
                "-RemoteResize=0",
                "-FullscreenSystemKeys=0",
                "-AcceptClipboard=1",
            ])
            .env("DISPLAY", display)
            .env("WAYLAND_DISPLAY", wayland)
            .env("XDG_RUNTIME_DIR", &xdg)
            .env("XAUTHORITY", &xauth)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                std::thread::sleep(Duration::from_millis(500));
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let stderr = child
                            .stderr
                            .take()
                            .map(|mut e| {
                                let mut s = String::new();
                                std::io::Read::read_to_string(&mut e, &mut s).ok();
                                s
                            })
                            .unwrap_or_default();
                        return PiActionResult {
                            success: false,
                            message: format!("vncviewer beendet ({}): {}", status, stderr.trim()),
                        };
                    }
                    _ => {
                        return PiActionResult {
                            success: true,
                            message: format!("vncviewer → {}:{}", ip, port),
                        }
                    }
                }
            }
            Err(e) => {
                return PiActionResult {
                    success: false,
                    message: format!("vncviewer Fehler: {}", e),
                }
            }
        }
    }

    PiActionResult {
        success: false,
        message: "Kein VNC-Client gefunden. Installiere: sudo pacman -S remmina libvncserver"
            .into(),
    }
}

fn open_rdp(
    ip: &str,
    port: u16,
    user: &str,
    password: &str,
    display: &str,
    wayland: &str,
) -> PiActionResult {
    let target = format!("/v:{}:{}", ip, port);
    let mut base = vec![target];
    if !user.is_empty() {
        base.push(format!("/u:{}", user));
    }
    if !password.is_empty() {
        base.push(format!("/p:{}", password));
    }

    // Try xfreerdp3/xfreerdp with table-driven extra args
    let clients: &[(&str, &[&str])] = &[
        (
            "xfreerdp3",
            &[
                "/cert:ignore",
                "/size:1920x1080",
                "/smart-sizing",
                "/kbd:layout:German",
                "+auto-reconnect",
            ],
        ),
        (
            "xfreerdp",
            &[
                "/cert-ignore",
                "/size:1920x1080",
                "/smart-sizing",
                "/kbd:German",
                "+auto-reconnect",
            ],
        ),
    ];
    for &(cmd, extra) in clients {
        if !cmd_exists(cmd) {
            continue;
        }
        let mut args: Vec<String> = base.clone();
        args.extend(extra.iter().map(|s| (*s).to_string()));
        match Command::new(cmd)
            .args(&args)
            .env("DISPLAY", display)
            .env("WAYLAND_DISPLAY", wayland)
            .stdin(Stdio::null())
            .spawn()
        {
            Ok(_) => {
                return PiActionResult {
                    success: true,
                    message: format!("{} → {}:{}", cmd, ip, port),
                }
            }
            Err(e) => {
                return PiActionResult {
                    success: false,
                    message: format!("{} Fehler: {}", cmd, e),
                }
            }
        }
    }

    // Fallback: remmina (different arg style)
    if cmd_exists("remmina") {
        let uri = format!("rdp://{}:{}", ip, port);
        match Command::new("remmina")
            .args(["-c", &uri])
            .env("DISPLAY", display)
            .env("WAYLAND_DISPLAY", wayland)
            .stdin(Stdio::null())
            .spawn()
        {
            Ok(_) => {
                return PiActionResult {
                    success: true,
                    message: format!("Remmina → {}:{}", ip, port),
                }
            }
            Err(e) => {
                return PiActionResult {
                    success: false,
                    message: format!("Remmina Fehler: {}", e),
                }
            }
        }
    }

    PiActionResult {
        success: false,
        message: "Kein RDP-Client gefunden. Installiere: sudo pacman -S freerdp".into(),
    }
}
