//! Direct USB HID RGB control — no OpenRGB server needed.
//!
//! Drives all non-Corsair RGB devices directly via `hidapi`:
//!
//! | Device                        | VID:PID     | Protocol            |
//! |-------------------------------|-------------|---------------------|
//! | Gigabyte IT8297 (Z690 UD)     | 048d:5702   | RGB Fusion 2        |
//! | Corsair K70 TKL Champion      | 1b1c:1bb9   | Corsair V2          |
//! | SteelSeries Aerox 3           | 1038:1836   | SS Aerox            |
//! | SteelSeries QCK Prism Cloth   | 1038:150d   | SS QCK              |
//! | XPG SPECTRIX S40G NVMe        | 1cc1:5762   | ENE NVMe Passthrough|
//! | XPG SPECTRIX S20G (Pi5)       | remote/SSH  | xpg-rgb CLI on Pi5  |
//!
//! Corsair Commander Core XT + iCUE NEXUS are in the `corsair` module.

#![allow(clippy::needless_pass_by_value)]

use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Mutex;
use tauri::command;

// ═══════════════════════════════════════════════════════════════
//  Common Types
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
}

/// Unified error type for all RGB drivers.
#[derive(Debug)]
enum RgbError {
    Hid(String),
    NotConnected,
    Protocol(String),
}

impl fmt::Display for RgbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hid(msg) => write!(f, "HID: {msg}"),
            Self::NotConnected => write!(f, "Nicht verbunden"),
            Self::Protocol(msg) => write!(f, "Protokoll: {msg}"),
        }
    }
}

impl From<hidapi::HidError> for RgbError {
    fn from(e: hidapi::HidError) -> Self {
        Self::Hid(e.to_string())
    }
}

// ═══════════════════════════════════════════════════════════════
//  Effect Types (shared by IT8297 + frontend)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RgbEffect {
    Off,
    Static,
    Pulse,
    Blinking,
    ColorCycle,
    Wave,
    Random,
}

