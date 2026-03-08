//! Commander Core XT (CCXT) driver — fans, temps, RGB.
//!
//! Key Rust patterns used:
//! - `DeviceSlot<CcxtInner>` encapsulates `Mutex<Option<T>>` (no boilerplate)
//! - `Drop` on `CcxtInner` for RAII hardware-mode restore
//! - `FanMode` enum as single source of truth per channel
//! - `SpeedPct` / `Celsius` newtypes for type safety

#![allow(dead_code)]

use super::hid::{DeviceSlot, HidError, HidHandle};
use super::protocol::*;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

// ═════════════════════════════════════════════════════════════
//  Public Types
// ═════════════════════════════════════════════════════════════

/// Status of a single fan channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanChannel {
    pub channel: u8,
    pub connected: bool,
    pub rpm: i16,
    pub duty: SpeedPct,
}

/// Status of a temperature probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempProbe {
    pub channel: u8,
    pub connected: bool,
    pub temp: Celsius,
}

/// Full CCXT device status (returned to frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcxtStatus {
    pub firmware: String,
    pub serial: String,
    pub product: String,
    pub fans: Vec<FanChannel>,
    pub temps: Vec<TempProbe>,
    pub fan_modes: Vec<FanMode>,
    pub connected: bool,
}

/// RGB colour value.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

// ═════════════════════════════════════════════════════════════
//  Driver — public API consumed by Tauri commands
// ═════════════════════════════════════════════════════════════

/// Commander Core XT device driver (thread-safe singleton).
pub struct CcxtDriver {
    slot: DeviceSlot<CcxtInner>,
}

impl CcxtDriver {
    /// Construct an empty (disconnected) driver.  `const` so it can live
    /// in a `static` without `OnceLock`.
    pub const fn new() -> Self {
        Self {
            slot: DeviceSlot::empty(),
        }
    }

    /// Open the device, enter software mode, probe channels.
    pub fn connect(&self, serial: &str) -> Result<(), HidError> {
        let handle = HidHandle::open(CCXT_PRODUCT_ID, serial)?;

        handle.transfer(CMD_SOFTWARE_MODE, None)?;
        info!("CCXT: software mode activated");

        let fw_resp = handle.transfer(CMD_GET_FIRMWARE, None)?;
        let firmware = parse_firmware(&fw_resp);
        info!("CCXT: firmware {firmware}");

        let led_count = init_rgb_subsystem(&handle)?;

        let fan_resp = handle.read_endpoint(MODE_GET_FANS)?;
        let fan_count = u8::try_from(parse_channel_count(&fan_resp)).unwrap_or(6);

        let temp_resp = handle.read_endpoint(MODE_GET_TEMPS)?;
        let temp_count = u8::try_from(parse_channel_count(&temp_resp)).unwrap_or(2);

        let fans = (0..fan_count)
            .map(|i| FanChannel {
                channel: i,
                connected: false,
                rpm: 0,
                duty: SpeedPct::default(),
            })
            .collect();

        let temps = (0..temp_count)
            .map(|i| TempProbe {
                channel: i,
                connected: false,
                temp: Celsius::default(),
            })
            .collect();

        let fan_modes = (0..fan_count).map(|_| FanMode::default()).collect();

        let inner = CcxtInner {
            handle,
            firmware,
            serial: serial.to_string(),
            fans,
            temps,
            fan_modes,
            led_count,
            current_color: None,
        };

        // Old CcxtInner (if any) is returned and dropped → Drop restores HW mode
        let _old = self.slot.connect(inner)?;
        Ok(())
    }

    /// Disconnect — `CcxtInner::drop()` restores hardware mode (RAII).
    pub fn disconnect(&self) -> Result<(), HidError> {
        let _old = self.slot.disconnect()?;
        info!("CCXT: disconnected");
        Ok(())
    }

    /// Poll fans + temps.  Call periodically (~1 s).
    pub fn poll(&self) -> Result<CcxtStatus, HidError> {
        self.slot.with_mut(CcxtInner::poll)
    }

