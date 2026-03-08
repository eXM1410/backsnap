//! Master lighting control — all RGB devices + Govee via Pi 5 LAN.
//!
//! Provides a single command to turn every light in the house on or off:
//! - Corsair Commander Core XT (6× fan RGB)
//! - OpenRGB devices (IT8297, K70 TKL, Aerox 3, QCK Prism)
//! - Govee smart lights via Pi 5 Rust controller (direct HTTP)

use serde::Serialize;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;
use tauri::command;

use reqwest::blocking::Client;

use super::corsair;
use super::openrgb;

// ─── Pi / Govee config ───────────────────────────────────────

const GOVEE_PI5_POWER: &str = "http://192.168.0.8:4080/api/govee/power";
const GOVEE_PI5_BRIGHTNESS: &str = "http://192.168.0.8:4080/api/govee/brightness";
const GOVEE_PI5_COLOR: &str = "http://192.168.0.8:4080/api/govee/color";
const GOVEE_PI5_DECKENLAMPE_COLOR: &str = "http://192.168.0.8:4080/api/govee/deckenlampe/color";
const GOVEE_PI5_DECKENLAMPE2_COLOR: &str = "http://192.168.0.8:4080/api/govee/deckenlampe2/color";
const GOVEE_PI5_STEHLAMPE_COLOR: &str = "http://192.168.0.8:4080/api/govee/stehlampe/color";
const GOVEE_PI5_RACHEL_COLOR: &str = "http://192.168.0.8:4080/api/govee/rachel/color";
const OPENRGB_DEVICE_IDS: &[&str] = &["it8297", "k70", "aerox3", "qck", "xpg_s40g", "xpg_s20g"];

type Rgb = (u8, u8, u8);

#[derive(Clone, Copy)]
struct GoveeLamp {
    id: &'static str,
    url: &'static str,
}

const GOVEE_LAMPS: &[GoveeLamp] = &[
    GoveeLamp {
        id: "deckenlampe",
        url: GOVEE_PI5_DECKENLAMPE_COLOR,
    },
    GoveeLamp {
        id: "deckenlampe2",
        url: GOVEE_PI5_DECKENLAMPE2_COLOR,
    },
    GoveeLamp {
        id: "stehlampe",
        url: GOVEE_PI5_STEHLAMPE_COLOR,
    },
    GoveeLamp {
        id: "rachel",
        url: GOVEE_PI5_RACHEL_COLOR,
    },
];

const PURPLE_PC_SCENE: PcScene = PcScene {
    corsair: (75, 0, 255),
    openrgb: (25, 0, 255),
};

const PURPLE_GOVEE_SCENE: &[GoveeTarget] = &[
    GoveeTarget::new("deckenlampe", GOVEE_PI5_DECKENLAMPE_COLOR, (70, 0, 255)),
    GoveeTarget::new("deckenlampe2", GOVEE_PI5_DECKENLAMPE2_COLOR, (75, 0, 255)),
    GoveeTarget::new("stehlampe", GOVEE_PI5_STEHLAMPE_COLOR, (110, 0, 255)),
    GoveeTarget::new("rachel", GOVEE_PI5_RACHEL_COLOR, (110, 0, 255)),
];
static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn http_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| match Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            log::warn!("[lighting] failed to build configured HTTP client, falling back: {err}");
            Client::new()
        }
    })
}

fn govee_post(url: &str, payload: &serde_json::Value) -> (bool, String) {
    match http_client().post(url).json(payload).send() {
        Ok(response) if response.status().is_success() => {
            let body = response.text().unwrap_or_default();
            (true, body.trim().to_string())
        }
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            (
                false,
                if body.trim().is_empty() {
                    format!("Govee request failed with {status}")
                } else {
                    body.trim().to_string()
                },
            )
        }
        Err(err) => (false, format!("Govee request error: {err}")),
    }
}

/// Default "on" colour for OpenRGB devices when no profile exists.
const FALLBACK_COLOR: (u8, u8, u8) = (20, 0, 255);

/// Global master brightness level (1–100). Updated by the brightness slider.
/// Used by `scale_rgb` when applying colours from any source.
static MASTER_BRIGHTNESS: AtomicU8 = AtomicU8::new(100);

/// Read the current master brightness (for use from other modules).
pub fn current_brightness() -> u8 {
    MASTER_BRIGHTNESS.load(Ordering::Relaxed)
}

// ─── Response type ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterLightResult {
    pub power: bool,
    pub corsair_ok: bool,
    pub corsair_msg: String,
    pub openrgb_ok: bool,
    pub openrgb_msg: String,
    pub govee_ok: bool,
    pub govee_msg: String,
}