impl RgbEffect {
    fn it8297_id(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Static => 1,
            Self::Pulse => 2,
            Self::Blinking => 3,
            Self::ColorCycle => 4,
            Self::Wave => 6,
            Self::Random => 8,
        }
    }

    /// Index matching the `effects` array used by `openrgb_set_mode`.
    pub fn mode_index(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Static => 1,
            Self::Pulse => 2,
            Self::Blinking => 3,
            Self::ColorCycle => 4,
            Self::Wave => 5,
            Self::Random => 6,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
//  IT8297 / RGB Fusion 2 Driver
// ═══════════════════════════════════════════════════════════════

mod it8297 {
    use super::*;

    pub const VID: u16 = 0x048D;
    pub const PID: u16 = 0x5702;
    const REPORT_ID: u8 = 0xCC;
    const BUF_SIZE: usize = 64;

    use std::cell::Cell;

    /// Zone layout for a Gigabyte motherboard.
    #[derive(Debug, Clone, Serialize)]
    pub struct ZoneInfo {
        pub idx: u8,
        pub name: String,
        pub header: u8,
        pub is_digital: bool,
        /// Configured LED count for digital strips; 1 for onboard zones.
        pub led_count: u32,
    }

    // LED count enum values (matches OpenRGB LEDCount)
    fn led_count_to_enum(count: u32) -> u8 {
        if count <= 32 {
            0
        }
        // LEDS_32
        else if count <= 64 {
            1
        }
        // LEDS_64
        else if count <= 256 {
            2
        }
        // LEDS_256
        else if count <= 512 {
            3
        }
        // LEDS_512
        else {
            4
        } // LEDS_1024
    }

    /// IT8297 device state.
    pub struct It8297 {
        dev: HidDevice,
        pub product_name: String,
        pub fw_version: u32,
        pub device_num: u8,
        pub zones: Vec<ZoneInfo>,
        /// Tracks the 0x32 "effect_disabled" bitmask (which strip headers have
        /// built-in effects disabled). Bits: 0=D_LED1, 1=D_LED2, 3=D_LED3, 4=D_LED4.
        /// 0x1B = all disabled, 0x00 = all enabled.
        effect_disabled: Cell<u8>,
        /// Calibration byte order for D_LED1: (R_offset, G_offset, B_offset)
        /// within each 3-byte LED block in PktRGB packets.
        d_led1_cal: (u8, u8, u8),
        /// Calibration byte order for D_LED2.
        d_led2_cal: (u8, u8, u8),
    }

    impl It8297 {
        /// Open the device, read hardware info, build zone map.
        pub fn connect() -> Result<Self, RgbError> {
            let api = HidApi::new()?;
            let dev = api.open(VID, PID)?;
            dev.set_blocking_mode(true)?;
            eprintln!("IT8297: HID opened (048d:5702)");

            let mut this = Self {
                dev,
                product_name: String::new(),
                fw_version: 0,
                device_num: 0,
                zones: Vec::new(),
                effect_disabled: Cell::new(0x1B), // all disabled initially
                d_led1_cal: (1, 0, 2),            // default GRB (WS2812B standard)
                d_led2_cal: (1, 0, 2),
            };

            this.read_hw_info()?;
            this.init()?;
            Ok(this)
        }

        /// Read the IT8297Report (cmd 0x60).
        fn read_hw_info(&mut self) -> Result<(), RgbError> {
            self.send_cc(0x60, 0, 0)?;

            let mut buf = [0u8; BUF_SIZE];
            buf[0] = REPORT_ID; // get_feature_report needs report ID at [0]
            let n = self.dev.get_feature_report(&mut buf)?;
            if n < 16 {
                return Err(RgbError::Protocol(format!(
                    "IT8297Report zu kurz ({n} bytes)"
                )));
            }

            self.device_num = buf[2];
            self.fw_version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);

            // Product name at offset 12..40
            let name_end = n.min(40);
            let name_bytes = &buf[12..name_end];
            let end = name_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(name_bytes.len());
            self.product_name = String::from_utf8_lossy(&name_bytes[..end]).to_string();

            eprintln!(
                "IT8297: '{}', FW={}, device_num={}, report_bytes={}",
                self.product_name, self.fw_version, self.device_num, n
            );
            eprintln!("IT8297: raw report header: {:02x?}", &buf[..n.min(16)]);
            if n >= 52 {
                eprintln!(
                    "IT8297: raw calibration bytes [44..52]: {:02x?}",
                    &buf[44..52]
                );
            }

            // Parse calibration data for D_LED strip byte order (offsets 44-51)
            fn parse_cal(val: u32) -> (u8, u8, u8) {
                if val == 0 {
                    // Calibration "OFF" → default GRB (WS2812B standard)
                    (1, 0, 2)
                } else {
                    let bo_r = ((val >> 16) & 0xFF) as u8;
                    let bo_g = ((val >> 8) & 0xFF) as u8;
                    let bo_b = (val & 0xFF) as u8;
                    (bo_r, bo_g, bo_b)
                }
            }
            if n >= 48 {
                let cal0 = u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]);
                self.d_led1_cal = parse_cal(cal0);
                eprintln!(
                    "IT8297: cal_strip0=0x{:08x} → byte_order={:?}",
                    cal0, self.d_led1_cal
                );
            }
            if n >= 52 {
                let cal1 = u32::from_le_bytes([buf[48], buf[49], buf[50], buf[51]]);
                self.d_led2_cal = parse_cal(cal1);
                eprintln!(
                    "IT8297: cal_strip1=0x{:08x} → byte_order={:?}",
                    cal1, self.d_led2_cal
                );
            }

            self.build_zones();
            Ok(())
        }

        /// Build zone map from device_num.
        fn build_zones(&mut self) {
            self.zones.clear();

            // Onboard accent LEDs — always add zone 0 for motherboard RGB
            // Zone 0 is typically the main accent LED / Back I/O
            self.zones.push(ZoneInfo {
                idx: 0,
                name: "Back I/O".into(),
                header: 0x20,
                is_digital: false,
                led_count: 1,
            });

            if self.device_num >= 2 {
                self.zones.push(ZoneInfo {
                    idx: 1,
                    name: "CPU Header".into(),
                    header: 0x21,
                    is_digital: false,
                    led_count: 1,
                });
            }
            if self.device_num >= 3 {
                self.zones.push(ZoneInfo {
                    idx: 2,
                    name: "LED C1C2".into(),
                    header: 0x22,
                    is_digital: false,
                    led_count: 1,
                });
            }
            if self.device_num >= 4 {
                self.zones.push(ZoneInfo {
                    idx: 3,
                    name: "PCIe".into(),
                    header: 0x23,
                    is_digital: false,
                    led_count: 1,
                });
            }
            if self.device_num >= 5 {
                self.zones.push(ZoneInfo {
                    idx: 4,
                    name: "12V RGB Strip".into(),
                    header: 0x24,
                    is_digital: false,
                    led_count: 1,
                });
            }

            // Digital LED headers — always present
            // Default 60 LEDs per strip (common for 1m WS2812B strips).
            // User can configure via set_strip_led_count command.
            self.zones.push(ZoneInfo {
                idx: 5,
                name: "D_LED1".into(),
                header: 0x25,
                is_digital: true,
                led_count: 60,
            });
            self.zones.push(ZoneInfo {
                idx: 6,
                name: "D_LED2".into(),
                header: 0x26,
                is_digital: true,
                led_count: 60,
            });

            eprintln!(
                "IT8297: {} zones created (device_num={})",
                self.zones.len(),
                self.device_num
            );
        }

        /// Initialise controller — matches OpenRGB's constructor + SetupZones.
        fn init(&mut self) -> Result<(), RgbError> {
            // 1. Reset all zone registers (0x20..0x27) — OpenRGB ResetController()
            for reg in 0x20u8..=0x27 {
                self.send_cc(reg, 0, 0)?;
            }
            // Fast-apply after reset (as OpenRGB does)
            self.send_cc(0x28, 0xFF, 0)?;
            std::thread::sleep(std::time::Duration::from_millis(20));

            // 2. Disable beat/audio sync mode
            self.send_cc(0x31, 0, 0)?;

            // 3. Set D_LED strip LED counts (cmd 0x34)
            //    byte2 = (D_LED2_enum << 4) | D_LED1_enum
            //    byte3 = (D_LED4_enum << 4) | D_LED3_enum
            //    Must match actual LED count per strip (OpenRGB's SetLedCount)
            let d1 = self
                .zones
                .iter()
                .find(|z| z.idx == 5)
                .map(|z| z.led_count)
                .unwrap_or(0);
            let d2 = self
                .zones
                .iter()
                .find(|z| z.idx == 6)
                .map(|z| z.led_count)
                .unwrap_or(0);
            let d1_enum = led_count_to_enum(d1);
            let d2_enum = led_count_to_enum(d2);
            let led_byte = (d2_enum << 4) | d1_enum;
            self.send_cc(0x34, led_byte, 0x00)?;
            eprintln!(
                "IT8297: SetLedCount 0x34, 0x{:02x} (D_LED1={}→enum{}, D_LED2={}→enum{})",
                led_byte, d1, d1_enum, d2, d2_enum
            );

            // 4. Disable built-in effects on all strips initially (OpenRGB SetupZones)
            //    0x1B = bits 0|1|3|4 = all strip headers disabled
            self.effect_disabled.set(0x1B);
            self.send_cc(0x32, 0x1B, 0)?;
            std::thread::sleep(std::time::Duration::from_millis(50));

            eprintln!("IT8297: init done (reset, beat off, D_LED counts set, built-in disabled)");
            Ok(())
        }

        /// Set a zone to a hardware effect with a color.
        pub fn set_zone_effect(
            &self,
            zone_idx: u8,
            effect: RgbEffect,
            color: RgbColor,
            speed: u8,
            brightness: u8,
        ) -> Result<(), RgbError> {
            let mut buf = [0u8; BUF_SIZE];
            buf[0] = REPORT_ID;

            // zone bitmask (u32 LE at offset 2) and header
            let mask = if zone_idx == 0xFF {
                buf[1] = 0x20; // "all zones" header
                0xFFu32
            } else {
                buf[1] = 0x20 + zone_idx;
                1u32 << zone_idx
            };
            buf[2..6].copy_from_slice(&mask.to_le_bytes());

            buf[11] = effect.it8297_id();
            buf[12] = brightness;
            buf[13] = 0; // min_brightness

            // Color in BGR format (the controller stores BGR)
            buf[14] = color.b;
            buf[15] = color.g;
            buf[16] = color.r;

            // Speed → period mapping
            let spd = speed.min(9) as u16;
            match effect {
                RgbEffect::Static | RgbEffect::Off => {
                    // period0=0 → "direct" mode
                }
                RgbEffect::Pulse => {
                    let p = if spd <= 6 {
                        400 + spd * 100
                    } else {
                        1000 + (spd - 6) * 200
                    };
                    buf[22..24].copy_from_slice(&p.to_le_bytes());
                    buf[24..26].copy_from_slice(&p.to_le_bytes());
                    buf[26..28].copy_from_slice(&200u16.to_le_bytes());
                }
                RgbEffect::Blinking => {
                    buf[22..24].copy_from_slice(&100u16.to_le_bytes());
                    buf[24..26].copy_from_slice(&100u16.to_le_bytes());
                    let hold = spd * 200 + 700;
                    buf[26..28].copy_from_slice(&hold.to_le_bytes());
                }
                RgbEffect::ColorCycle => {
                    let p0 = spd * 100 + 300;
                    buf[22..24].copy_from_slice(&p0.to_le_bytes());
                    let p1 = p0.saturating_sub(200);
                    buf[24..26].copy_from_slice(&p1.to_le_bytes());
                    buf[30] = 7; // cycle through 7 colors
                }
                RgbEffect::Wave => {
                    let s = (spd + 1) as f64;
                    let p = (2.5 * s * s + 2.5 * s + 25.0) as u16;
                    buf[22..24].copy_from_slice(&p.to_le_bytes());
                    buf[30] = 7;
                    buf[31] = 1;
                }
                RgbEffect::Random => {
                    buf[22..24].copy_from_slice(&100u16.to_le_bytes());
                    buf[30] = 1;
                    buf[31] = 5;
                }
            }

            eprintln!(
                "IT8297: set_zone_effect zone={} effect={:?} color=#{:02x}{:02x}{:02x} hdr=0x{:02x} mask=0x{:08x}",
                zone_idx, effect, color.r, color.g, color.b, buf[1], mask
            );
            let res = self.dev.send_feature_report(&buf);
            eprintln!("IT8297: send_feature_report → {:?}", res);
            res?;
            Ok(())
        }

        /// Apply pending effects with accumulated zone mask.
        pub fn apply(&self, zone_mask: u32) -> Result<(), RgbError> {
            eprintln!("IT8297: apply (0x28) mask=0x{:08x}", zone_mask);
            let mut buf = [0u8; BUF_SIZE];
            buf[0] = REPORT_ID;
            buf[1] = 0x28;
            buf[2..6].copy_from_slice(&zone_mask.to_le_bytes());
            self.dev.send_feature_report(&buf)?;
            Ok(())
        }

        /// Send a uniform color to a digital strip via Direct ARGB mode.
        /// Disables built-in effects, then writes per-LED color data via PktRGB (0x58/0x59).
        /// No apply() needed — direct mode takes effect immediately.
        fn send_strip_direct(&self, zone: &ZoneInfo, color: &RgbColor) -> Result<(), RgbError> {
            // ALWAYS send 0x32 to disable built-in effects before PktRGB data.
            // Don't rely on cache — the controller needs to see this command
            // right before receiving ARGB packets.
            let bitmask: u8 = match zone.idx {
                6 => 0x02,
                _ => 0x01,
            };
            let disabled_val = self.effect_disabled.get() | bitmask;
            self.effect_disabled.set(disabled_val);
            eprintln!(
                "IT8297: force 0x32 → 0x{:02x} (disable built-in for zone {})",
                disabled_val, zone.idx
            );
            self.send_cc(0x32, disabled_val, 0)?;
            std::thread::sleep(std::time::Duration::from_millis(50));

            let argb_addr: u8 = match zone.idx {
                6 => 0x59, // D_LED2_ARGB
                _ => 0x58, // D_LED1_ARGB
            };

            let (bo_r, bo_g, bo_b) = match zone.idx {
                6 => self.d_led2_cal,
                _ => self.d_led1_cal,
            };

            let total_leds = zone.led_count as usize;
            let mut byte_offset = 0u16;
            let mut sent = 0usize;

            eprintln!(
                "IT8297: send_strip_direct zone={} addr=0x{:02x} leds={} cal=({},{},{}) color=#{:02x}{:02x}{:02x}",
                zone.idx, argb_addr, total_leds, bo_r, bo_g, bo_b, color.r, color.g, color.b
            );

            while sent < total_leds {
                let leds_in_pkt = (total_leds - sent).min(19);
                let bcount = (leds_in_pkt * 3) as u8;

                let mut buf = [0u8; BUF_SIZE];
                buf[0] = REPORT_ID;
                buf[1] = argb_addr;
                buf[2..4].copy_from_slice(&byte_offset.to_le_bytes());
                buf[4] = bcount;

                for i in 0..leds_in_pkt {
                    let base = 5 + i * 3;
                    buf[base + bo_r as usize] = color.r;
                    buf[base + bo_g as usize] = color.g;
                    buf[base + bo_b as usize] = color.b;
                }

                // Hex dump first packet for debugging
                if sent == 0 {
                    eprintln!("IT8297: PktRGB[0] first 16 bytes: {:02x?}", &buf[..16]);
                }

                self.dev.send_feature_report(&buf)?;
                byte_offset += bcount as u16;
                sent += leds_in_pkt;
            }

            eprintln!(
                "IT8297: strip direct done, sent {} LEDs in {} packets",
                total_leds,
                (total_leds + 18) / 19
            );
            Ok(())
        }

        /// Set ALL zones to one static color — including D_LED strips.
        /// Onboard LEDs use hardware effect mode; digital strips use Direct ARGB mode.
        pub fn set_color_all(&self, color: RgbColor) -> Result<(), RgbError> {
            eprintln!(
                "IT8297: set_color_all #{:02x}{:02x}{:02x} ({} zones)",
                color.r,
                color.g,
                color.b,
                self.zones.len()
            );

            let mut zone_mask: u32 = 0;

            for zone in &self.zones {
                eprintln!(
                    "IT8297:   zone {} '{}' header=0x{:02x} digital={}",
                    zone.idx, zone.name, zone.header, zone.is_digital
                );

                if zone.is_digital {
                    // Digital strips → Direct ARGB mode (bypass effect engine)
                    if let Err(e) = self.send_strip_direct(zone, &color) {
                        eprintln!("IT8297:   strip direct error zone={}: {e}", zone.idx);
                    }
                } else {
                    // Onboard LEDs → hardware effect mode
                    if let Err(e) =
                        self.set_zone_effect(zone.idx, RgbEffect::Static, color, 0, 0xFF)
                    {
                        eprintln!("IT8297:   zone {} error: {e}", zone.idx);
                    } else {
                        zone_mask |= 1u32 << zone.idx;
                    }
                }
            }

            // Apply only for onboard zones (direct strips don't need apply)
            if zone_mask != 0 {
                self.apply(zone_mask)?;
            }
            Ok(())
        }

        /// Set a single zone to one static color.
        pub fn set_zone_color(&self, zone_local: usize, color: RgbColor) -> Result<(), RgbError> {
            let zone = self
                .zones
                .get(zone_local)
                .ok_or_else(|| RgbError::Protocol(format!("Zone {zone_local} existiert nicht")))?;

            if zone.is_digital {
                // Digital strip → Direct ARGB mode
                self.send_strip_direct(zone, &color)
            } else {
                // Onboard LED → hardware effect mode
                let mask = 1u32 << zone.idx;
                self.set_zone_effect(zone.idx, RgbEffect::Static, color, 0, 0xFF)?;
                self.apply(mask)
            }
        }

        /// Send colors for a digital LED strip (D_LED1/D_LED2).
        /// Uses Direct ARGB mode with calibration-based byte order.
        pub fn set_strip_colors(
            &self,
            zone_local: usize,
            colors: &[RgbColor],
        ) -> Result<(), RgbError> {
            let zone = self
                .zones
                .get(zone_local)
                .ok_or_else(|| RgbError::Protocol(format!("Zone {zone_local} existiert nicht")))?;
            if !zone.is_digital {
                return Err(RgbError::Protocol(format!(
                    "'{}' ist kein digitaler Strip",
                    zone.name
                )));
            }

            // Disable built-in effects for Direct ARGB mode
            self.strip_builtin_set(zone.idx, false)?;

            let argb_addr: u8 = match zone.idx {
                6 => 0x59, // D_LED2
                _ => 0x58, // D_LED1
            };

            let (bo_r, bo_g, bo_b) = match zone.idx {
                6 => self.d_led2_cal,
                _ => self.d_led1_cal,
            };

            // Send RGB data in 19-LED chunks
            let mut byte_offset = 0u16;
            for chunk in colors.chunks(19) {
                let bcount = (chunk.len() * 3) as u8;

                let mut buf = [0u8; BUF_SIZE];
                buf[0] = REPORT_ID;
                buf[1] = argb_addr;
                buf[2..4].copy_from_slice(&byte_offset.to_le_bytes());
                buf[4] = bcount;

                for (i, c) in chunk.iter().enumerate() {
                    let base = 5 + i * 3;
                    buf[base + bo_r as usize] = c.r;
                    buf[base + bo_g as usize] = c.g;
                    buf[base + bo_b as usize] = c.b;
                }

                self.dev.send_feature_report(&buf)?;
                byte_offset += u16::from(bcount);
            }

            Ok(())
        }

        /// Turn off all LEDs.
        pub fn off(&self) -> Result<(), RgbError> {
            self.set_color_all(RgbColor::BLACK)
        }

        /// Set a hardware effect on all zones — including D_LED strips.
        /// Static/Off effects use Direct ARGB mode for strips;
        /// animated effects (Pulse, Wave, etc.) use the built-in effect engine.
        pub fn set_effect_all(
            &self,
            effect: RgbEffect,
            color: RgbColor,
            speed: u8,
        ) -> Result<(), RgbError> {
            eprintln!(
                "IT8297: set_effect_all {:?} #{:02x}{:02x}{:02x}",
                effect, color.r, color.g, color.b
            );

            // Static / Off → use Direct ARGB for strips
            if matches!(effect, RgbEffect::Static | RgbEffect::Off) {
                return self.set_color_all(if effect == RgbEffect::Off {
                    RgbColor::BLACK
                } else {
                    color
                });
            }

            // Animated effects → enable built-in on strips + SetLEDEffect on all
            for zone in &self.zones {
                if zone.is_digital {
                    self.strip_builtin_set(zone.idx, true)?;
                }
            }

            let mut zone_mask: u32 = 0;
            for zone in &self.zones {
                self.set_zone_effect(zone.idx, effect, color, speed, 0xFF)?;
                zone_mask |= 1u32 << zone.idx;
            }
            self.apply(zone_mask)
        }

        // ── helpers ──

        /// Enable or disable built-in effects for a specific D_LED header.
        /// Tracks state in `effect_disabled` Cell — only sends 0x32 when state changes.
        /// This matches OpenRGB's `SetStripBuiltinEffectState` exactly.
        fn strip_builtin_set(&self, zone_idx: u8, enable: bool) -> Result<(), RgbError> {
            // Bitmask per header (matches OpenRGB switch/case):
            let bitmask: u8 = match zone_idx {
                6 => 0x02, // HDR_D_LED2
                7 => 0x08, // HDR_D_LED3
                8 => 0x10, // HDR_D_LED4
                _ => 0x01, // HDR_D_LED1 (default)
            };

            let current = self.effect_disabled.get();
            let new_val = if enable {
                current & !bitmask // Clear bit = enable built-in effects
            } else {
                current | bitmask // Set bit = disable built-in effects
            };

            if new_val != current {
                eprintln!("IT8297: strip_builtin zone={zone_idx} 0x{current:02x}→0x{new_val:02x}");
                self.effect_disabled.set(new_val);
                self.send_cc(0x32, new_val, 0)?;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(())
        }

        fn send_cc(&self, cmd: u8, b: u8, c: u8) -> Result<(), RgbError> {
            let mut buf = [0u8; BUF_SIZE];
            buf[0] = REPORT_ID;
            buf[1] = cmd;
            buf[2] = b;
            buf[3] = c;
            self.dev.send_feature_report(&buf)?;
            Ok(())
        }
    }
}