    /// Set the operating mode for a single fan channel.
    ///
    /// `FanMode::Fixed` applies the speed immediately.
    /// `FanMode::Curve` takes effect on the next `apply_fan_modes()` call.
    pub fn set_fan_mode(&self, channel: u8, mode: FanMode) -> Result<(), HidError> {
        self.slot.with_mut(|inner| {
            let idx = usize::from(channel);
            if idx >= inner.fan_modes.len() {
                return Err(HidError::Api(format!("Invalid channel {channel}")));
            }
            if let FanMode::Fixed { speed } = &mode {
                write_speed(&inner.handle, channel, *speed)?;
            }
            inner.fan_modes[idx] = mode;
            Ok(())
        })
    }

    /// Evaluate all fan curves against the current temperature and apply.
    pub fn apply_fan_modes(&self) -> Result<(), HidError> {
        self.slot.with_mut(CcxtInner::evaluate_curves)
    }

    /// Set static RGB colour on all LED channels.
    ///
    /// The colour endpoint was opened during `connect()`, so we only need
    /// to build the payload and send it via `CMD_WRITE_COLOR`.  Large
    /// payloads are chunked per the CCXT protocol (max 381 bytes per packet).
    pub fn set_color_static(&self, color: RgbColor) -> Result<(), HidError> {
        self.slot.with_mut(|inner| {
            inner.current_color = Some(color);
            let led_count = inner.led_count;
            if led_count == 0 {
                debug!("CCXT: no LEDs detected, skipping colour");
                return Ok(());
            }

            // Flat RGB data: [R, G, B, R, G, B, …] for every LED
            let rgb_len = led_count * 3;
            let mut rgb_data = vec![0u8; rgb_len];
            for i in 0..led_count {
                let off = i * 3;
                rgb_data[off] = color.r;
                rgb_data[off + 1] = color.g;
                rgb_data[off + 2] = color.b;
            }

            // Build write buffer: [len_lo, len_hi, 0, 0, dataTypeSetColor, RGB…]
            let payload_len = DATA_TYPE_SET_COLOR.len() + rgb_len;
            let len_val = u16::try_from(payload_len)
                .map_err(|_e| HidError::WriteFailed("Colour data too large".into()))?;

            let buf_len = CCXT_DATA_HEADER_SIZE + payload_len;
            let mut buf = vec![0u8; buf_len];
            buf[0..2].copy_from_slice(&len_val.to_le_bytes());
            // buf[2..4] = 0 (padding)
            let dt_end = CCXT_DATA_HEADER_SIZE + DATA_TYPE_SET_COLOR.len();
            buf[CCXT_DATA_HEADER_SIZE..dt_end].copy_from_slice(DATA_TYPE_SET_COLOR);
            buf[dt_end..].copy_from_slice(&rgb_data);

            // Send, chunking at CCXT_MAX_PAYLOAD if the buffer exceeds one packet
            for (i, chunk) in buf.chunks(CCXT_MAX_PAYLOAD).enumerate() {
                if i == 0 {
                    inner.handle.transfer(CMD_WRITE_COLOR, Some(chunk))?;
                } else {
                    inner.handle.transfer(CMD_WRITE_COLOR_NEXT, Some(chunk))?;
                }
            }

            debug!(
                "CCXT: static colour #{:02x}{:02x}{:02x} on {led_count} LEDs",
                color.r, color.g, color.b
            );
            Ok(())
        })
    }

    pub fn is_connected(&self) -> bool {
        self.slot.is_connected()
    }

    /// Return the currently active RGB colour (if any).
    pub fn current_color(&self) -> Option<RgbColor> {
        self.slot
            .with(|inner| Ok(inner.current_color))
            .ok()
            .flatten()
    }
}

// ═════════════════════════════════════════════════════════════
//  CcxtInner — owns the HID handle + volatile state
// ═════════════════════════════════════════════════════════════

struct CcxtInner {
    handle: HidHandle,
    firmware: String,
    serial: String,
    fans: Vec<FanChannel>,
    temps: Vec<TempProbe>,
    /// Single source of truth for each channel's operating mode.
    fan_modes: Vec<FanMode>,
    /// Total number of LEDs across all connected ports.
    led_count: usize,
    /// Active colour — re-applied on every poll to prevent firmware revert.
    current_color: Option<RgbColor>,
}

