//! HID transport layer for Corsair USB devices.
//!
//! Provides:
//! - `HidError` — unified error enum with `Disconnected` + `LockPoisoned` variants
//! - `DeviceSlot<T>` — encapsulates `Mutex<Option<T>>` with typed access methods
//! - `HidHandle` — framed packet I/O on top of `hidapi`
//! - `enumerate_devices()` — Corsair device discovery

#![allow(dead_code)]

use super::protocol::{
    CCXT_BUF_SIZE, CCXT_BUF_WRITE, CCXT_CMD_BYTE, CCXT_DATA_HEADER_SIZE, CCXT_HEADER_SIZE,
    CCXT_PRODUCT_ID, CMD_CLOSE_ENDPOINT, CMD_OPEN_ENDPOINT, CMD_READ, CMD_WRITE, CORSAIR_VENDOR_ID,
    NEXUS_PRODUCT_ID,
};
use log::{debug, warn};
use std::fmt;
use std::sync::{Mutex, MutexGuard};

// ═════════════════════════════════════════════════════════════
//  Error
// ═════════════════════════════════════════════════════════════

#[derive(Debug)]
pub enum HidError {
    /// `hidapi` returned an error.
    Api(String),
    /// Requested device not found during enumeration.
    NotFound { product_id: u16 },
    /// Read returned fewer bytes than expected.
    ShortRead { expected: usize, got: usize },
    /// Write failed or wrote fewer bytes than expected.
    WriteFailed(String),
    /// Device is not connected — call `connect()` first.
    Disconnected,
    /// Internal mutex was poisoned (another thread panicked while holding it).
    LockPoisoned,
}

impl fmt::Display for HidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Api(msg) => write!(f, "HID API: {msg}"),
            Self::NotFound { product_id } => write!(f, "Device 0x{product_id:04x} not found"),
            Self::ShortRead { expected, got } => {
                write!(f, "Short read: expected {expected}, got {got}")
            }
            Self::WriteFailed(msg) => write!(f, "Write failed: {msg}"),
            Self::Disconnected => write!(f, "Device not connected"),
            Self::LockPoisoned => write!(f, "Internal lock poisoned"),
        }
    }
}

impl std::error::Error for HidError {}

impl From<hidapi::HidError> for HidError {
    fn from(e: hidapi::HidError) -> Self {
        Self::Api(e.to_string())
    }
}

// ═════════════════════════════════════════════════════════════
//  DeviceSlot<T> — typed Mutex<Option<T>> wrapper
// ═════════════════════════════════════════════════════════════

/// Thread-safe slot for a device that transitions between connected and
/// disconnected states.
///
/// Encapsulates the `Mutex<Option<T>>` pattern and eliminates the
/// repetitive `.lock().map_err(…)?.as_mut().ok_or(…)?` boilerplate.
pub struct DeviceSlot<T>(Mutex<Option<T>>);

impl<T> DeviceSlot<T> {
    /// Construct an empty (disconnected) slot.  `const`-compatible for use
    /// in `static` declarations.
    pub const fn empty() -> Self {
        Self(Mutex::new(None))
    }

    /// Run `f` with a shared reference to the connected device.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> Result<R, HidError>) -> Result<R, HidError> {
        let guard = self.lock()?;
        let inner = guard.as_ref().ok_or(HidError::Disconnected)?;
        f(inner)
    }

    /// Run `f` with an exclusive reference to the connected device.
    pub fn with_mut<R>(
        &self,
        f: impl FnOnce(&mut T) -> Result<R, HidError>,
    ) -> Result<R, HidError> {
        let mut guard = self.lock()?;
        let inner = guard.as_mut().ok_or(HidError::Disconnected)?;
        f(inner)
    }

    /// Store a new device, returning the previous one (if any) for
    /// cleanup via its `Drop` impl.
    pub fn connect(&self, value: T) -> Result<Option<T>, HidError> {
        let mut guard = self.lock()?;
        Ok(guard.replace(value))
    }

    /// Take the device out.  The returned value will be dropped by the
    /// caller, triggering its `Drop` impl for RAII cleanup.
    pub fn disconnect(&self) -> Result<Option<T>, HidError> {
        let mut guard = self.lock()?;
        Ok(guard.take())
    }

    /// Non-blocking connectivity check.
    pub fn is_connected(&self) -> bool {
        self.0.lock().ok().is_some_and(|g| g.is_some())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<T>>, HidError> {
        self.0.lock().map_err(|poison| {
            log::warn!("Mutex poisoned: {poison}");
            HidError::LockPoisoned
        })
    }
}

// ═════════════════════════════════════════════════════════════
//  Device Enumeration
// ═════════════════════════════════════════════════════════════

/// Discovered Corsair HID device metadata.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsairDeviceInfo {
    pub product_id: u16,
    pub serial: String,
    pub product: String,
    pub path: String,
}

/// Supported Corsair product IDs (only devices we can actually drive).
const SUPPORTED_PIDS: &[u16] = &[CCXT_PRODUCT_ID, NEXUS_PRODUCT_ID];

/// Enumerate supported Corsair HID devices (interface 0 only).
pub fn enumerate_devices() -> Result<Vec<CorsairDeviceInfo>, HidError> {
    let api = hidapi::HidApi::new()?;
    let devices = api
        .device_list()
        .filter(|i| {
            i.vendor_id() == CORSAIR_VENDOR_ID
                && i.interface_number() == 0
                && SUPPORTED_PIDS.contains(&i.product_id())
        })
        .map(|i| CorsairDeviceInfo {
            product_id: i.product_id(),
            serial: i.serial_number().unwrap_or("").to_string(),
            product: i.product_string().unwrap_or("Unknown").to_string(),
            path: i.path().to_string_lossy().to_string(),
        })
        .collect();
    Ok(devices)
}