#[derive(Clone, Copy)]
struct GoveeTarget {
    name: &'static str,
    url: &'static str,
    color: Rgb,
}

impl GoveeTarget {
    const fn new(name: &'static str, url: &'static str, color: Rgb) -> Self {
        Self { name, url, color }
    }
}

#[derive(Clone, Copy)]
struct PcScene {
    corsair: Rgb,
    openrgb: Rgb,
}

#[derive(Clone, Copy)]
struct PcLightingResult {
    corsair_ok: bool,
    openrgb_ok: bool,
}

impl PcLightingResult {
    fn message(self) -> String {
        format!(
            "Corsair {} · OpenRGB {}",
            if self.corsair_ok { "✓" } else { "✗" },
            if self.openrgb_ok { "✓" } else { "✗" }
        )
    }

    fn any_ok(self) -> bool {
        self.corsair_ok || self.openrgb_ok
    }
}

// ─── Govee via Pi 5 (direct HTTP, no SSH) ───────────────────

fn govee_power(on: bool) -> (bool, String) {
    govee_post(GOVEE_PI5_POWER, &serde_json::json!({"power": on}))
}

fn govee_brightness(brightness: u8) -> (bool, String) {
    let br = brightness.clamp(1, 100);
    govee_post(GOVEE_PI5_BRIGHTNESS, &serde_json::json!({"brightness": br}))
}

fn govee_color(r: u8, g: u8, b: u8) -> (bool, String) {
    govee_post(GOVEE_PI5_COLOR, &serde_json::json!({"r": r, "g": g, "b": b}))
}

fn govee_device_color(url: &str, r: u8, g: u8, b: u8) -> (bool, String) {
    govee_post(url, &serde_json::json!({"r": r, "g": g, "b": b}))
}

fn resolve_govee_lamp(target: &str) -> Option<GoveeLamp> {
    let normalized = target.trim().to_lowercase();
    let normalized = normalized.as_str();

    let lamp_id = if ["deckenlampe", "decke", "deckenlicht", "ceiling", "ceiling1"]
        .contains(&normalized)
    {
        "deckenlampe"
    } else if [
        "deckenlampe2",
        "decke2",
        "deckenlicht2",
        "ceiling2",
        "zweite deckenlampe",
    ]
    .contains(&normalized)
    {
        "deckenlampe2"
    } else if ["stehlampe", "bodenlampe", "floor", "floorlamp"]
        .contains(&normalized)
    {
        "stehlampe"
    } else if normalized == "rachel" {
        "rachel"
    } else {
        return None;
    };

    GOVEE_LAMPS.iter().copied().find(|lamp| lamp.id == lamp_id)
}

fn apply_govee_scene(scene: &[GoveeTarget], scene_name: &str) -> (bool, String) {
    let mut ok = 0u32;
    let mut fail = 0u32;
    let mut failed_names = Vec::new();

    for target in scene {
        let (r, g, b) = target.color;
        if govee_device_color(target.url, r, g, b).0 {
            ok += 1;
        } else {
            fail += 1;
            failed_names.push(target.name);
        }
    }

    let failed_suffix = if failed_names.is_empty() {
        String::new()
    } else {
        format!(" ({})", failed_names.join(", "))
    };

    (
        ok > 0,
        format!("{scene_name}: {ok} applied, {fail} failed{failed_suffix}"),
    )
}

fn apply_openrgb_color(color: Rgb) -> bool {
    let (r, g, b) = color;
    let mut ok = 0u32;
    for id in OPENRGB_DEVICE_IDS {
        if openrgb::openrgb_set_color((*id).to_string(), r, g, b).is_ok() {
            ok += 1;
        }
    }
    ok > 0
}

fn apply_pc_scene(scene: PcScene) -> PcLightingResult {
    let corsair_color = apply_brightness(scene.corsair.0, scene.corsair.1, scene.corsair.2);
    let openrgb_color = apply_brightness(scene.openrgb.0, scene.openrgb.1, scene.openrgb.2);

    std::thread::scope(|s| {
        let corsair_handle = s.spawn(move || {
            corsair::corsair_set_rgb(corsair::SetRgbRequest {
                r: corsair_color.0,
                g: corsair_color.1,
                b: corsair_color.2,
            })
            .is_ok()
        });

        let openrgb_handle = s.spawn(move || {
            let _ = openrgb::openrgb_connect();
            apply_openrgb_color(openrgb_color)
        });

        PcLightingResult {
            corsair_ok: corsair_handle.join().unwrap_or(false),
            openrgb_ok: openrgb_handle.join().unwrap_or(false),
        }
    })
}