/// RAII: restore hardware mode when the device object is dropped
/// (on disconnect OR program exit).
impl Drop for CcxtInner {
    fn drop(&mut self) {
        if let Err(e) = self.handle.transfer(CMD_HARDWARE_MODE, None) {
            warn!("CCXT drop: failed to restore hardware mode: {e}");
        } else {
            info!("CCXT: hardware mode restored (RAII)");
        }
    }
}

impl CcxtInner {
    /// Read fans + temps from HID and build a status snapshot.
    fn poll(&mut self) -> Result<CcxtStatus, HidError> {
        self.read_fan_rpms()?;
        self.read_temperatures()?;
        self.evaluate_curves()?;
        self.refresh_color()?;
        Ok(self.snapshot())
    }

    /// Evaluate fan curves against the water temp and write speeds.
    ///
    /// Uses the first **connected** probe (not necessarily index 0).
    /// Only channels in `Curve` mode are touched; `Fixed` channels are
    /// left alone.  Called every poll cycle (~2 s) for smooth regulation.
    fn evaluate_curves(&mut self) -> Result<(), HidError> {
        // Water temp = first *connected* probe (any channel)
        let water_temp = self
            .temps
            .iter()
            .find(|t| t.connected)
            .map_or(Celsius::new(35.0), |t| t.temp);

        for i in 0..self.fans.len() {
            if !self.fans[i].connected {
                continue;
            }
            if let FanMode::Curve { .. } = &self.fan_modes[i] {
                let target = self.fan_modes[i].resolve(water_temp);
                // Only write if the duty actually changed
                if target != self.fans[i].duty {
                    let ch = u8::try_from(i).unwrap_or(0);
                    write_speed(&self.handle, ch, target)?;
                    self.fans[i].duty = target;
                    debug!(
                        "CCXT: curve fan {ch} → {}% (water {water_temp})",
                        target.get()
                    );
                }
            }
        }
        Ok(())
    }

    /// Re-send the active colour (if any) so the firmware doesn't revert.
    fn refresh_color(&self) -> Result<(), HidError> {
        let Some(color) = self.current_color else {
            return Ok(());
        };
        let led_count = self.led_count;
        if led_count == 0 {
            return Ok(());
        }

        let rgb_len = led_count * 3;
        let mut rgb_data = vec![0u8; rgb_len];
        for i in 0..led_count {
            let off = i * 3;
            rgb_data[off] = color.r;
            rgb_data[off + 1] = color.g;
            rgb_data[off + 2] = color.b;
        }

        let payload_len = DATA_TYPE_SET_COLOR.len() + rgb_len;
        let len_val = u16::try_from(payload_len)
            .map_err(|_e| HidError::WriteFailed("Colour data too large".into()))?;

        let buf_len = CCXT_DATA_HEADER_SIZE + payload_len;
        let mut buf = vec![0u8; buf_len];
        buf[0..2].copy_from_slice(&len_val.to_le_bytes());
        let dt_end = CCXT_DATA_HEADER_SIZE + DATA_TYPE_SET_COLOR.len();
        buf[CCXT_DATA_HEADER_SIZE..dt_end].copy_from_slice(DATA_TYPE_SET_COLOR);
        buf[dt_end..].copy_from_slice(&rgb_data);

        for (i, chunk) in buf.chunks(CCXT_MAX_PAYLOAD).enumerate() {
            if i == 0 {
                self.handle.transfer(CMD_WRITE_COLOR, Some(chunk))?;
            } else {
                self.handle.transfer(CMD_WRITE_COLOR_NEXT, Some(chunk))?;
            }
        }
        Ok(())
    }

    fn read_fan_rpms(&mut self) -> Result<(), HidError> {
        let channels = self.handle.read_endpoint(MODE_GET_FANS)?;
        let count = parse_channel_count(&channels);

        let speeds = self.handle.read_endpoint(MODE_GET_SPEEDS)?;
        if speeds.len() < 6 {
            return Ok(());
        }
        let sensor = &speeds[6..];

        for (m, i) in (0..count).enumerate() {
            let s = i * 2;
            if s + 2 > sensor.len() || m >= self.fans.len() {
                break;
            }
            let status = channels.get(6 + i).copied().unwrap_or(0);
            self.fans[m].connected = status == CHANNEL_FAN_CONNECTED;
            if self.fans[m].connected {
                let rpm = i16::from_le_bytes([sensor[s], sensor[s + 1]]);
                if rpm > 0 {
                    self.fans[m].rpm = rpm;
                }
            }
        }
        Ok(())
    }

