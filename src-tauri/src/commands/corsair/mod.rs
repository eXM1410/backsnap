//! Corsair device control — Commander Core XT + iCUE NEXUS.
//!
//! Integrates Corsair HID device management into Arclight as Tauri commands.
//!
//! # Module Layout
//!
//! ```text
//! mod.rs      — Tauri command handlers + request/response types (this file)
//! protocol.rs — Constants, newtypes (SpeedPct, Celsius, FanMode), fan curves
//! hid.rs      — HID transport + DeviceSlot<T> abstraction
//! ccxt.rs     — Commander Core XT driver (fans, temps, RGB)
//! nexus.rs    — iCUE NEXUS driver (640×48 LCD, touch buttons)
//! ```

// Tauri commands must receive owned arguments (deserialized from JSON).
#![allow(clippy::needless_pass_by_value)]

mod ccxt;
mod hid;
mod nexus;
mod protocol;

use super::openrgb::RgbDeviceState;
use ccxt::{CcxtDriver, CcxtStatus, RgbColor};
use hid::CorsairDeviceInfo;
use nexus::{NexusDriver, NexusLayout, NexusStatus, NexusSysData};
use protocol::{FanCurvePoint, FanMode, SpeedPct};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::command;

// ─── Singletons ─────────────────────────────────────────────
//
// Both drivers are `const`-constructible → plain `static`, no `OnceLock`.
// The `DeviceSlot` inside each driver handles connect/disconnect transitions.

fn ccxt() -> &'static CcxtDriver {
    static INSTANCE: CcxtDriver = CcxtDriver::new();
    &INSTANCE
}

fn nexus() -> &'static NexusDriver {
    static INSTANCE: NexusDriver = NexusDriver::new();
    &INSTANCE
}

// ─── CCXT data cache (for NEXUS refresh thread) ─────────────

fn ccxt_cache() -> &'static Mutex<Option<CcxtStatus>> {
    static CACHE: OnceLock<Mutex<Option<CcxtStatus>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Flag controlling the NEXUS background refresh thread.
static NEXUS_THREAD_RUNNING: AtomicBool = AtomicBool::new(false);

/// Start the NEXUS background refresh thread (500 ms interval).
/// Safe to call multiple times — only the first call spawns a thread.
fn start_nexus_refresh_thread() {
    if NEXUS_THREAD_RUNNING.swap(true, Ordering::SeqCst) {
        return; // already running
    }
    std::thread::Builder::new()
        .name("nexus-refresh".into())
        .spawn(move || {
            log::info!("NEXUS refresh thread started (500 ms)");
            while NEXUS_THREAD_RUNNING.load(Ordering::SeqCst) {
                if nexus().is_connected() {
                    // Poll CCXT directly for real-time fan/temp data
                    let ccxt_data = if ccxt().is_connected() {
                        match ccxt().poll() {
                            Ok(status) => {
                                // Also update cache for other consumers
                                if let Ok(mut guard) = ccxt_cache().lock() {
                                    *guard = Some(status.clone());
                                }
                                Some(status)
                            }
                            Err(_) => ccxt_cache().lock().ok().and_then(|g| g.clone()),
                        }
                    } else {
                        None
                    };
                    let sys = gather_sys_data();
                    let _ = nexus().refresh(ccxt_data.as_ref(), Some(&sys));
                } else {
                    // Device disconnected externally → stop thread
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            NEXUS_THREAD_RUNNING.store(false, Ordering::SeqCst);
            log::info!("NEXUS refresh thread stopped");
        })
        .ok();
}

/// Stop the NEXUS background refresh thread.
fn stop_nexus_refresh_thread() {
    NEXUS_THREAD_RUNNING.store(false, Ordering::SeqCst);
}

// ═════════════════════════════════════════════════════════════
//  Corsair Profile — persisted fan curve + RGB colour
// ═════════════════════════════════════════════════════════════

/// Saved Corsair hardware profile (JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsairProfile {
    /// Fan curve applied to all channels.
    pub fan_mode: FanMode,
    /// Static RGB colour (if any).
    pub rgb: Option<RgbColor>,
    /// NEXUS LCD widget layout (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexus_layout: Option<NexusLayout>,
    /// NEXUS active page index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexus_page: Option<u8>,
    /// NEXUS auto-cycle enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexus_auto_cycle: Option<bool>,
    /// OpenRGB per-device colour/effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrgb: Option<Vec<RgbDeviceState>>,
}

fn profile_path() -> PathBuf {
    crate::config::io::config_dir().join("corsair-profile.json")
}

fn load_profile() -> Option<CorsairProfile> {
    let path = profile_path();
    let data = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&data) {
        Ok(p) => Some(p),
        Err(e) => {
            log::warn!("Corsair profile parse error: {e}");
            None
        }
    }
}

