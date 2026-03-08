//! Corsair HID protocol constants, domain newtypes, and fan-curve logic.
//!
//! All byte sequences derived from OpenLinkHub (MIT, jurkovic-nikola).

#![allow(dead_code)] // Protocol surface — constants used as features expand.

use serde::{Deserialize, Serialize};

// ─── USB Identifiers ────────────────────────────────────────

pub const CORSAIR_VENDOR_ID: u16 = 0x1B1C;
pub const CCXT_PRODUCT_ID: u16 = 3114; // Commander Core XT
pub const NEXUS_PRODUCT_ID: u16 = 7054; // iCUE NEXUS

// ─── CCXT Buffer Geometry ───────────────────────────────────

pub const CCXT_BUF_SIZE: usize = 384;
pub const CCXT_BUF_WRITE: usize = CCXT_BUF_SIZE + 1; // +1 HID report-ID
/// Offset where the endpoint/command bytes start in the HID write buffer.
/// `bufferW[0]` = HID report ID, `bufferW[1]` = `CCXT_CMD_BYTE`, `bufferW[2..]` = endpoint.
pub const CCXT_HEADER_SIZE: usize = 2;
/// The data-payload header used by the higher-level `write()` helper
/// (length prefix + padding) — **not** the HID packet header.
pub const CCXT_DATA_HEADER_SIZE: usize = 4;
/// Fixed command byte placed at `bufferW[1]` in every HID packet.
pub const CCXT_CMD_BYTE: u8 = 0x08;

// ─── CCXT HID Commands ─────────────────────────────────────

pub const CMD_SOFTWARE_MODE: &[u8] = &[0x01, 0x03, 0x00, 0x02];
pub const CMD_HARDWARE_MODE: &[u8] = &[0x01, 0x03, 0x00, 0x01];
pub const CMD_GET_FIRMWARE: &[u8] = &[0x02, 0x13];
pub const CMD_CLOSE_ENDPOINT: &[u8] = &[0x05, 0x01, 0x01];
pub const CMD_WRITE: &[u8] = &[0x06, 0x01];
pub const CMD_WRITE_COLOR: &[u8] = &[0x06, 0x00];
pub const CMD_WRITE_COLOR_NEXT: &[u8] = &[0x07, 0x00];
pub const CMD_READ: &[u8] = &[0x08, 0x01];
pub const CMD_OPEN_ENDPOINT: &[u8] = &[0x0d, 0x01];
pub const CMD_OPEN_COLOR_ENDPOINT: &[u8] = &[0x0d, 0x00];

/// Initialize a single LED port for detection.  Send for ports 1–6 before
/// reading `MODE_GET_LEDS`.  Matches OpenLinkHub `initLedPorts()`.
pub const CMD_INIT_LED_PORT: u8 = 0x14;
/// Configure which LED device type is on each port (0x1e).
pub const CMD_SET_LED_PORTS: u8 = 0x1e;
/// Reset LED power after port configuration (0x15, 0x01).
pub const CMD_RESET_LED_POWER: &[u8] = &[0x15, 0x01];

// ─── CCXT Data Modes ────────────────────────────────────────

pub const MODE_GET_FANS: &[u8] = &[0x1a];
pub const MODE_GET_SPEEDS: &[u8] = &[0x17];
pub const MODE_SET_SPEED: &[u8] = &[0x18];
pub const MODE_GET_TEMPS: &[u8] = &[0x21];
pub const MODE_SET_COLOR: &[u8] = &[0x22];
pub const MODE_GET_LEDS: &[u8] = &[0x20];

pub const DATA_TYPE_SET_SPEED: &[u8] = &[0x07, 0x00];
pub const DATA_TYPE_SET_COLOR: &[u8] = &[0x12, 0x00];

/// Channel status byte indicating a fan is connected.
pub const CHANNEL_FAN_CONNECTED: u8 = 0x07;

/// Max payload bytes per single HID transfer (buffer minus header minus command).
pub const CCXT_MAX_PAYLOAD: usize = CCXT_BUF_WRITE - CCXT_HEADER_SIZE - 2;

// ─── NEXUS Constants ────────────────────────────────────────

pub const NEXUS_IMG_WIDTH: u32 = 640;
pub const NEXUS_IMG_HEIGHT: u32 = 48;
pub const NEXUS_LCD_BUF_SIZE: usize = 1024;
pub const NEXUS_LCD_HEADER: usize = 8;
pub const NEXUS_LCD_MAX_PAYLOAD: usize = NEXUS_LCD_BUF_SIZE - NEXUS_LCD_HEADER;