// ─── Tauri command ───────────────────────────────────────────

/// Master power: ON restores saved Corsair profile color + OpenRGB fallback,
/// OFF sets everything to black. Does NOT change colors — purely on/off.
///
/// Runs Corsair, OpenRGB, and Govee in parallel to avoid blocking the UI.
#[command]
pub fn lighting_master_power(power: bool) -> MasterLightResult {
    // Run all three subsystems in parallel — they use independent locks.
    std::thread::scope(|s| {
        // 1. Corsair CCXT (separate thread — contends with NEXUS refresh)
        let corsair_handle = s.spawn(|| {
            let (r, g, b) = if power {
                corsair::load_profile_rgb().unwrap_or(FALLBACK_COLOR)
            } else {
                (0, 0, 0)
            };
            match corsair::corsair_set_rgb(corsair::SetRgbRequest { r, g, b }) {
                Ok(msg) => (true, msg),
                Err(e) => (false, e),
            }
        });

        // 2. OpenRGB (separate thread — single manager() lock)
        let openrgb_handle = s.spawn(|| {
            // Always ensure devices are connected first
            let _ = openrgb::openrgb_connect();

            if power {
                // Try to restore saved per-device states from profile
                let saved = corsair::load_profile_openrgb();
                if !saved.is_empty() {
                    openrgb::apply_openrgb_state(&saved);
                    let n = saved.len();
                    (true, format!("{n} Geräte aus Profil wiederhergestellt"))
                } else {
                    // No profile — fall back to default colour on ALL devices
                    let (r, g, b) = FALLBACK_COLOR;
                    let mut ok = 0u32;
                    let mut fail = 0u32;
                    for id in OPENRGB_DEVICE_IDS {
                        match openrgb::openrgb_set_color((*id).to_string(), r, g, b) {
                            Ok(_) => ok += 1,
                            Err(_) => fail += 1,
                        }
                    }
                    (ok > 0, format!("{ok} an, {fail} nicht erreichbar"))
                }
            } else {
                match openrgb::openrgb_all_off() {
                    Ok(msg) => (true, msg),
                    Err(e) => (false, e),
                }
            }
        });

        // 3. Govee via Pi 5 HTTP (separate thread — I/O only, no locks)
        let govee_handle = s.spawn(|| govee_power(power));

        let (corsair_ok, corsair_msg) = corsair_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));
        let (openrgb_ok, openrgb_msg) = openrgb_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));
        let (govee_ok, govee_msg) = govee_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));

        MasterLightResult {
            power,
            corsair_ok,
            corsair_msg,
            openrgb_ok,
            openrgb_msg,
            govee_ok,
            govee_msg,
        }
    })
}

/// Set brightness for all Govee devices via Pi 5.
#[command]
pub fn govee_master_brightness(brightness: u8) -> (bool, String) {
    govee_brightness(brightness)
}

#[command]
pub fn govee_master_color(r: u8, g: u8, b: u8) -> (bool, String) {
    govee_color(r, g, b)
}

#[command]
pub fn govee_lamp_color(target: String, r: u8, g: u8, b: u8) -> (bool, String) {
    match resolve_govee_lamp(&target) {
        Some(lamp) => {
            let (ok, msg) = govee_device_color(lamp.url, r, g, b);
            (ok, format!("{} RGB ({r},{g},{b}): {msg}", lamp.id))
        }
        None => (
            false,
            format!(
                "Unknown Govee lamp '{target}'. Valid targets: deckenlampe, deckenlampe2, stehlampe, rachel"
            ),
        ),
    }
}

/// Govee-only power: turn all Govee lights on or off.
#[command]
pub fn govee_master_power(power: bool) -> (bool, String) {
    govee_power(power)
}

pub fn govee_master_purple() -> (bool, String) {
    apply_govee_scene(PURPLE_GOVEE_SCENE, "Govee purple")
}

pub fn rgb_master_purple() -> (bool, String) {
    let result = apply_pc_scene(PURPLE_PC_SCENE);
    (result.any_ok(), result.message())
}

pub fn lighting_master_purple() -> MasterLightResult {
    std::thread::scope(|s| {
        let pc_handle = s.spawn(|| apply_pc_scene(PURPLE_PC_SCENE));
        let govee_handle = s.spawn(|| apply_govee_scene(PURPLE_GOVEE_SCENE, "Govee purple"));

        let pc_result = pc_handle.join().unwrap_or(PcLightingResult {
            corsair_ok: false,
            openrgb_ok: false,
        });
        let pc_msg = pc_result.message();
        let (govee_ok, govee_msg) = govee_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));

        MasterLightResult {
            power: true,
            corsair_ok: pc_result.corsair_ok,
            corsair_msg: pc_msg.clone(),
            openrgb_ok: pc_result.openrgb_ok,
            openrgb_msg: pc_msg,
            govee_ok,
            govee_msg,
        }
    })
}