/// Load the saved RGB colour from the Corsair profile (if any).
/// Returns `Some((r, g, b))` or `None`.
pub fn load_profile_rgb() -> Option<(u8, u8, u8)> {
    load_profile().and_then(|p| p.rgb).map(|c| (c.r, c.g, c.b))
}

/// Load saved OpenRGB device states from the profile.
pub fn load_profile_openrgb() -> Vec<RgbDeviceState> {
    load_profile().and_then(|p| p.openrgb).unwrap_or_default()
}

fn save_profile(profile: &CorsairProfile) -> Result<(), String> {
    let path = profile_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(profile).map_err(|e| format!("json: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write: {e}"))?;
    log::info!("Corsair profile saved → {}", path.display());
    Ok(())
}

/// Apply a saved profile to the connected CCXT.
fn apply_profile(profile: &CorsairProfile, restore_rgb: bool) {
    if !ccxt().is_connected() {
        log::warn!("Profile: CCXT not connected, skipping");
        return;
    }

    // Small delay — give the CCXT time to stabilise after connect
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Get fan count from a quick poll
    match ccxt().poll() {
        Ok(status) => {
            let fan_count = status.fans.iter().filter(|f| f.connected).count();
            for ch in 0..fan_count {
                let ch_u8 = u8::try_from(ch).unwrap_or(0);
                if let Err(e) = ccxt().set_fan_mode(ch_u8, profile.fan_mode.clone()) {
                    log::warn!("Profile: fan {ch} mode failed: {e}");
                }
            }
            if let Err(e) = ccxt().apply_fan_modes() {
                log::warn!("Profile: apply fan modes failed: {e}");
            }
            log::warn!("Profile: fan curve applied to {fan_count} fans");
        }
        Err(e) => log::warn!("Profile: initial poll failed: {e}"),
    }

    if !restore_rgb {
        log::warn!("Profile: skipping RGB restore (startup)");
        return;
    }

    if let Some(color) = profile.rgb {
        match ccxt().set_color_static(color) {
            Ok(()) => log::warn!(
                "Profile: RGB #{:02x}{:02x}{:02x} applied",
                color.r,
                color.g,
                color.b
            ),
            Err(e) => log::warn!("Profile: RGB failed: {e}"),
        }
    } else {
        log::warn!("Profile: no RGB in profile");
    }
}