// ═══════════════════════════════════════════════════════════════
//  Corsair K70 RGB TKL Champion Series — OpenLinkHub Protocol
//
//  The K70 RGB TKL CS (PID 0x1BB9) uses 1024-byte HID reports,
//  NOT the 64-byte "Corsair V2 Peripheral" protocol from OpenRGB.
//  Protocol reverse-engineered from OpenLinkHub k70rgbtklcs.go.
// ═══════════════════════════════════════════════════════════════

mod k70 {
    use super::*;

    pub const VID: u16 = 0x1B1C;
    pub const PID: u16 = 0x1BB9;

    /// HID report sizes — K70 TKL CS uses 1024-byte reports.
    const BUF_SIZE: usize = 1024;
    const BUF_SIZE_WRITE: usize = BUF_SIZE + 1; // +1 for report ID
    const HEADER_SIZE: usize = 2; // [reportID, 0x08]
    const HEADER_WRITE_SIZE: usize = 4; // prefix in color buffer
    const MAX_CHUNK: usize = 1021; // max payload per transfer
    const TIMEOUT: i32 = 200; // ms read timeout
    const WRITE_TIMEOUT: i32 = 100; // ms read timeout for color writes

    /// Color data: 389 bytes = 130 LEDs × 3 (R,G,B) – 1 (last LED has 2 bytes?)
    /// Matches OpenLinkHub's colorPacketLength.
    const COLOR_PKT_LEN: usize = 389;
    const LED_CHANNELS: usize = 130;

    // ── Commands (passed as "endpoint" to transfer()) ──
    const CMD_SOFTWARE_MODE: &[u8] = &[0x01, 0x03, 0x00, 0x02];
    const CMD_HARDWARE_MODE: &[u8] = &[0x01, 0x03, 0x00, 0x01];
    const CMD_ACTIVATE_LED: &[u8] = &[0x0D, 0x00, 0x22];
    const CMD_GET_FIRMWARE: &[u8] = &[0x02, 0x13];
    const CMD_WRITE_COLOR: &[u8] = &[0x06, 0x00];
    const CMD_SUB_COLOR: &[u8] = &[0x07, 0x00];
    const CMD_KEEPALIVE: &[u8] = &[0x12];
    const CMD_BRIGHTNESS: &[u8] = &[0x01, 0x02, 0x00];

    /// dataTypeSetColor prefix inside the color buffer
    const DATA_TYPE_SET_COLOR: &[u8] = &[0x12, 0x00];

    pub struct K70 {
        dev: HidDevice,
        pub led_count: usize,
    }

    impl K70 {
        pub fn connect() -> Result<Self, RgbError> {
            let api = HidApi::new()?;

            // K70 TKL CS uses interface 1
            let info = api
                .device_list()
                .find(|d| {
                    d.vendor_id() == VID && d.product_id() == PID && d.interface_number() == 1
                })
                .ok_or(RgbError::Hid("K70 TKL nicht gefunden".into()))?;

            let path = info.path().to_owned();
            let dev = info.open_device(&api)?;
            dev.set_blocking_mode(true)?;
            eprintln!("K70: HID opened (1b1c:1bb9 iface 1) path={:?}", path);

            let mut this = Self {
                dev,
                led_count: LED_CHANNELS,
            };

            this.init()?;

            // Non-blocking for normal operation
            this.dev.set_blocking_mode(false)?;
            Ok(this)
        }