/// Feature reports that restore NEXUS hardware mode on disconnect.
pub const NEXUS_LCD_STOP_REPORTS: &[&[u8]] =
    &[&[0x03, 0x0d, 0x01, 0x01], &[0x03, 0x01, 0x64, 0x01]];

// ═════════════════════════════════════════════════════════════
//  Domain Newtypes — zero-cost type safety
// ═════════════════════════════════════════════════════════════

/// Fan speed percentage. Invariant: `0 ≤ inner ≤ 100`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct SpeedPct(u8);

impl SpeedPct {
    pub const MIN: Self = Self(0);
    pub const MAX: Self = Self(100);
    pub const DEFAULT: Self = Self(50);

    /// Construct with automatic clamping to 0–100.
    #[must_use]
    pub const fn new(raw: u8) -> Self {
        // const-compatible: no method calls, just a ternary
        Self(if raw > 100 { 100 } else { raw })
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<u8> for SpeedPct {
    fn from(v: u8) -> Self {
        Self::new(v)
    }
}

impl Default for SpeedPct {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Temperature in °C.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Celsius(f32);

impl Celsius {
    #[must_use]
    pub const fn new(val: f32) -> Self {
        Self(val)
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Decode from the CCXT wire format: little-endian u16, unit = 0.1 °C.
    #[must_use]
    pub fn from_raw_le(bytes: [u8; 2]) -> Self {
        Self(f32::from(u16::from_le_bytes(bytes)) / 10.0)
    }
}

impl Default for Celsius {
    fn default() -> Self {
        Self(0.0)
    }
}

impl std::fmt::Display for Celsius {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1}°C", self.0)
    }
}

// ═════════════════════════════════════════════════════════════
//  FanMode — single source of truth per channel
// ═════════════════════════════════════════════════════════════

/// Per-channel fan operating mode.
///
/// Replaces the anti-pattern of separate `manual_speeds: Vec<Option<u8>>`
/// and `fan_curves: Vec<Vec<FanCurvePoint>>`.  The enum is the single
/// source of truth, no ambiguity about which field wins.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FanMode {
    /// Fixed speed override.
    Fixed { speed: SpeedPct },
    /// Temperature-reactive curve.
    Curve { points: Vec<FanCurvePoint> },
}

impl FanMode {
    /// Resolve the target speed for the current temperature.
    #[must_use]
    pub fn resolve(&self, temp: Celsius) -> SpeedPct {
        match self {
            Self::Fixed { speed } => *speed,
            Self::Curve { points } => interpolate_speed(points, temp),
        }
    }
}

impl Default for FanMode {
    fn default() -> Self {
        Self::Curve {
            points: default_fan_curve(),
        }
    }
}

// ─── Fan Curve ──────────────────────────────────────────────

/// A single temperature → speed mapping point on a fan curve.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FanCurvePoint {
    pub temp: Celsius,
    pub speed: SpeedPct,
}

/// Default "silent" fan curve.
#[must_use]
pub fn default_fan_curve() -> Vec<FanCurvePoint> {
    [
        (25.0, 25),
        (35.0, 30),
        (45.0, 45),
        (55.0, 60),
        (65.0, 80),
        (75.0, 100),
    ]
    .into_iter()
    .map(|(t, s)| FanCurvePoint {
        temp: Celsius(t),
        speed: SpeedPct::new(s),
    })
    .collect()
}

/// Linear interpolation along a sorted fan curve.
#[must_use]
pub fn interpolate_speed(curve: &[FanCurvePoint], temp: Celsius) -> SpeedPct {
    if curve.is_empty() {
        return SpeedPct::DEFAULT;
    }

    let t = temp.get();

    // Below lowest point
    if t <= curve[0].temp.get() {
        return curve[0].speed;
    }

    // Above highest point
    if let Some(last) = curve.last() {
        if t >= last.temp.get() {
            return last.speed;
        }
    }

    // Linear interpolation between surrounding points
    for pair in curve.windows(2) {
        let (lo, hi) = (&pair[0], &pair[1]);
        let (lo_t, hi_t) = (lo.temp.get(), hi.temp.get());
        if t >= lo_t && t <= hi_t {
            let range = hi_t - lo_t;
            if range < f32::EPSILON {
                return lo.speed;
            }
            let ratio = (t - lo_t) / range;
            let s = f32::from(lo.speed.get())
                + ratio * (f32::from(hi.speed.get()) - f32::from(lo.speed.get()));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            return SpeedPct::new(s.round().clamp(0.0, 100.0) as u8);
        }
    }

    SpeedPct::DEFAULT
}

// ─── Response Parsing ───────────────────────────────────────