    fn read_temperatures(&mut self) -> Result<(), HidError> {
        let resp = self.handle.read_endpoint(MODE_GET_TEMPS)?;
        let count = parse_channel_count(&resp);
        if resp.len() < 6 {
            return Ok(());
        }
        let sensor = &resp[6..];

        // Each temp entry is 3 bytes: [status, temp_lo, temp_hi]
        for i in 0..count {
            let s = i * 3;
            if s + 3 > sensor.len() || i >= self.temps.len() {
                break;
            }
            let status = sensor[s];
            // status 0x00 = connected, 0x01 = disconnected
            self.temps[i].connected = status == 0x00;
            if self.temps[i].connected {
                self.temps[i].temp = Celsius::from_raw_le([sensor[s + 1], sensor[s + 2]]);
            }
        }
        Ok(())
    }

    /// Build an owned status snapshot (cheap clones of small vecs).
    fn snapshot(&self) -> CcxtStatus {
        CcxtStatus {
            firmware: self.firmware.clone(),
            serial: self.serial.clone(),
            product: "Commander Core XT".into(),
            fans: self.fans.clone(),
            temps: self.temps.clone(),
            fan_modes: self.fan_modes.clone(),
            connected: true,
        }
    }
}

// ═════════════════════════════════════════════════════════════
//  Free Functions
// ═════════════════════════════════════════════════════════════

fn init_rgb_subsystem(handle: &HidHandle) -> Result<usize, HidError> {
    for port in 1..=6u8 {
        handle.transfer(&[CMD_INIT_LED_PORT, port, 0x01], None)?;
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    info!("CCXT: LED ports initialized");

    let led_resp = handle.read_endpoint(MODE_GET_LEDS)?;
    let (led_ports, led_count) = parse_led_ports(&led_resp);
    info!("CCXT: {led_count} total LEDs across 6 ports");

    handle.transfer(CMD_CLOSE_ENDPOINT, Some(MODE_SET_COLOR))?;
    handle.transfer(CMD_OPEN_COLOR_ENDPOINT, Some(MODE_SET_COLOR))?;
    info!("CCXT: colour endpoint opened");

    let mut buf: Vec<u8> = vec![0x0d, 0x00, 0x07];
    buf.push(0x00);
    for port in &led_ports {
        if port.connected && port.command != 0 {
            buf.push(0x01);
            buf.push(port.command);
        } else {
            buf.push(0x00);
        }
    }
    handle.write_endpoint(&[CMD_SET_LED_PORTS], &[], &buf)?;
    info!("CCXT: LED port types configured");

    handle.transfer(CMD_RESET_LED_POWER, None)?;
    std::thread::sleep(std::time::Duration::from_millis(100));
    info!("CCXT: LED power reset");

    Ok(led_count)
}

/// Write a fan speed to the device using the 4-step endpoint protocol.
///
/// Buffer format per OpenLinkHub `setSpeed()`:
/// `[count, ch0, mode, speed, 0x00, ch1, mode, speed, 0x00, ...]`
/// Each channel occupies a 4-byte block; the first byte is the total channel count.
fn write_speed(handle: &HidHandle, channel: u8, speed: SpeedPct) -> Result<(), HidError> {
    let data: [u8; 5] = [
        1,           // number of channels being set
        channel,     // channel index
        0,           // mode byte (0 = fixed)
        speed.get(), // speed percentage (0–100)
        0,           // padding (4th byte of the block)
    ];

    let resp = handle.write_endpoint(MODE_SET_SPEED, DATA_TYPE_SET_SPEED, &data)?;

    // Retry if the device signals an error (resp[2] != 0x00), up to 10 times.
    if resp.get(2).copied().unwrap_or(0xFF) != 0x00 {
        for attempt in 1..=10 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let retry = handle.write_endpoint(MODE_SET_SPEED, DATA_TYPE_SET_SPEED, &data)?;
            if retry.get(2).copied().unwrap_or(0xFF) == 0x00 {
                debug!("CCXT: fan {channel} speed accepted on retry {attempt}");
                break;
            }
        }
    }

    debug!("CCXT: fan {channel} → {}%", speed.get());
    Ok(())
}