/// Auto-connect all known Corsair devices on app startup.
/// Spawns a background thread so it doesn't block the UI.
pub fn auto_connect_devices() {
    std::thread::Builder::new()
        .name("corsair-autoconnect".into())
        .spawn(|| {
            use protocol::{CCXT_PRODUCT_ID, NEXUS_PRODUCT_ID};

            // Small delay so HID subsystem is ready
            std::thread::sleep(std::time::Duration::from_millis(500));

            let devices = match hid::enumerate_devices() {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("Corsair auto-connect: enumerate failed: {e}");
                    return;
                }
            };

            log::warn!("Corsair auto-connect: found {} device(s)", devices.len());

            for dev in &devices {
                match dev.product_id {
                    CCXT_PRODUCT_ID => {
                        match ccxt().connect(&dev.serial) {
                            Ok(()) => {
                                log::warn!("Auto-connected CCXT ({})", dev.serial);
                                // Apply saved profile so RGB does not stay dark after connect.
                                if let Some(profile) = load_profile() {
                                    log::warn!("Profile loaded, applying fans + RGB...");
                                    apply_profile(&profile, true);
                                } else {
                                    log::warn!("No saved profile found");
                                }
                            }
                            Err(e) => log::warn!("Auto-connect CCXT failed: {e}"),
                        }
                    }
                    NEXUS_PRODUCT_ID => {
                        match nexus().connect(&dev.serial) {
                            Ok(()) => {
                                log::warn!("Auto-connected NEXUS ({})", dev.serial);
                                // Restore saved NEXUS layout from profile
                                if let Some(ref profile) = load_profile() {
                                    if let Some(ref layout) = profile.nexus_layout {
                                        NexusDriver::set_layout(layout.clone());
                                        log::warn!("Profile: NEXUS layout restored");
                                    }
                                    if let Some(page) = profile.nexus_page {
                                        nexus().set_page(page);
                                        log::warn!("Profile: NEXUS page → {page}");
                                    }
                                }
                                let auto_cycle = load_profile()
                                    .and_then(|p| p.nexus_auto_cycle)
                                    .unwrap_or(false);
                                nexus().set_auto_cycle(auto_cycle);
                                refresh_nexus_display();
                                start_nexus_refresh_thread();
                            }
                            Err(e) => log::warn!("Auto-connect NEXUS failed: {e}"),
                        }
                    }
                    _ => {}
                }
            }

            // ── OpenRGB peripherals stay deferred until first explicit RGB action ──────
            // Some devices (notably the K70) switch into software mode during connect and
            // can briefly clear their hardware lighting. To preserve the user's current
            // startup lighting, do not auto-connect them here.
            log::warn!("OpenRGB startup connect deferred to preserve hardware lighting state");
        })
        .ok();
}

// ═════════════════════════════════════════════════════════════
//  Request / Response Types
// ═════════════════════════════════════════════════════════════

/// Overall Corsair subsystem status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsairStatus {
    pub devices: Vec<CorsairDeviceInfo>,
    pub ccxt: Option<CcxtStatus>,
    pub nexus: Option<NexusStatus>,
}

/// Set a fixed fan speed or return to auto/curve.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetFanSpeedRequest {
    pub channel: u8,
    /// `Some(pct)` = fixed speed, `None` = return to curve mode.
    pub speed: Option<SpeedPct>,
}

/// Set a custom fan curve on a channel.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetFanCurveRequest {
    pub channel: u8,
    pub points: Vec<FanCurvePoint>,
}

/// Set a static RGB colour.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRgbRequest {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Content to push to the NEXUS LCD.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum NexusDisplayRequest {
    Clear,
    Text { label: String, value: String },
}

/// Gather system info for the NEXUS dashboard from the sysmon module.
fn gather_sys_data() -> NexusSysData {
    let m = crate::sysmon::read_system_monitor();
    #[allow(clippy::cast_possible_truncation)]
    let total_power = {
        let cpu_w = m.cpu_sensor.power_watts.unwrap_or(0.0);
        let gpu_w = m.gpu.power_watts.unwrap_or(0.0);
        let dram_w = m.extra_power.dram.unwrap_or(0.0);
        let sum = cpu_w + gpu_w + dram_w;
        if sum > 0.0 {
            Some(sum as f32)
        } else {
            m.extra_power.total_system.map(|w| w as f32)
        }
    };
    #[allow(clippy::cast_possible_truncation)]
    NexusSysData {
        cpu_usage: m.cpu.usage_percent as f32,
        cpu_freq_mhz: m.cpu.frequency_mhz.unwrap_or(0.0) as f32,
        ram_used_pct: m.memory.percent as f32,
        ram_total_gib: m.memory.total_mib as f32 / 1024.0,
        cpu_temp: m.cpu_sensor.temp_celsius.map(|t| t as f32),
        gpu_temp: m.gpu.temp_celsius.map(|t| t as f32),
        gpu_usage: m.gpu.gpu_busy_percent.map(|u| u as f32),
        total_power_w: total_power,
        water_temp: None, // filled from CCXT probe
    }
}

/// Trigger a single immediate NEXUS refresh (e.g. after page change).
fn refresh_nexus_display() {
    if nexus().is_connected() {
        let ccxt_data = ccxt_cache().lock().ok().and_then(|g| g.clone());
        let sys = gather_sys_data();
        let _ = nexus().refresh(ccxt_data.as_ref(), Some(&sys));
    }
}