/// The CCXT encodes channel count at response byte offset 5.
#[must_use]
pub fn parse_channel_count(resp: &[u8]) -> usize {
    resp.get(5).copied().map_or(0, usize::from)
}

/// Per-port LED detection result.
#[derive(Debug, Clone, Copy)]
pub struct LedPort {
    pub connected: bool,
    pub led_count: u16,
    /// Device-type command byte for `CMD_SET_LED_PORTS` (derived from LED count).
    pub command: u8,
}

/// Map an LED count to the CCXT device-type command byte
/// (matches OpenLinkHub `getLedDevices()` switch-case).
fn led_count_to_command(count: u16) -> u8 {
    match count {
        4 => 0x03,  // ML PRO RGB Series Fan
        8 => 0x05,  // 8-LED Series Fan
        10 => 0x01, // RGB Led Strip
        12 => 0x04, // HD RGB Series Fan
        16 => 0x02, // LL RGB Series Fan
        34 => 0x06, // QL RGB Series Fan
        _ => 0x00,  // unknown → treat as inactive
    }
}

/// Parse total LED count from a `MODE_GET_LEDS` response.
///
/// CCXT has 6 internal LED ports. Data starts at byte 10, each port
/// occupying 4 bytes: `[status_lo, status_hi, led_count_lo, led_count_hi]`.
/// Status `2` means a device is connected.
#[must_use]
pub fn parse_led_ports(resp: &[u8]) -> ([LedPort; 6], usize) {
    const LED_START: usize = 10;
    const PORT_COUNT: usize = 6;
    let mut ports = [LedPort {
        connected: false,
        led_count: 0,
        command: 0,
    }; 6];
    let mut total: usize = 0;
    for i in 0..PORT_COUNT {
        let off = LED_START + i * 4;
        if off + 4 > resp.len() {
            break;
        }
        let status = u16::from_le_bytes([resp[off], resp[off + 1]]);
        let count = u16::from_le_bytes([resp[off + 2], resp[off + 3]]);
        let connected = status == 2;
        let command = if connected {
            led_count_to_command(count)
        } else {
            0
        };
        log::info!("CCXT LED port {i}: status={status} leds={count} cmd=0x{command:02x} connected={connected}");
        ports[i] = LedPort {
            connected,
            led_count: count,
            command,
        };
        if connected {
            total = total.saturating_add(usize::from(count));
        }
    }
    (ports, total)
}

/// Parse `major.minor.patch` from a `CMD_GET_FIRMWARE` response.
/// Offsets: `resp[3]=major`, `resp[4]=minor`, `resp[5..7]=patch (LE u16)`.
#[must_use]
pub fn parse_firmware(resp: &[u8]) -> String {
    if resp.len() >= 7 {
        let patch = u16::from_le_bytes([resp[5], resp[6]]);
        format!("{}.{}.{patch}", resp[3], resp[4])
    } else {
        "unknown".to_string()
    }
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_pct_clamps_to_100() {
        assert_eq!(SpeedPct::new(200).get(), 100);
        assert_eq!(SpeedPct::new(0).get(), 0);
        assert_eq!(SpeedPct::new(50).get(), 50);
    }

    #[test]
    fn celsius_from_raw_le_parses_correctly() {
        // 500 in 0.1°C = 50.0°C
        let c = Celsius::from_raw_le(500u16.to_le_bytes());
        assert!((c.get() - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fan_mode_default_is_curve() {
        assert!(matches!(FanMode::default(), FanMode::Curve { .. }));
    }

    #[test]
    fn fan_mode_resolve_fixed_ignores_temp() {
        let mode = FanMode::Fixed {
            speed: SpeedPct::new(42),
        };
        assert_eq!(mode.resolve(Celsius(99.0)).get(), 42);
    }

    #[test]
    fn fan_mode_resolve_curve_interpolates() {
        let mode = FanMode::default();
        // 40°C is between 35°C (30%) and 45°C (45%) → ~37–38%
        let speed = mode.resolve(Celsius(40.0));
        assert!(
            speed.get() >= 37 && speed.get() <= 38,
            "got {}",
            speed.get()
        );
    }

    #[test]
    fn interpolate_below_range() {
        assert_eq!(
            interpolate_speed(&default_fan_curve(), Celsius(10.0)).get(),
            25
        );
    }

    #[test]
    fn interpolate_above_range() {
        assert_eq!(
            interpolate_speed(&default_fan_curve(), Celsius(90.0)).get(),
            100
        );
    }

    #[test]
    fn interpolate_empty_returns_default() {
        assert_eq!(interpolate_speed(&[], Celsius(50.0)), SpeedPct::DEFAULT);
    }
}