        /// Init sequence from OpenLinkHub:
        ///   setHardwareMode(reset) → setSoftwareMode → initLeds (+ 500ms sleep) → getDeviceFirmware → setBrightness
        fn init(&mut self) -> Result<(), RgbError> {
            // 0. Reset: hardware mode first, then software mode
            eprintln!("K70: sending hardware mode (reset)...");
            let _ = self.transfer(CMD_HARDWARE_MODE, &[]); // ignore errors
            std::thread::sleep(std::time::Duration::from_millis(100));

            // 1. Switch to software mode
            self.transfer(CMD_SOFTWARE_MODE, &[])?;
            eprintln!("K70: software mode set");

            // 2. Activate LED endpoint (initLeds)
            self.transfer(CMD_ACTIVATE_LED, &[])?;
            std::thread::sleep(std::time::Duration::from_millis(500));
            eprintln!("K70: LED endpoint activated");

            // 3. Query firmware
            let fw = self.transfer(CMD_GET_FIRMWARE, &[])?;
            if fw.len() > 6 {
                let v1 = fw[3] as u32;
                let v2 = fw[4] as u32;
                let v3 = u16::from_le_bytes([fw[5], fw[6]]) as u32;
                eprintln!("K70: firmware {v1}.{v2}.{v3}");
            } else {
                eprintln!("K70: firmware query → {:02x?}", &fw[..fw.len().min(12)]);
            }

            // 4. Set brightness to max (1000) — OpenLinkHub's setBrightnessLevel()
            //    cmdBrightness = [0x01, 0x02, 0x00], data = LE16(1000)
            let brightness: u16 = 1000;
            self.transfer(CMD_BRIGHTNESS, &brightness.to_le_bytes())?;
            eprintln!("K70: brightness set to {}", brightness);

            eprintln!("K70: init ok, led_count={}", self.led_count);
            Ok(())
        }

        /// Set all LEDs to one color.
        ///
        /// Color data is 389 bytes of interleaved R,G,B per key position.
        pub fn set_color_all(&self, color: RgbColor) -> Result<(), RgbError> {
            eprintln!(
                "K70: set_color_all #{:02x}{:02x}{:02x}",
                color.r, color.g, color.b
            );

            // Build interleaved RGB buffer (389 bytes)
            let mut rgb_data = vec![0u8; COLOR_PKT_LEN];
            // Fill with repeating R,G,B triplets for all positions
            let mut i = 0;
            while i + 2 < COLOR_PKT_LEN {
                rgb_data[i] = color.r;
                rgb_data[i + 1] = color.g;
                rgb_data[i + 2] = color.b;
                i += 3;
            }
            // Handle remaining bytes at the end
            if i < COLOR_PKT_LEN {
                rgb_data[i] = color.r;
            }
            if i + 1 < COLOR_PKT_LEN {
                rgb_data[i + 1] = color.g;
            }

            self.write_color(&rgb_data)?;
            eprintln!("K70: set_color_all done");
            Ok(())
        }

        /// Turn off.
        pub fn off(&self) -> Result<(), RgbError> {
            self.set_color_all(RgbColor::BLACK)
        }

        /// Return to hardware mode.
        pub fn disconnect_clean(&self) -> Result<(), RgbError> {
            self.transfer(CMD_HARDWARE_MODE, &[])?;
            Ok(())
        }

        // ── protocol helpers ──

        /// Core transfer function matching OpenLinkHub's `transfer()`.
        ///
        /// Packet layout (1025 bytes):
        ///   [0] = 0x00 (report ID)
        ///   [1] = 0x08 (command byte)
        ///   [2..2+endpoint.len()] = endpoint bytes
        ///   [2+endpoint.len()..] = buffer data
        ///
        /// Then read 1024-byte response.
        fn transfer(&self, endpoint: &[u8], buffer: &[u8]) -> Result<Vec<u8>, RgbError> {
            self.transfer_inner(endpoint, buffer, TIMEOUT)
        }

        /// Like transfer() but with a shorter read timeout for color data writes
        /// where we don't really need the response.
        fn transfer_fast(&self, endpoint: &[u8], buffer: &[u8]) -> Result<(), RgbError> {
            let _ = self.transfer_inner(endpoint, buffer, WRITE_TIMEOUT)?;
            Ok(())
        }

        fn transfer_inner(
            &self,
            endpoint: &[u8],
            buffer: &[u8],
            read_timeout: i32,
        ) -> Result<Vec<u8>, RgbError> {
            let mut buf_w = vec![0u8; BUF_SIZE_WRITE];
            buf_w[1] = 0x08;
            let ep_end = HEADER_SIZE + endpoint.len();
            buf_w[HEADER_SIZE..ep_end].copy_from_slice(endpoint);
            if !buffer.is_empty() {
                let data_end = (ep_end + buffer.len()).min(BUF_SIZE_WRITE);
                buf_w[ep_end..data_end].copy_from_slice(&buffer[..data_end - ep_end]);
            }

            self.dev.write(&buf_w)?;

            let mut buf_r = vec![0u8; BUF_SIZE];
            let n = self.dev.read_timeout(&mut buf_r, read_timeout)?;
            buf_r.truncate(n);

            Ok(buf_r)
        }

        /// Write color data to the device, matching OpenLinkHub's writeColor().
        ///
        /// Builds a color buffer:
        ///   [0..1] = LE16(data.len())     ← headerWriteSize prefix
        ///   [2..3] = 0x00, 0x00
        ///   [4..5] = dataTypeSetColor [0x12, 0x00]
        ///   [6..]  = RGB data
        ///
        /// Splits into 1021-byte chunks:
        ///   first  → transfer(CMD_WRITE_COLOR, chunk)
        ///   rest   → transfer(CMD_SUB_COLOR, chunk)
        fn write_color(&self, data: &[u8]) -> Result<(), RgbError> {
            // Build the full color buffer
            let buf_len = HEADER_WRITE_SIZE + DATA_TYPE_SET_COLOR.len() + data.len();
            let mut buffer = vec![0u8; buf_len];

            // First 2 bytes: LE16 length of data
            let len_le = (data.len() as u16).to_le_bytes();
            buffer[0] = len_le[0];
            buffer[1] = len_le[1];
            // bytes [2..3] stay 0x00
            // dataTypeSetColor
            buffer[HEADER_WRITE_SIZE..HEADER_WRITE_SIZE + DATA_TYPE_SET_COLOR.len()]
                .copy_from_slice(DATA_TYPE_SET_COLOR);
            // RGB data
            buffer[HEADER_WRITE_SIZE + DATA_TYPE_SET_COLOR.len()..].copy_from_slice(data);

            // Split into chunks and send (use fast transfer — no need to wait for response)
            let chunks: Vec<&[u8]> = buffer.chunks(MAX_CHUNK).collect();
            for (i, chunk) in chunks.iter().enumerate() {
                if i == 0 {
                    self.transfer_fast(CMD_WRITE_COLOR, chunk)?;
                } else {
                    self.transfer_fast(CMD_SUB_COLOR, chunk)?;
                }
            }
            Ok(())
        }

        /// Send keepalive (should be called every ~20s).
        #[allow(dead_code)]
        pub fn keepalive(&self) -> Result<(), RgbError> {
            self.transfer(CMD_KEEPALIVE, &[])?;
            Ok(())
        }
    }
}

// ═══════════════════════════════════════════════════════════════
//  SteelSeries Aerox 3 Driver
// ═══════════════════════════════════════════════════════════════

mod aerox3 {
    use super::*;

    pub const VID: u16 = 0x1038;
    pub const PID: u16 = 0x1836;
    const BUF_SIZE: usize = 65;

    /// Zone names
    pub const ZONES: &[&str] = &["Front", "Mitte", "Hinten"];

    pub struct Aerox3 {
        dev: HidDevice,
    }

    impl Aerox3 {
        pub fn connect() -> Result<Self, RgbError> {
            let api = HidApi::new()?;

            // Interface 3, usage page 0xFFC0
            let info = api
                .device_list()
                .find(|d| {
                    d.vendor_id() == VID && d.product_id() == PID && d.interface_number() == 3
                })
                .ok_or(RgbError::Hid("SteelSeries Aerox 3 nicht gefunden".into()))?;

            let dev = info.open_device(&api)?;
            eprintln!("Aerox 3: HID opened (1038:1836 iface 3)");

            let this = Self { dev };
            this.init()?;
            Ok(this)
        }

        fn init(&self) -> Result<(), RgbError> {
            // Enter software control: feature report 0x2D
            let mut buf = [0u8; BUF_SIZE];
            buf[0] = 0x00;
            buf[1] = 0x2D;
            self.dev.send_feature_report(&buf)?;
            eprintln!("Aerox 3: init (software mode)");
            Ok(())
        }

        /// Set one zone to a color. zone_id: 0=Front, 1=Middle, 2=Rear
        pub fn set_zone_color(&self, zone_id: usize, color: RgbColor) -> Result<(), RgbError> {
            if zone_id >= 3 {
                return Err(RgbError::Protocol(format!("Zone {zone_id} ungültig (0-2)")));
            }
            let mut buf = [0u8; BUF_SIZE];
            buf[0] = 0x00; // report ID
            buf[1] = 0x21; // set color command
            buf[2] = 1u8 << zone_id; // zone bitmask

            let off = 3 + zone_id * 3;
            buf[off] = color.r;
            buf[off + 1] = color.g;
            buf[off + 2] = color.b;

            self.dev.write(&buf)?;
            Ok(())
        }

        /// Set all 3 zones to one color.
        pub fn set_color_all(&self, color: RgbColor) -> Result<(), RgbError> {
            let mut buf = [0u8; BUF_SIZE];
            buf[0] = 0x00;
            buf[1] = 0x21;
            buf[2] = 0x07; // all 3 zones

            for i in 0..3 {
                let off = 3 + i * 3;
                buf[off] = color.r;
                buf[off + 1] = color.g;
                buf[off + 2] = color.b;
            }

            self.dev.write(&buf)?;
            Ok(())
        }

        /// Set brightness (0–100).
        #[allow(dead_code)]
        pub fn set_brightness(&self, brightness: u8) -> Result<(), RgbError> {
            let mut buf = [0u8; BUF_SIZE];
            buf[0] = 0x00;
            buf[1] = 0x23;
            buf[2] = brightness.min(100);
            self.dev.write(&buf)?;
            Ok(())
        }