// ═════════════════════════════════════════════════════════════
//  Tauri Commands
// ═════════════════════════════════════════════════════════════

#[command]
pub fn get_corsair_status() -> Result<CorsairStatus, String> {
    let devices = hid::enumerate_devices().map_err(|e| e.to_string())?;
    let ccxt_data = if ccxt().is_connected() {
        ccxt().poll().ok()
    } else {
        None
    };
    // Cache latest CCXT data for the NEXUS refresh thread
    if let (Some(data), Ok(mut guard)) = (ccxt_data.as_ref(), ccxt_cache().lock()) {
        *guard = Some(data.clone());
    }
    let nexus_connected = nexus().is_connected();
    Ok(CorsairStatus {
        ccxt: ccxt_data,
        nexus: if nexus_connected {
            nexus().status().ok()
        } else {
            None
        },
        devices,
    })
}

#[command]
pub fn corsair_ccxt_connect(serial: String) -> Result<String, String> {
    ccxt().connect(&serial).map_err(|e| e.to_string())?;
    Ok("Commander Core XT connected".into())
}

#[command]
pub fn corsair_ccxt_disconnect() -> Result<String, String> {
    ccxt().disconnect().map_err(|e| e.to_string())?;
    Ok("Commander Core XT disconnected".into())
}

#[command]
pub fn corsair_ccxt_poll() -> Result<CcxtStatus, String> {
    let status = ccxt().poll().map_err(|e| e.to_string())?;
    // Cache for NEXUS refresh thread
    if let Ok(mut guard) = ccxt_cache().lock() {
        *guard = Some(status.clone());
    }
    Ok(status)
}

/// Set fan speed.  Constructs the appropriate `FanMode` variant from the
/// request, so the frontend doesn't need to know about the enum directly.
#[command]
pub fn corsair_set_fan_speed(request: SetFanSpeedRequest) -> Result<String, String> {
    let mode = match request.speed {
        Some(pct) => FanMode::Fixed { speed: pct },
        None => FanMode::default(),
    };
    ccxt()
        .set_fan_mode(request.channel, mode)
        .map_err(|e| e.to_string())?;
    Ok(match request.speed {
        Some(pct) => format!("Fan {} → {}%", request.channel, pct.get()),
        None => format!("Fan {} → curve", request.channel),
    })
}

#[command]
pub fn corsair_set_fan_curve(request: SetFanCurveRequest) -> Result<String, String> {
    ccxt()
        .set_fan_mode(
            request.channel,
            FanMode::Curve {
                points: request.points,
            },
        )
        .map_err(|e| e.to_string())?;
    Ok(format!("Fan {} curve updated", request.channel))
}

#[command]
pub fn corsair_apply_fan_curves() -> Result<String, String> {
    ccxt().apply_fan_modes().map_err(|e| e.to_string())?;
    Ok("Fan modes applied".into())
}

#[command]
pub fn corsair_set_rgb(request: SetRgbRequest) -> Result<String, String> {
    // Scale by master brightness before sending to hardware
    let (sr, sg, sb) = super::lighting::apply_brightness(request.r, request.g, request.b);
    ccxt()
        .set_color_static(RgbColor {
            r: sr,
            g: sg,
            b: sb,
        })
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "RGB → #{:02x}{:02x}{:02x}",
        request.r, request.g, request.b
    ))
}

#[command]
pub fn corsair_nexus_connect(serial: String) -> Result<String, String> {
    nexus().connect(&serial).map_err(|e| e.to_string())?;
    // Push first frame immediately, then start background refresh
    refresh_nexus_display();
    start_nexus_refresh_thread();
    Ok("iCUE NEXUS connected".into())
}

#[command]
pub fn corsair_nexus_disconnect() -> Result<String, String> {
    stop_nexus_refresh_thread();
    nexus().disconnect().map_err(|e| e.to_string())?;
    Ok("iCUE NEXUS disconnected".into())
}

#[command]
pub fn corsair_nexus_status() -> Result<NexusStatus, String> {
    nexus().status().map_err(|e| e.to_string())
}