// ═════════════════════════════════════════════════════════════
//  HidHandle — framed packet I/O
// ═════════════════════════════════════════════════════════════

/// Opened HID device with CCXT-style packet framing.
pub struct HidHandle {
    device: hidapi::HidDevice,
    product_id: u16,
}

impl HidHandle {
    /// Open by product ID and serial number.
    pub fn open(product_id: u16, serial: &str) -> Result<Self, HidError> {
        let api = hidapi::HidApi::new()?;
        let device = if serial.is_empty() {
            api.open(CORSAIR_VENDOR_ID, product_id)?
        } else {
            api.open_serial(CORSAIR_VENDOR_ID, product_id, serial)?
        };
        device.set_blocking_mode(false)?;
        debug!("HID opened PID=0x{product_id:04x}");
        Ok(Self { device, product_id })
    }

    /// Send a command, read the response.
    pub fn transfer(&self, command: &[u8], data: Option<&[u8]>) -> Result<Vec<u8>, HidError> {
        self.write_packet(command, data)?;
        self.read_packet()
    }

    /// CCXT 4-step endpoint read: close → open → CMD_READ → close.
    /// This is required for all data reads (fans, speeds, temps, LEDs).
    pub fn read_endpoint(&self, mode: &[u8]) -> Result<Vec<u8>, HidError> {
        self.transfer(CMD_CLOSE_ENDPOINT, Some(mode))?;
        self.transfer(CMD_OPEN_ENDPOINT, Some(mode))?;
        let data = self.transfer(CMD_READ, Some(mode))?;
        self.transfer(CMD_CLOSE_ENDPOINT, Some(mode))?;
        Ok(data)
    }

    /// CCXT 4-step endpoint write: close → open → CMD_WRITE → close.
    /// Mirrors OpenLinkHub's `write()` function.
    ///
    /// Builds the data-payload header (`[len_lo, len_hi, 0, 0, buffer_type…, data…]`)
    /// and wraps it in the endpoint open/close sequence.
    pub fn write_endpoint(
        &self,
        mode: &[u8],
        buffer_type: &[u8],
        data: &[u8],
    ) -> Result<Vec<u8>, HidError> {
        let payload_len = data.len() + buffer_type.len();
        let len_val = u16::try_from(payload_len)
            .map_err(|_e| HidError::WriteFailed("Write payload too large".into()))?;

        let buf_len = CCXT_DATA_HEADER_SIZE + buffer_type.len() + data.len();
        let mut buf = vec![0u8; buf_len];
        buf[0..2].copy_from_slice(&len_val.to_le_bytes());
        // buf[2..4] stays zero (padding per CCXT wire format)
        let bt_end = CCXT_DATA_HEADER_SIZE + buffer_type.len();
        buf[CCXT_DATA_HEADER_SIZE..bt_end].copy_from_slice(buffer_type);
        buf[bt_end..].copy_from_slice(data);

        self.transfer(CMD_CLOSE_ENDPOINT, Some(mode))?;
        self.transfer(CMD_OPEN_ENDPOINT, Some(mode))?;
        let resp = self.transfer(CMD_WRITE, Some(&buf))?;
        self.transfer(CMD_CLOSE_ENDPOINT, Some(mode))?;
        Ok(resp)
    }

    /// Send a HID feature report (used by NEXUS for mode control).
    pub fn send_feature_report(&self, data: &[u8]) -> Result<(), HidError> {
        self.device.send_feature_report(data)?;
        Ok(())
    }

    /// Write raw bytes without CCXT framing (used by NEXUS LCD transfers).
    pub fn write_raw(&self, data: &[u8]) -> Result<(), HidError> {
        let written = self.device.write(data)?;
        if written != data.len() {
            return Err(HidError::WriteFailed(format!(
                "Wrote {written}/{}",
                data.len()
            )));
        }
        Ok(())
    }

    // ── Internals ───────────────────────────────────────────

    fn write_packet(&self, command: &[u8], data: Option<&[u8]>) -> Result<(), HidError> {
        let mut buf = vec![0u8; CCXT_BUF_WRITE];
        // buf[0] = 0x00 (HID report ID)
        buf[1] = CCXT_CMD_BYTE; // 0x08 — required command marker

        let cmd_end = CCXT_HEADER_SIZE + command.len();
        if cmd_end > CCXT_BUF_WRITE {
            return Err(HidError::WriteFailed("Command too large".into()));
        }
        buf[CCXT_HEADER_SIZE..cmd_end].copy_from_slice(command);

        if let Some(d) = data {
            let data_end = cmd_end + d.len();
            if data_end > CCXT_BUF_WRITE {
                return Err(HidError::WriteFailed("Payload too large".into()));
            }
            buf[cmd_end..data_end].copy_from_slice(d);
        }

        let written = self.device.write(&buf)?;
        if written == 0 {
            return Err(HidError::WriteFailed("Zero bytes written".into()));
        }
        Ok(())
    }

    fn read_packet(&self) -> Result<Vec<u8>, HidError> {
        let mut buf = vec![0u8; CCXT_BUF_SIZE];
        // Poll: 10 × 5 ms = 50 ms max
        for _ in 0..10 {
            let n = self.device.read_timeout(&mut buf, 5)?;
            if n > 0 {
                buf.truncate(n);
                return Ok(buf);
            }
        }
        warn!("HID read timeout PID=0x{:04x}", self.product_id);
        Ok(Vec::new())
    }
}

impl fmt::Debug for HidHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HidHandle")
            .field("product_id", &format_args!("0x{:04x}", self.product_id))
            .finish_non_exhaustive()
    }
}