        pub fn off(&self) -> Result<(), RgbError> {
            self.set_color_all(RgbColor::BLACK)
        }
    }
}

// ═══════════════════════════════════════════════════════════════
//  SteelSeries QCK Prism Cloth XL Driver
// ═══════════════════════════════════════════════════════════════

mod qck {
    use super::*;

    pub const VID: u16 = 0x1038;
    pub const PID: u16 = 0x150D;
    const FEATURE_SIZE: usize = 525;
    const COMMIT_SIZE: usize = 65;

    pub const ZONES: &[&str] = &["Unten", "Oben"];

    pub struct Qck {
        dev: HidDevice,
    }

    impl Qck {
        pub fn connect() -> Result<Self, RgbError> {
            let api = HidApi::new()?;

            let info = api
                .device_list()
                .find(|d| {
                    d.vendor_id() == VID && d.product_id() == PID && d.interface_number() == 0
                })
                .ok_or(RgbError::Hid("SteelSeries QCK Prism nicht gefunden".into()))?;

            let dev = info.open_device(&api)?;
            eprintln!("QCK Prism: HID opened (1038:150d iface 0)");

            Ok(Self { dev })
        }

        /// Set both zones. bottom=Zone 0, top=Zone 1
        pub fn set_colors(&self, bottom: RgbColor, top: RgbColor) -> Result<(), RgbError> {
            let mut buf = vec![0u8; FEATURE_SIZE];
            buf[0] = 0x00; // report ID
            buf[1] = 0x0E; // set dual-zone effect
            buf[3] = 0x02; // zone count

            // Zone 0 (bottom) color
            buf[5] = bottom.r;
            buf[6] = bottom.g;
            buf[7] = bottom.b;
            // Zone 0 static effect params
            buf[8] = 0xFF;
            buf[9] = 0x32;
            buf[10] = 0xC8;
            buf[14] = 0x01;

            // Zone 1 (top) color
            buf[0x11] = top.r;
            buf[0x12] = top.g;
            buf[0x13] = top.b;
            // Zone 1 static effect params
            buf[0x14] = 0xFF;
            buf[0x15] = 0x32;
            buf[0x16] = 0xC8;
            buf[0x19] = 0x01;
            buf[0x1A] = 0x01;
            buf[0x1C] = 0x01;

            self.dev.send_feature_report(&buf)?;

            // Commit
            let mut cbuf = [0u8; COMMIT_SIZE];
            cbuf[0] = 0x00;
            cbuf[1] = 0x0D;
            self.dev.write(&cbuf)?;

            Ok(())
        }

        /// Set both zones to same color.
        pub fn set_color_all(&self, color: RgbColor) -> Result<(), RgbError> {
            self.set_colors(color, color)
        }

        pub fn off(&self) -> Result<(), RgbError> {
            self.set_color_all(RgbColor::BLACK)
        }
    }
}

// ═══════════════════════════════════════════════════════════════
//  XPG SPECTRIX S40G NVMe SSD — ENE RGB via NVMe Admin Passthrough
//
//  Uses NVMe vendor-specific admin commands (0xFA read, 0xFB write)
//  to control the onboard ENE RGB controller through /dev/nvmeX.
//  Protocol from OpenRGB: ENESMBusInterface_SpectrixS40G_Linux.
// ═══════════════════════════════════════════════════════════════

#[allow(unsafe_code)]
mod xpg_s40g {
    use super::*;
    use std::fs;
    use std::os::unix::io::AsRawFd;

    /// ENE I2C device address on the NVMe-internal bus.
    const ENE_DEV: u32 = 0x67;

    /// Number of RGB LEDs on the S40G heatspreader.
    pub const LED_COUNT: usize = 8;

    // ── ENE Register addresses (V2 — AUDA0-E6K5-0101 variant) ──
    #[allow(dead_code)]
    const REG_DIRECT_COLOR: u16 = 0x8100; // V2: 30 bytes (5 LEDs × R,B,G)
    const REG_EFFECT_COLOR: u16 = 0x8160; // V2: 30 bytes (5 LEDs × R,B,G)
    const REG_DIRECT_ACCESS: u16 = 0x8020; // 1=direct, 0=effect
    const REG_MODE: u16 = 0x8021; // 0=Off,1=Static,2=Breathing,…
    const REG_SPEED: u16 = 0x8022; // 0=fastest … 4=slowest
    #[allow(dead_code)]
    const REG_DIRECTION: u16 = 0x8023; // 0=forward, 1=reverse
    const REG_APPLY: u16 = 0x80A0; // write 0x01 to apply

    // ── NVMe ioctl ──
    // NVME_IOCTL_ADMIN_CMD = _IOWR('N', 0x41, struct nvme_admin_cmd)
    // = (3 << 30) | (0x4E << 8) | 0x41 | (72 << 16)
    const NVME_IOCTL_ADMIN_CMD: libc::c_ulong = 0xC048_4E41;

    /// Matches the kernel's `struct nvme_admin_cmd` (72 bytes).
    #[repr(C)]
    #[derive(Default)]
    struct NvmeAdminCmd {
        opcode: u8,
        flags: u8,
        rsvd1: u16,
        nsid: u32,
        cdw2: u32,
        cdw3: u32,
        metadata: u64,
        addr: u64,
        metadata_len: u32,
        data_len: u32,
        cdw10: u32,
        cdw11: u32,
        cdw12: u32,
        cdw13: u32,
        cdw14: u32,
        cdw15: u32,
        timeout_ms: u32,
        result: u32,
    }

    pub struct XpgS40g {
        fd: fs::File,
        pub device_name: String,
        pub nvme_path: String,
    }

    impl XpgS40g {
        /// Scan NVMe devices and connect to S40G if found.
        pub fn connect() -> Result<Self, RgbError> {
            let nvme_path = Self::find_device()?;
            let fd = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&nvme_path)
                .map_err(|e| {
                    RgbError::Protocol(format!(
                        "Kann {nvme_path} nicht öffnen: {e} — Root-Rechte nötig?"
                    ))
                })?;

            let mut this = Self {
                fd,
                device_name: String::new(),
                nvme_path,
            };

            // Read device name from ENE register 0x1000
            this.device_name = this.read_device_name()?;
            eprintln!(
                "XPG S40G: verbunden auf {} ({})",
                this.nvme_path, this.device_name
            );

            Ok(this)
        }

        /// Find the NVMe device path for the S40G.
        fn find_device() -> Result<String, RgbError> {
            let nvme_dir = fs::read_dir("/sys/class/nvme")
                .map_err(|e| RgbError::Protocol(format!("Kein NVMe: {e}")))?;

            for entry in nvme_dir.flatten() {
                let model_path = entry.path().join("model");
                if let Ok(model) = fs::read_to_string(&model_path) {
                    let model = model.trim();
                    if model.contains("SPECTRIX S40G")
                        || model.contains("GAMMIX S41")
                        || model.contains("FALCON")
                    {
                        let name = entry.file_name();
                        let nvme_name = name.to_string_lossy();
                        let dev_path = format!("/dev/{nvme_name}");
                        eprintln!("XPG S40G: gefunden: {model} → {dev_path}");
                        return Ok(dev_path);
                    }
                }
            }

            Err(RgbError::Protocol(
                "XPG SPECTRIX S40G nicht gefunden".into(),
            ))
        }

        /// Read the 16-byte device name from ENE register 0x1000.
        fn read_device_name(&self) -> Result<String, RgbError> {
            let mut name_bytes = Vec::with_capacity(16);
            for i in 0..16u16 {
                let b = self.ene_read(0x1000 + i)?;
                if b == 0 {
                    break;
                }
                name_bytes.push(b);
            }
            Ok(String::from_utf8_lossy(&name_bytes).to_string())
        }

        /// Byte-swap a 16-bit ENE register address.
        fn reg_swap(reg: u16) -> u32 {
            u32::from(((reg << 8) & 0xFF00) | ((reg >> 8) & 0x00FF))
        }