#[command]
pub fn corsair_nexus_display(content: NexusDisplayRequest) -> Result<String, String> {
    match content {
        NexusDisplayRequest::Clear => {
            nexus().clear_display().map_err(|e| e.to_string())?;
            Ok("NEXUS cleared".into())
        }
        NexusDisplayRequest::Text { label, value } => {
            nexus()
                .display_text(&label, &value)
                .map_err(|e| e.to_string())?;
            Ok("NEXUS text updated".into())
        }
    }
}

#[command]
#[allow(clippy::unnecessary_wraps)]
pub fn corsair_nexus_set_page(page: u8) -> Result<String, String> {
    nexus().set_page(page);
    refresh_nexus_display();
    Ok(format!("NEXUS page → {page}"))
}

#[command]
#[allow(clippy::unnecessary_wraps)]
pub fn corsair_nexus_next_page() -> Result<String, String> {
    nexus().next_page();
    refresh_nexus_display();
    Ok("NEXUS next page".into())
}

#[command]
#[allow(clippy::unnecessary_wraps)]
pub fn corsair_nexus_prev_page() -> Result<String, String> {
    nexus().prev_page();
    refresh_nexus_display();
    Ok("NEXUS prev page".into())
}

#[command]
#[allow(clippy::unnecessary_wraps)]
pub fn corsair_nexus_set_auto_cycle(enabled: bool) -> Result<String, String> {
    nexus().set_auto_cycle(enabled);
    Ok(format!("NEXUS auto-cycle = {enabled}"))
}

#[command]
pub fn corsair_nexus_refresh_sys(data: NexusSysData) -> Result<String, String> {
    if !nexus().is_connected() {
        return Ok("NEXUS not connected".into());
    }
    let ccxt_data = if ccxt().is_connected() {
        ccxt().poll().ok()
    } else {
        None
    };
    nexus()
        .refresh(ccxt_data.as_ref(), Some(&data))
        .map_err(|e| e.to_string())?;
    Ok("NEXUS refreshed".into())
}

#[command]
#[allow(clippy::unnecessary_wraps)]
pub fn corsair_nexus_get_layout() -> Result<NexusLayout, String> {
    Ok(NexusDriver::get_layout())
}

#[command]
pub fn corsair_nexus_set_layout(layout: NexusLayout) -> Result<String, String> {
    NexusDriver::set_layout(layout);
    refresh_nexus_display();
    Ok("NEXUS layout updated".into())
}

#[command]
pub fn corsair_nexus_reset_layout() -> Result<NexusLayout, String> {
    let layout = NexusLayout::default_layout();
    NexusDriver::set_layout(layout.clone());
    refresh_nexus_display();
    Ok(layout)
}

#[command]
#[allow(clippy::unnecessary_wraps)]
pub fn corsair_nexus_get_frame() -> Result<String, String> {
    Ok(NexusDriver::get_last_frame_base64())
}

/// Save the current fan curve + RGB + NEXUS layout + OpenRGB as a profile.
/// This profile is automatically restored on every app startup.
#[command]
pub fn corsair_save_profile() -> Result<String, String> {
    let fan_mode = if ccxt().is_connected() {
        let status = ccxt().poll().map_err(|e| e.to_string())?;
        status.fan_modes.first().cloned().unwrap_or_default()
    } else {
        FanMode::default()
    };

    let rgb = ccxt().current_color();
    let nexus_layout = Some(NexusDriver::get_layout());

    let (nexus_page, nexus_auto_cycle) = if nexus().is_connected() {
        let st = nexus().status().ok();
        (
            st.as_ref().map(|s| s.current_page),
            st.as_ref().map(|s| s.auto_cycle),
        )
    } else {
        (None, None)
    };

    let openrgb_state = super::openrgb::get_openrgb_state();
    let openrgb = if openrgb_state.is_empty() {
        None
    } else {
        Some(openrgb_state)
    };

    let profile = CorsairProfile {
        fan_mode,
        rgb,
        nexus_layout,
        nexus_page,
        nexus_auto_cycle,
        openrgb,
    };
    save_profile(&profile)?;

    Ok("Profil gespeichert (Lüfter + RGB + NEXUS + Peripherie)".into())
}