/// Set the same RGB colour on all lights, including Govee.
#[command]
pub fn lighting_master_color(r: u8, g: u8, b: u8) -> MasterLightResult {
    let (sr, sg, sb) = apply_brightness(r, g, b);

    std::thread::scope(|s| {
        let corsair_handle = s.spawn(move || {
            match corsair::corsair_set_rgb(corsair::SetRgbRequest {
                r: sr,
                g: sg,
                b: sb,
            }) {
                Ok(msg) => (true, msg),
                Err(e) => (false, e),
            }
        });

        let openrgb_handle = s.spawn(move || {
            let _ = openrgb::openrgb_connect();
            let mut ok = 0u32;
            let mut fail = 0u32;
            for id in OPENRGB_DEVICE_IDS {
                match openrgb::openrgb_set_color((*id).to_string(), sr, sg, sb) {
                    Ok(_) => ok += 1,
                    Err(_) => fail += 1,
                }
            }
            (ok > 0, format!("{ok} gesetzt, {fail} fehlgeschlagen"))
        });

        let govee_handle = s.spawn(move || govee_color(r, g, b));

        let (corsair_ok, corsair_msg) = corsair_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));
        let (openrgb_ok, openrgb_msg) = openrgb_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));
        let (govee_ok, govee_msg) = govee_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));

        MasterLightResult {
            power: true,
            corsair_ok,
            corsair_msg,
            openrgb_ok,
            openrgb_msg,
            govee_ok,
            govee_msg,
        }
    })
}

/// PC RGB brightness: Corsair + OpenRGB only (no Govee).
/// Stores the level and scales colours. Does NOT touch Govee.
#[command]
pub fn rgb_master_brightness(brightness: u8) -> (bool, String) {
    let br = brightness.clamp(1, 100);
    MASTER_BRIGHTNESS.store(br, Ordering::Relaxed);
    let rgb_br = br.max(MIN_RGB_BRIGHTNESS);

    std::thread::scope(|s| {
        let corsair_handle = s.spawn(move || {
            let (r, g, b) = corsair::load_profile_rgb().unwrap_or(FALLBACK_COLOR);
            let (sr, sg, sb) = scale_rgb(r, g, b, rgb_br);
            corsair::corsair_set_rgb(corsair::SetRgbRequest {
                r: sr,
                g: sg,
                b: sb,
            })
            .is_ok()
        });

        let openrgb_handle = s.spawn(move || {
            let _ = openrgb::openrgb_connect();
            let saved = corsair::load_profile_openrgb();
            if !saved.is_empty() {
                let mut ok = 0u32;
                for st in &saved {
                    let (sr, sg, sb) = scale_rgb(st.color.r, st.color.g, st.color.b, rgb_br);
                    if openrgb::openrgb_set_color(st.id.clone(), sr, sg, sb).is_ok() {
                        ok += 1;
                    }
                }
                ok > 0
            } else {
                let (r, g, b) = FALLBACK_COLOR;
                let (sr, sg, sb) = scale_rgb(r, g, b, rgb_br);
                let mut ok = 0u32;
                for id in OPENRGB_DEVICE_IDS {
                    if openrgb::openrgb_set_color((*id).to_string(), sr, sg, sb).is_ok() {
                        ok += 1;
                    }
                }
                ok > 0
            }
        });

        let result = PcLightingResult {
            corsair_ok: corsair_handle.join().unwrap_or(false),
            openrgb_ok: openrgb_handle.join().unwrap_or(false),
        };
        (result.any_ok(), format!("{rgb_br}%: {}", result.message()))
    })
}