        /// Read a single byte from an ENE register via NVMe admin passthrough.
        #[allow(clippy::cast_possible_truncation)]
        fn ene_read(&self, reg: u16) -> Result<u8, RgbError> {
            let mut data = [0u8; 4];
            let corrected = Self::reg_swap(reg);

            let mut cmd = NvmeAdminCmd {
                opcode: 0xFA,
                addr: data.as_mut_ptr() as u64,
                data_len: 4,
                cdw12: (corrected << 16) | (ENE_DEV << 1),
                cdw13: 0x8110_0001,
                timeout_ms: 1000,
                ..NvmeAdminCmd::default()
            };

            let ret = unsafe { libc::ioctl(self.fd.as_raw_fd(), NVME_IOCTL_ADMIN_CMD, &mut cmd) };

            if ret < 0 {
                return Err(RgbError::Protocol(format!(
                    "NVMe read reg 0x{reg:04X} fehlgeschlagen: {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(data[0])
        }

        /// Write a single byte to an ENE register.
        #[allow(clippy::cast_possible_truncation)]
        fn ene_write(&self, reg: u16, val: u8) -> Result<(), RgbError> {
            let mut data = [0u8; 4];
            data[0] = val;
            let corrected = Self::reg_swap(reg);

            let mut cmd = NvmeAdminCmd {
                opcode: 0xFB,
                addr: data.as_mut_ptr() as u64,
                data_len: 4,
                cdw12: (corrected << 16) | (ENE_DEV << 1),
                cdw13: 0x0110_0001,
                timeout_ms: 1000,
                ..NvmeAdminCmd::default()
            };

            let ret = unsafe { libc::ioctl(self.fd.as_raw_fd(), NVME_IOCTL_ADMIN_CMD, &mut cmd) };

            if ret < 0 {
                return Err(RgbError::Protocol(format!(
                    "NVMe write reg 0x{reg:04X}=0x{val:02X} fehlgeschlagen: {}",
                    std::io::Error::last_os_error()
                )));
            }

            Ok(())
        }

        /// Write a block of bytes starting at an ENE register (max 24 bytes).
        #[allow(clippy::cast_possible_truncation)]
        fn ene_write_block(&self, reg: u16, data: &[u8]) -> Result<(), RgbError> {
            let sz = data.len();
            if sz == 0 || sz > 24 {
                return Err(RgbError::Protocol(format!(
                    "Block size {sz} ungültig (1-24)"
                )));
            }

            // Pad to 4-byte boundary for NVMe DMA alignment
            let padded_len = (sz + 3) & !3;
            let mut buf = vec![0u8; padded_len];
            buf[..sz].copy_from_slice(data);

            let corrected = Self::reg_swap(reg);

            let mut cmd = NvmeAdminCmd {
                opcode: 0xFB,
                addr: buf.as_mut_ptr() as u64,
                data_len: padded_len as u32,
                cdw12: (corrected << 16) | (ENE_DEV << 1),
                cdw13: 0x0310_0000 | (sz as u32),
                timeout_ms: 1000,
                ..NvmeAdminCmd::default()
            };

            let ret = unsafe { libc::ioctl(self.fd.as_raw_fd(), NVME_IOCTL_ADMIN_CMD, &mut cmd) };

            if ret < 0 {
                return Err(RgbError::Protocol(format!(
                    "NVMe write_block reg 0x{reg:04X} ({sz}B) fehlgeschlagen: {}",
                    std::io::Error::last_os_error()
                )));
            }

            Ok(())
        }

        /// Apply pending register changes.
        fn apply(&self) -> Result<(), RgbError> {
            self.ene_write(REG_APPLY, 0x01)
        }

        /// Build a 24-byte R,B,G color buffer for 8 LEDs.
        /// ENE V2 uses R,B,G byte order (not R,G,B).
        fn build_color_buf(color: RgbColor) -> [u8; 24] {
            let mut rgb = [0u8; 24];
            for i in 0..LED_COUNT {
                rgb[i * 3] = color.r;
                rgb[i * 3 + 1] = color.b; // B before G!
                rgb[i * 3 + 2] = color.g;
            }
            rgb
        }

        /// Set all 5 LEDs to one color via effect mode (Static).
        /// Uses V2 registers (0x8160) with R,B,G byte order.
        /// Sequence from OpenRGB: colors → apply → mode → apply → direct=0 → apply
        pub fn set_color_all(&self, color: RgbColor) -> Result<(), RgbError> {
            let rbg = Self::build_color_buf(color);

            // Step 1: Write colors to V2 effect register + apply
            self.ene_write_block(REG_EFFECT_COLOR, &rbg)?;
            self.apply()?;

            // Step 2: Set mode + apply
            if color.r == 0 && color.g == 0 && color.b == 0 {
                self.ene_write(REG_MODE, 0x00)?; // Off
            } else {
                self.ene_write(REG_MODE, 0x01)?; // Static
            }
            self.ene_write(REG_SPEED, 0x00)?;
            self.apply()?;

            // Step 3: Disable direct mode + apply
            self.ene_write(REG_DIRECT_ACCESS, 0x00)?;
            self.apply()?;
            Ok(())
        }

        /// Set a hardware effect mode with color.
        /// Sequence from OpenRGB: colors → apply → mode → apply → direct=0 → apply
        pub fn set_effect(
            &self,
            effect: RgbEffect,
            color: RgbColor,
            speed: u8,
        ) -> Result<(), RgbError> {
            let ene_mode: u8 = match effect {
                RgbEffect::Off => 0,
                RgbEffect::Static => 1,
                RgbEffect::Pulse => 2,      // Breathing
                RgbEffect::Blinking => 3,   // Flashing
                RgbEffect::ColorCycle => 4, // Spectrum Cycle
                RgbEffect::Wave => 5,       // Rainbow
                RgbEffect::Random => 9,     // Random Flicker (ENE mode 9)
            };

            // Step 1: Write colors to V2 effect register + apply
            let rbg = Self::build_color_buf(color);
            self.ene_write_block(REG_EFFECT_COLOR, &rbg)?;
            self.apply()?;

            // Step 2: Set mode + speed + apply
            self.ene_write(REG_MODE, ene_mode)?;
            self.ene_write(REG_SPEED, speed.min(4))?;
            self.apply()?;

            // Step 3: Disable direct mode + apply
            self.ene_write(REG_DIRECT_ACCESS, 0x00)?;
            self.apply()?;
            Ok(())
        }

        pub fn off(&self) -> Result<(), RgbError> {
            self.ene_write(REG_MODE, 0x00)?;
            self.apply()?;
            self.ene_write(REG_DIRECT_ACCESS, 0x00)?;
            self.apply()
        }
    }
}

// ═══════════════════════════════════════════════════════════════
//  XPG SPECTRIX S20G (Pi5)  —  remote control via SSH + xpg-rgb CLI
// ═══════════════════════════════════════════════════════════════

mod xpg_s20g_remote {
    use super::*;
    use std::process::Command;

    const PI5_HOST: &str = "max@192.168.0.8";
    #[allow(dead_code)]
    const SSH_TIMEOUT: &str = "3";
    pub const LED_COUNT: usize = 8;

    pub struct XpgS20gRemote {
        pub device_name: String,
        pub temperature: Option<f32>,
    }

    impl XpgS20gRemote {
        fn ssh_cmd(args: &str) -> Result<String, RgbError> {
            let output = Command::new("ssh")
                .args([
                    "-o",
                    "ConnectTimeout=3",
                    "-o",
                    "StrictHostKeyChecking=no",
                    "-o",
                    "BatchMode=yes",
                    PI5_HOST,
                    &format!("sudo xpg-rgb {args}"),
                ])
                .output()
                .map_err(|e| RgbError::Protocol(format!("SSH fehlgeschlagen: {e}")))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(RgbError::Protocol(format!("Pi5 xpg-rgb Fehler: {stderr}")));
            }
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }

        pub fn connect() -> Result<Self, RgbError> {
            let info = Self::ssh_cmd("info")?;
            let mut device_name = String::from("XPG S20G");
            let mut temperature = None;

            for line in info.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("Controller:") {
                    device_name = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("Temperatur:") {
                    let val = val.trim().trim_end_matches(" C");
                    temperature = val.parse::<f32>().ok();
                }
            }

            eprintln!("XPG S20G Remote: Pi5 verbunden ({})", device_name);
            Ok(Self {
                device_name,
                temperature,
            })
        }

        pub fn set_color_all(&self, color: RgbColor) -> Result<(), RgbError> {
            Self::ssh_cmd(&format!(
                "color {:02x}{:02x}{:02x}",
                color.r, color.g, color.b
            ))?;
            Ok(())
        }

        pub fn set_effect(
            &self,
            effect: RgbEffect,
            color: RgbColor,
            speed: u8,
        ) -> Result<(), RgbError> {
            let name = match effect {
                RgbEffect::Off => "off",
                RgbEffect::Static => "static",
                RgbEffect::Pulse => "pulse",
                RgbEffect::Blinking => "blink",
                RgbEffect::ColorCycle => "cycle",
                RgbEffect::Wave => "wave",
                RgbEffect::Random => "random",
            };
            if effect == RgbEffect::Off {
                return self.off();
            }
            Self::ssh_cmd(&format!(
                "effect {} -c {:02x}{:02x}{:02x} -s {}",
                name, color.r, color.g, color.b, speed
            ))?;
            Ok(())
        }

        pub fn off(&self) -> Result<(), RgbError> {
            Self::ssh_cmd("off")?;
            Ok(())
        }

        /// Refresh temperature reading.
        #[allow(dead_code)]
        pub fn refresh_temp(&mut self) {
            if let Ok(info) = Self::ssh_cmd("info") {
                for line in info.lines() {
                    if let Some(val) = line.trim().strip_prefix("Temperatur:") {
                        let val = val.trim().trim_end_matches(" C");
                        self.temperature = val.parse::<f32>().ok();
                    }
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
//  Unified RGB Manager  —  one singleton that holds all drivers
// ═══════════════════════════════════════════════════════════════

/// Per-device status returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct RgbDeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String,
    pub vendor: String,
    pub connected: bool,
    pub zones: Vec<RgbZoneInfo>,
    pub effects: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RgbZoneInfo {
    pub name: String,
    pub leds_count: u32,
    pub is_digital: bool,
}

/// Full system RGB status.
#[derive(Debug, Clone, Serialize)]
pub struct RgbStatus {
    pub connected: bool,
    pub devices: Vec<RgbDeviceInfo>,
}

/// Saved per-device colour/effect (for profile persistence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RgbDeviceState {
    pub id: String,
    pub color: RgbColor,
    pub effect: RgbEffect,
    #[serde(default)]
    pub speed: u8,
}

/// Internal state holding all connected drivers.
struct RgbManager {
    it8297: Option<it8297::It8297>,
    k70: Option<k70::K70>,
    aerox3: Option<aerox3::Aerox3>,
    qck: Option<qck::Qck>,
    xpg_s40g: Option<xpg_s40g::XpgS40g>,
    xpg_s20g: Option<xpg_s20g_remote::XpgS20gRemote>,
    /// Last applied state per device (keyed by device id).
    state: std::collections::HashMap<String, RgbDeviceState>,
}

fn manager() -> &'static Mutex<RgbManager> {
    use std::sync::OnceLock;
    static INSTANCE: OnceLock<Mutex<RgbManager>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        Mutex::new(RgbManager {
            it8297: None,
            k70: None,
            aerox3: None,
            qck: None,
            xpg_s40g: None,
            xpg_s20g: None,
            state: std::collections::HashMap::new(),
        })
    })
}

/// Get a snapshot of the last-applied colour/effect for all devices.
pub fn get_openrgb_state() -> Vec<RgbDeviceState> {
    manager()
        .lock()
        .map(|g| g.state.values().cloned().collect())
        .unwrap_or_default()
}

/// Restore previously saved OpenRGB device states.
pub fn apply_openrgb_state(states: &[RgbDeviceState]) {
    for s in states {
        eprintln!(
            "Profile: restoring {} → #{:02x}{:02x}{:02x} {:?}",
            s.id, s.color.r, s.color.g, s.color.b, s.effect
        );
        if s.effect == RgbEffect::Off || (s.color.r == 0 && s.color.g == 0 && s.color.b == 0) {
            let _ = openrgb_off(s.id.clone());
        } else if s.effect == RgbEffect::Static {
            let _ = openrgb_set_color(s.id.clone(), s.color.r, s.color.g, s.color.b);
        } else {
            let _ = openrgb_set_mode(
                s.id.clone(),
                s.effect.mode_index(),
                Some(u32::from(s.speed)),
                None,
                None,
                Some(vec![s.color]),
            );
        }
    }
}

fn with_mgr<R>(f: impl FnOnce(&mut RgbManager) -> Result<R, RgbError>) -> Result<R, String> {
    // Try to acquire the lock with a timeout to prevent the UI from freezing
    // if a device I/O operation is blocking.
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(3);
    let mut guard = loop {
        match manager().try_lock() {
            Ok(g) => break g,
            Err(std::sync::TryLockError::Poisoned(e)) => {
                return Err(format!("Lock poisoned: {e}"));
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                if start.elapsed() > timeout {
                    return Err("RGB-Manager blockiert (Timeout 3s) — evtl. Gerät hängt".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    };
    f(&mut guard).map_err(|e| e.to_string())
}

/// Build device info list from current manager state.
fn build_status(mgr: &RgbManager) -> RgbStatus {
    let mut devices = Vec::new();

    // IT8297
    if let Some(ref dev) = mgr.it8297 {
        devices.push(RgbDeviceInfo {
            id: "it8297".into(),
            name: format!("Gigabyte {} (IT8297)", dev.product_name),
            device_type: "Motherboard".into(),
            vendor: "Gigabyte".into(),
            connected: true,
            zones: dev
                .zones
                .iter()
                .map(|z| RgbZoneInfo {
                    name: z.name.clone(),
                    leds_count: z.led_count,
                    is_digital: z.is_digital,
                })
                .collect(),
            effects: vec![
                "Static".into(),
                "Pulse".into(),
                "Blinking".into(),
                "ColorCycle".into(),
                "Wave".into(),
                "Random".into(),
            ],
        });
    }

    // K70
    if let Some(ref dev) = mgr.k70 {
        devices.push(RgbDeviceInfo {
            id: "k70".into(),
            name: "Corsair K70 RGB TKL".into(),
            device_type: "Keyboard".into(),
            vendor: "Corsair".into(),
            connected: true,
            zones: vec![RgbZoneInfo {
                name: "Keyboard".into(),
                leds_count: dev.led_count as u32,
                is_digital: false,
            }],
            effects: vec!["Static".into()],
        });
    }

    // Aerox 3
    if mgr.aerox3.is_some() {
        devices.push(RgbDeviceInfo {
            id: "aerox3".into(),
            name: "SteelSeries Aerox 3".into(),
            device_type: "Mouse".into(),
            vendor: "SteelSeries".into(),
            connected: true,
            zones: aerox3::ZONES
                .iter()
                .map(|&n| RgbZoneInfo {
                    name: n.into(),
                    leds_count: 1,
                    is_digital: false,
                })
                .collect(),
            effects: vec!["Static".into()],
        });
    }

    // QCK
    if mgr.qck.is_some() {
        devices.push(RgbDeviceInfo {
            id: "qck".into(),
            name: "SteelSeries QCK Prism Cloth".into(),
            device_type: "Mousemat".into(),
            vendor: "SteelSeries".into(),
            connected: true,
            zones: qck::ZONES
                .iter()
                .map(|&n| RgbZoneInfo {
                    name: n.into(),
                    leds_count: 1,
                    is_digital: false,
                })
                .collect(),
            effects: vec!["Static".into()],
        });
    }

    // XPG S40G NVMe
    if let Some(ref dev) = mgr.xpg_s40g {
        devices.push(RgbDeviceInfo {
            id: "xpg_s40g".into(),
            name: format!("XPG SPECTRIX S40G ({})", dev.device_name),
            device_type: "Storage".into(),
            vendor: "ADATA".into(),
            connected: true,
            zones: vec![RgbZoneInfo {
                name: "NVMe LEDs".into(),
                leds_count: xpg_s40g::LED_COUNT as u32,
                is_digital: false,
            }],
            effects: vec![
                "Static".into(),
                "Pulse".into(),
                "Blinking".into(),
                "ColorCycle".into(),
                "Wave".into(),
                "Random".into(),
            ],
        });
    }

    // XPG S20G NVMe (Pi5 — remote via SSH)
    if let Some(ref dev) = mgr.xpg_s20g {
        let mut name = format!("XPG SPECTRIX S20G Pi5 ({})", dev.device_name);
        if let Some(t) = dev.temperature {
            name.push_str(&format!(" {:.0}°C", t));
        }
        devices.push(RgbDeviceInfo {
            id: "xpg_s20g".into(),
            name,
            device_type: "Storage".into(),
            vendor: "ADATA".into(),
            connected: true,
            zones: vec![RgbZoneInfo {
                name: "NVMe LEDs".into(),
                leds_count: xpg_s20g_remote::LED_COUNT as u32,
                is_digital: false,
            }],
            effects: vec![
                "Static".into(),
                "Pulse".into(),
                "Blinking".into(),
                "ColorCycle".into(),
                "Wave".into(),
                "Random".into(),
            ],
        });
    }

    let connected = !devices.is_empty();
    RgbStatus { connected, devices }
}

// ═══════════════════════════════════════════════════════════════
//  Tauri Commands
// ═══════════════════════════════════════════════════════════════

/// Scan system & connect to all available RGB devices.
#[command]
pub fn openrgb_connect() -> Result<RgbStatus, String> {
    eprintln!("RGB CMD: openrgb_connect");
    with_mgr(|mgr| {
        let mut errors = Vec::new();

        // IT8297
        if mgr.it8297.is_none() {
            match it8297::It8297::connect() {
                Ok(dev) => {
                    eprintln!("RGB: IT8297 '{}' verbunden", dev.product_name);
                    mgr.it8297 = Some(dev);
                }
                Err(e) => errors.push(format!("IT8297: {e}")),
            }
        }

        // K70
        if mgr.k70.is_none() {
            match k70::K70::connect() {
                Ok(dev) => {
                    eprintln!("RGB: K70 TKL verbunden ({} LEDs)", dev.led_count);
                    mgr.k70 = Some(dev);
                }
                Err(e) => errors.push(format!("K70: {e}")),
            }
        }

        // Aerox 3
        if mgr.aerox3.is_none() {
            match aerox3::Aerox3::connect() {
                Ok(dev) => {
                    eprintln!("RGB: Aerox 3 verbunden");
                    mgr.aerox3 = Some(dev);
                }
                Err(e) => errors.push(format!("Aerox 3: {e}")),
            }
        }

        // QCK
        if mgr.qck.is_none() {
            match qck::Qck::connect() {
                Ok(dev) => {
                    eprintln!("RGB: QCK Prism verbunden");
                    mgr.qck = Some(dev);
                }
                Err(e) => errors.push(format!("QCK: {e}")),
            }
        }

        // XPG S40G NVMe
        if mgr.xpg_s40g.is_none() {
            match xpg_s40g::XpgS40g::connect() {
                Ok(dev) => {
                    eprintln!("RGB: XPG S40G verbunden ({})", dev.device_name);
                    mgr.xpg_s40g = Some(dev);
                }
                Err(e) => errors.push(format!("XPG S40G: {e}")),
            }
        }

        // XPG S20G NVMe (Pi5 — remote)
        if mgr.xpg_s20g.is_none() {
            match xpg_s20g_remote::XpgS20gRemote::connect() {
                Ok(dev) => {
                    eprintln!("RGB: XPG S20G Pi5 verbunden ({})", dev.device_name);
                    mgr.xpg_s20g = Some(dev);
                }
                Err(e) => errors.push(format!("XPG S20G Pi5: {e}")),
            }
        }

        if !errors.is_empty() {
            eprintln!("RGB: Einige Geräte nicht gefunden: {}", errors.join(", "));
        }

        Ok(build_status(mgr))
    })
}

/// Disconnect all RGB devices.
#[command]
pub fn openrgb_disconnect() -> Result<String, String> {
    eprintln!("RGB CMD: openrgb_disconnect");
    with_mgr(|mgr| {
        if let Some(ref k70) = mgr.k70 {
            let _ = k70.disconnect_clean();
        }
        mgr.it8297 = None;
        mgr.k70 = None;
        mgr.aerox3 = None;
        mgr.qck = None;
        mgr.xpg_s40g = None;
        mgr.xpg_s20g = None;
        eprintln!("RGB: Alle Geräte getrennt");
        Ok("Alle RGB-Geräte getrennt".into())
    })
}

/// Get current status without reconnecting.
#[command]
pub fn openrgb_status() -> Result<RgbStatus, String> {
    with_mgr(|mgr| Ok(build_status(mgr)))
}

/// Re-scan: disconnect all, then reconnect.
#[command]
pub fn openrgb_refresh() -> Result<RgbStatus, String> {
    // Disconnect first — use with_mgr for consistent timeout behavior
    with_mgr(|mgr| {
        if let Some(ref k70) = mgr.k70 {
            let _ = k70.disconnect_clean();
        }
        mgr.it8297 = None;
        mgr.k70 = None;
        mgr.aerox3 = None;
        mgr.qck = None;
        mgr.xpg_s40g = None;
        mgr.xpg_s20g = None;
        Ok(())
    })?;
    openrgb_connect()
}

/// Set all LEDs on a device to one color.
#[command]
pub fn openrgb_set_color(device_id: String, r: u8, g: u8, b: u8) -> Result<String, String> {
    eprintln!("RGB CMD: openrgb_set_color({device_id}, #{r:02x}{g:02x}{b:02x})");
    // Scale by master brightness before sending to hardware
    let (sr, sg, sb) = super::lighting::apply_brightness(r, g, b);
    let hw_color = RgbColor {
        r: sr,
        g: sg,
        b: sb,
    };
    let profile_color = RgbColor { r, g, b };
    with_mgr(|mgr| {
        match device_id.as_str() {
            "it8297" => mgr
                .it8297
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_color_all(hw_color),
            "k70" => mgr
                .k70
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_color_all(hw_color),
            "aerox3" => mgr
                .aerox3
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_color_all(hw_color),
            "qck" => mgr
                .qck
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_color_all(hw_color),
            "xpg_s40g" => mgr
                .xpg_s40g
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_color_all(hw_color),
            "xpg_s20g" => mgr
                .xpg_s20g
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_color_all(hw_color),
            _ => Err(RgbError::Protocol(format!(
                "Unbekanntes Gerät: {device_id}"
            ))),
        }?;
        // Store unscaled color in state/profile so brightness can re-scale later
        let effect = if r == 0 && g == 0 && b == 0 {
            RgbEffect::Off
        } else {
            RgbEffect::Static
        };
        mgr.state.insert(
            device_id.clone(),
            RgbDeviceState {
                id: device_id.clone(),
                color: profile_color,
                effect,
                speed: 0,
            },
        );
        Ok(format!("{device_id}: #{r:02x}{g:02x}{b:02x} gesetzt"))
    })
}

/// Set a single zone to one color.
#[command]
pub fn openrgb_set_zone_color(
    device_id: String,
    zone_id: u32,
    r: u8,
    g: u8,
    b: u8,
) -> Result<String, String> {
    eprintln!(
        "RGB CMD: openrgb_set_zone_color({device_id}, zone={zone_id}, #{r:02x}{g:02x}{b:02x})"
    );
    let color = RgbColor { r, g, b };
    with_mgr(|mgr| {
        match device_id.as_str() {
            "it8297" => mgr
                .it8297
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_zone_color(zone_id as usize, color),
            "aerox3" => mgr
                .aerox3
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_zone_color(zone_id as usize, color),
            "qck" => {
                let dev = mgr.qck.as_ref().ok_or(RgbError::NotConnected)?;
                // QCK has 2 zones, always sends both
                match zone_id {
                    0 => dev.set_colors(color, RgbColor::BLACK),
                    1 => dev.set_colors(RgbColor::BLACK, color),
                    _ => Err(RgbError::Protocol("QCK hat nur 2 Zonen".into())),
                }
            }
            "k70" => {
                // K70 only has one zone (keyboard), treat as set_color_all
                mgr.k70
                    .as_ref()
                    .ok_or(RgbError::NotConnected)?
                    .set_color_all(color)
            }
            "xpg_s40g" => {
                // S40G only has one zone
                mgr.xpg_s40g
                    .as_ref()
                    .ok_or(RgbError::NotConnected)?
                    .set_color_all(color)
            }
            "xpg_s20g" => mgr
                .xpg_s20g
                .as_ref()
                .ok_or(RgbError::NotConnected)?
                .set_color_all(color),
            _ => Err(RgbError::Protocol(format!(
                "Unbekanntes Gerät: {device_id}"
            ))),
        }?;
        Ok(format!(
            "{device_id} Zone {zone_id}: #{r:02x}{g:02x}{b:02x}"
        ))
    })
}

/// Set a single LED (only digital strips on IT8297).
#[command]
pub fn openrgb_set_led(
    device_id: String,
    led_id: i32,
    r: u8,
    g: u8,
    b: u8,
) -> Result<String, String> {
    let _color = RgbColor { r, g, b };
    Err(format!(
        "Einzelne LED-Steuerung für {device_id}:{led_id} nicht unterstützt — nutze set_zone_leds"
    ))
}

/// Set a zone's LEDs to individual colors (addressable strip).
#[command]
pub fn openrgb_set_zone_leds(
    device_id: String,
    zone_id: u32,
    colors: Vec<RgbColor>,
) -> Result<String, String> {
    with_mgr(|mgr| match device_id.as_str() {
        "it8297" => {
            let dev = mgr.it8297.as_ref().ok_or(RgbError::NotConnected)?;
            dev.set_strip_colors(zone_id as usize, &colors)?;
            Ok(format!("{} LEDs auf IT8297 Zone {zone_id}", colors.len()))
        }
        _ => Err(RgbError::Protocol(format!(
            "{device_id} unterstützt keine adressierbaren LEDs"
        ))),
    })
}

/// Set hardware effect on a device (IT8297 supports all, others static only).
#[command]
pub fn openrgb_set_mode(
    device_id: String,
    mode_id: usize,
    speed: Option<u32>,
    _brightness: Option<u32>,
    _direction: Option<u32>,
    colors: Option<Vec<RgbColor>>,
) -> Result<String, String> {
    eprintln!("RGB CMD: openrgb_set_mode({device_id}, mode={mode_id})");
    with_mgr(|mgr| {
        let effects = [
            RgbEffect::Off,
            RgbEffect::Static,
            RgbEffect::Pulse,
            RgbEffect::Blinking,
            RgbEffect::ColorCycle,
            RgbEffect::Wave,
            RgbEffect::Random,
        ];
        let effect = effects
            .get(mode_id)
            .copied()
            .ok_or_else(|| RgbError::Protocol(format!("Effekt {mode_id} ungültig (0-6)")))?;
        let color = colors
            .as_ref()
            .and_then(|c| c.first().copied())
            .unwrap_or(RgbColor { r: 255, g: 0, b: 0 });
        let spd = speed.unwrap_or(5) as u8;

        match device_id.as_str() {
            "it8297" => {
                let dev = mgr.it8297.as_ref().ok_or(RgbError::NotConnected)?;
                dev.set_effect_all(effect, color, spd)?;
            }
            "xpg_s40g" => {
                let dev = mgr.xpg_s40g.as_ref().ok_or(RgbError::NotConnected)?;
                dev.set_effect(effect, color, spd)?;
            }
            "xpg_s20g" => {
                let dev = mgr.xpg_s20g.as_ref().ok_or(RgbError::NotConnected)?;
                dev.set_effect(effect, color, spd)?;
            }
            _ => {
                // For non-IT8297 devices: just set static color or off
                let target_color = if effect == RgbEffect::Off {
                    RgbColor::BLACK
                } else {
                    color
                };
                match device_id.as_str() {
                    "k70" => mgr
                        .k70
                        .as_ref()
                        .ok_or(RgbError::NotConnected)?
                        .set_color_all(target_color),
                    "aerox3" => mgr
                        .aerox3
                        .as_ref()
                        .ok_or(RgbError::NotConnected)?
                        .set_color_all(target_color),
                    "qck" => mgr
                        .qck
                        .as_ref()
                        .ok_or(RgbError::NotConnected)?
                        .set_color_all(target_color),
                    _ => Err(RgbError::Protocol(format!("Unbekannt: {device_id}"))),
                }?;
            }
        }
        mgr.state.insert(
            device_id.clone(),
            RgbDeviceState {
                id: device_id.clone(),
                color,
                effect,
                speed: spd,
            },
        );
        Ok(format!("{device_id}: {effect:?}"))
    })
}

/// Turn off all LEDs on one device.
#[command]
pub fn openrgb_off(device_id: String) -> Result<String, String> {
    openrgb_set_color(device_id, 0, 0, 0)
}

/// Turn off ALL RGB devices.
#[command]
pub fn openrgb_all_off() -> Result<String, String> {
    with_mgr(|mgr| {
        let mut count = 0u32;
        if let Some(ref dev) = mgr.it8297 {
            if dev.off().is_ok() {
                count += 1;
            }
        }
        if let Some(ref dev) = mgr.k70 {
            if dev.off().is_ok() {
                count += 1;
            }
        }
        if let Some(ref dev) = mgr.aerox3 {
            if dev.off().is_ok() {
                count += 1;
            }
        }
        if let Some(ref dev) = mgr.qck {
            if dev.off().is_ok() {
                count += 1;
            }
        }
        if let Some(ref dev) = mgr.xpg_s40g {
            if dev.off().is_ok() {
                count += 1;
            }
        }
        if let Some(ref dev) = mgr.xpg_s20g {
            if dev.off().is_ok() {
                count += 1;
            }
        }
        Ok(format!("{count} Geräte ausgeschaltet"))
    })
}