/// PC RGB power: Corsair + OpenRGB only (no Govee).
#[command]
pub fn rgb_master_power(power: bool) -> (bool, String) {
    std::thread::scope(|s| {
        let corsair_handle = s.spawn(|| {
            let (r, g, b) = if power {
                corsair::load_profile_rgb().unwrap_or(FALLBACK_COLOR)
            } else {
                (0, 0, 0)
            };
            corsair::corsair_set_rgb(corsair::SetRgbRequest { r, g, b }).is_ok()
        });

        let openrgb_handle = s.spawn(|| {
            let _ = openrgb::openrgb_connect();
            if power {
                let saved = corsair::load_profile_openrgb();
                if !saved.is_empty() {
                    openrgb::apply_openrgb_state(&saved);
                    true
                } else {
                    let (r, g, b) = FALLBACK_COLOR;
                    let mut ok = 0u32;
                    for id in OPENRGB_DEVICE_IDS {
                        if openrgb::openrgb_set_color((*id).to_string(), r, g, b).is_ok() {
                            ok += 1;
                        }
                    }
                    ok > 0
                }
            } else {
                openrgb::openrgb_all_off().is_ok()
            }
        });

        let result = PcLightingResult {
            corsair_ok: corsair_handle.join().unwrap_or(false),
            openrgb_ok: openrgb_handle.join().unwrap_or(false),
        };
        (result.any_ok(), result.message())
    })
}

// ─── Helper: scale an RGB colour by a brightness percentage ──

/// Minimum effective brightness for RGB-based devices (Corsair/OpenRGB).
/// Hardware LEDs can't display very dim colours; below ~10% they appear off.
const MIN_RGB_BRIGHTNESS: u8 = 10;

fn scale_rgb(r: u8, g: u8, b: u8, pct: u8) -> (u8, u8, u8) {
    let f = (pct.min(100) as u16).max(1);
    (
        ((r as u16) * f / 100) as u8,
        ((g as u16) * f / 100) as u8,
        ((b as u16) * f / 100) as u8,
    )
}

/// Scale an RGB colour by the current master brightness.
/// Applies a floor of MIN_RGB_BRIGHTNESS so hardware LEDs stay visible.
/// Called from other modules (corsair, openrgb) when applying user-chosen colours.
pub fn apply_brightness(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let br = current_brightness().max(MIN_RGB_BRIGHTNESS);
    scale_rgb(r, g, b, br)
}

/// Master brightness: stores the level, scales Corsair + OpenRGB colours,
/// and sets Govee brightness. Runs all three subsystems in parallel.
#[command]
pub fn lighting_master_brightness(brightness: u8) -> MasterLightResult {
    let br = brightness.clamp(1, 100);
    MASTER_BRIGHTNESS.store(br, Ordering::Relaxed);

    // Corsair / OpenRGB get a floor so the LEDs stay visible
    let rgb_br = br.max(MIN_RGB_BRIGHTNESS);

    std::thread::scope(|s| {
        // 1. Corsair — scale profile colour
        let corsair_handle = s.spawn(move || {
            let (r, g, b) = corsair::load_profile_rgb().unwrap_or(FALLBACK_COLOR);
            let (sr, sg, sb) = scale_rgb(r, g, b, rgb_br);
            match corsair::corsair_set_rgb(corsair::SetRgbRequest {
                r: sr,
                g: sg,
                b: sb,
            }) {
                Ok(msg) => (true, msg),
                Err(e) => (false, e),
            }
        });

        // 2. OpenRGB — scale each saved device colour
        let openrgb_handle = s.spawn(move || {
            let _ = openrgb::openrgb_connect();
            let saved = corsair::load_profile_openrgb();
            if !saved.is_empty() {
                let mut ok = 0u32;
                let mut fail = 0u32;
                for st in &saved {
                    let (sr, sg, sb) = scale_rgb(st.color.r, st.color.g, st.color.b, rgb_br);
                    match openrgb::openrgb_set_color(st.id.clone(), sr, sg, sb) {
                        Ok(_) => ok += 1,
                        Err(_) => fail += 1,
                    }
                }
                (ok > 0, format!("{ok} skaliert, {fail} fehlgeschlagen"))
            } else {
                let (r, g, b) = FALLBACK_COLOR;
                let (sr, sg, sb) = scale_rgb(r, g, b, rgb_br);
                let mut ok = 0u32;
                let mut fail = 0u32;
                for id in OPENRGB_DEVICE_IDS {
                    match openrgb::openrgb_set_color((*id).to_string(), sr, sg, sb) {
                        Ok(_) => ok += 1,
                        Err(_) => fail += 1,
                    }
                }
                (ok > 0, format!("{ok} skaliert, {fail} fehlgeschlagen"))
            }
        });

        // 3. Govee — native brightness
        let govee_handle = s.spawn(move || govee_brightness(br));

        let (corsair_ok, corsair_msg) = corsair_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));
        let (openrgb_ok, openrgb_msg) = openrgb_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));
        let (govee_ok, govee_msg) = govee_handle
            .join()
            .unwrap_or((false, "Thread panic".into()));

        MasterLightResult {
            power: true,
            corsair_ok,
            corsair_msg,
            openrgb_ok,
            openrgb_msg,
            govee_ok,
            govee_msg,
        }
    })
}
