//! iCUE NEXUS touchscreen driver — 640×48 LCD + 5 soft-buttons.
//!
//! Multi-page live dashboard:
//! - Page 0 "FANS":  Per-fan RPM + duty bars
//! - Page 1 "TEMPS": Water temp, probe readings
//! - Page 2 "SYS":   CPU usage / freq / RAM
//! - Page 3 "CLOCK": Current time + date
//!
//! Pages auto-cycle every ~5 s or can be pinned via touch buttons / frontend.

#![allow(dead_code)]

use super::ccxt::CcxtStatus;
use super::hid::{DeviceSlot, HidError, HidHandle};
use super::protocol::*;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// Last rendered RGBA frame — shared with frontend for live preview.
fn frame_cache() -> &'static Mutex<Vec<u8>> {
    static CACHE: OnceLock<Mutex<Vec<u8>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

// ═════════════════════════════════════════════════════════════
//  Public Types
// ═════════════════════════════════════════════════════════════

/// A touch button definition on the NEXUS screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusButton {
    pub index: u8,
    pub pos_min: u16,
    pub pos_max: u16,
    pub label: String,
}

/// NEXUS device status (returned to frontend).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusStatus {
    pub firmware: String,
    pub serial: String,
    pub product: String,
    pub connected: bool,
    pub lcd_width: u32,
    pub lcd_height: u32,
    pub buttons: Vec<NexusButton>,
    pub last_button: Option<u8>,
    pub current_page: u8,
    pub page_count: u8,
    pub auto_cycle: bool,
}

// ─── Pages ──────────────────────────────────────────────────

pub const PAGE_FANS: u8 = 0;
pub const PAGE_TEMPS: u8 = 1;
pub const PAGE_SYS: u8 = 2;
pub const PAGE_CLOCK: u8 = 3;
pub const PAGE_COUNT: u8 = 4;

const PAGE_NAMES: [&str; PAGE_COUNT as usize] = ["FANS", "TEMPS", "SYSTEM", "UHR"];

/// Lightweight system data passed from the frontend for the SYS page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusSysData {
    pub cpu_usage: f32,
    pub cpu_freq_mhz: f32,
    pub ram_used_pct: f32,
    pub ram_total_gib: f32,
    pub cpu_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub gpu_usage: Option<f32>,
    pub total_power_w: Option<f32>,
    pub water_temp: Option<f32>,
}

// ═════════════════════════════════════════════════════════════
//  Widget Layout Model
// ═════════════════════════════════════════════════════════════

/// Named colour presets for widgets (maps 1:1 to `Rgba` constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WidgetColor {
    #[default]
    White,
    Cyan,
    Amber,
    Red,
    Purple,
    Dim,
}

impl WidgetColor {
    const fn to_rgba(self) -> Rgba {
        match self {
            Self::White => Rgba::WHITE,
            Self::Cyan => Rgba::CYAN,
            Self::Amber => Rgba::AMBER,
            Self::Red => Rgba::RED,
            Self::Purple => Rgba::PURPLE,
            Self::Dim => Rgba::DIM,
        }
    }
}

fn default_color_dim_dark() -> WidgetColor {
    WidgetColor::Dim
}

/// Live data source that a dynamic widget binds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DataSource {
    WaterTemp,
    CpuTemp,
    GpuTemp,
    TotalPower,
    CpuUsage,
    RamUsage,
    CpuFreq,
    RamTotal,
}

/// Visual content type — each variant carries its own configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WidgetKind {
    /// Spinning fan icon with RPM readout.
    FanIcon {
        channel: u8,
        #[serde(default)]
        color: WidgetColor,
        #[serde(default = "default_scale_1")]
        scale: f32,
    },
    /// Label + scaled sensor value (e.g. "H2O  32C").
    Sensor {
        source: DataSource,
        label: String,
        #[serde(default = "default_scale_2")]
        scale: f32,
        #[serde(default)]
        color: WidgetColor,
    },
    /// Label + percentage + horizontal progress bar.
    StatusBar {
        source: DataSource,
        label: String,
        #[serde(default)]
        color: WidgetColor,
        #[serde(default = "default_scale_1")]
        scale: f32,
    },
    /// Static text at a given scale + colour.
    Label {
        text: String,
        #[serde(default = "default_scale_1")]
        scale: f32,
        #[serde(default)]
        color: WidgetColor,
    },
    /// Live clock (time + date).
    Clock {
        #[serde(default)]
        color: WidgetColor,
        #[serde(default = "default_scale_1")]
        scale: f32,
    },
    /// Vertical divider line.
    Divider {
        #[serde(default = "default_color_dim_dark")]
        color: WidgetColor,
    },
    /// Page indicator dots.
    PageDots {
        #[serde(default)]
        color: WidgetColor,
    },
}

fn default_scale_1() -> f32 {
    1.0
}
fn default_scale_2() -> f32 {
    2.0
}

/// A single positioned widget on the NEXUS LCD.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusWidget {
    /// Unique identifier (for frontend drag-and-drop).
    pub id: String,
    /// Content type and configuration.
    pub kind: WidgetKind,
    /// Left edge (0..640).
    pub x: u16,
    /// Top edge (0..48).
    pub y: u16,
    /// Width in pixels.
    pub w: u16,
    /// Height in pixels.
    pub h: u16,
}

/// Layout for a single NEXUS page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayout {
    pub name: String,
    pub widgets: Vec<NexusWidget>,
}

/// Complete NEXUS display layout (all pages).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusLayout {
    pub pages: Vec<PageLayout>,
}

// ─── Layout persistence ─────────────────────────────────────

fn layout_path() -> std::path::PathBuf {
    crate::config::io::config_dir().join("nexus-layout.json")
}

fn load_layout() -> Option<NexusLayout> {
    let data = std::fs::read_to_string(layout_path()).ok()?;
    match serde_json::from_str(&data) {
        Ok(l) => Some(l),
        Err(e) => {
            warn!("NEXUS layout parse error: {e}");
            None
        }
    }
}

fn save_layout(layout: &NexusLayout) -> Result<(), String> {
    let path = layout_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(layout).map_err(|e| format!("json: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write: {e}"))?;
    info!("NEXUS layout saved → {}", path.display());
    Ok(())
}

/// Current widget layout — loaded from disk on first access, then kept in memory.
fn layout_cache() -> &'static Mutex<NexusLayout> {
    static CACHE: OnceLock<Mutex<NexusLayout>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let layout = load_layout().unwrap_or_else(NexusLayout::default_layout);
        Mutex::new(layout)
    })
}

// ─── Default layouts (match original hardcoded dashboard) ───

impl NexusLayout {
    /// Produce the factory-default layout.
    #[must_use]
    pub fn default_layout() -> Self {
        Self {
            pages: vec![
                Self::fans_page(),
                Self::temps_page(),
                Self::sys_page(),
                Self::clock_page(),
            ],
        }
    }

    fn fans_page() -> PageLayout {
        let mut w = Vec::with_capacity(12);
        // 6 spinning fan icons (280 px zone, each 46 px column)
        for ch in 0..6u8 {
            w.push(NexusWidget {
                id: format!("fan-{ch}"),
                kind: WidgetKind::FanIcon {
                    channel: ch,
                    color: WidgetColor::White,
                    scale: 1.0,
                },
                x: u16::from(ch) * 46,
                y: 4,
                w: 46,
                h: 40,
            });
        }
        // Vertical divider
        w.push(NexusWidget {
            id: "div-0".into(),
            kind: WidgetKind::Divider {
                color: WidgetColor::Dim,
            },
            x: 290,
            y: 10,
            w: 1,
            h: 38,
        });
        // Sensor columns: H2O | CPU | GPU | PWR
        let cols: &[(&str, DataSource, WidgetColor)] = &[
            ("H2O", DataSource::WaterTemp, WidgetColor::White),
            ("CPU", DataSource::CpuTemp, WidgetColor::White),
            ("GPU", DataSource::GpuTemp, WidgetColor::White),
            ("PWR", DataSource::TotalPower, WidgetColor::Cyan),
        ];
        for (i, &(lbl, src, clr)) in cols.iter().enumerate() {
            let idx = u16::try_from(i).unwrap_or(0);
            w.push(NexusWidget {
                id: format!("s-{}", lbl.to_lowercase()),
                kind: WidgetKind::Sensor {
                    source: src,
                    label: lbl.into(),
                    scale: 2.0,
                    color: clr,
                },
                x: 300 + idx * 85,
                y: 12,
                w: 85,
                h: 34,
            });
        }
        // Page dots
        w.push(NexusWidget {
            id: "dots".into(),
            kind: WidgetKind::PageDots {
                color: WidgetColor::White,
            },
            x: 604,
            y: 3,
            w: 32,
            h: 8,
        });
        PageLayout {
            name: "FANS".into(),
            widgets: w,
        }
    }

    fn temps_page() -> PageLayout {
        vec_into_page(
            "TEMPS",
            vec![
                NexusWidget {
                    id: "hdr".into(),
                    kind: WidgetKind::Label {
                        text: "TEMPS".into(),
                        scale: 1.0,
                        color: WidgetColor::Cyan,
                    },
                    x: 4,
                    y: 2,
                    w: 60,
                    h: 10,
                },
                NexusWidget {
                    id: "water".into(),
                    kind: WidgetKind::Sensor {
                        source: DataSource::WaterTemp,
                        label: "WASSER".into(),
                        scale: 2.0,
                        color: WidgetColor::White,
                    },
                    x: 4,
                    y: 14,
                    w: 200,
                    h: 30,
                },
                NexusWidget {
                    id: "dots".into(),
                    kind: WidgetKind::PageDots {
                        color: WidgetColor::White,
                    },
                    x: 604,
                    y: 3,
                    w: 32,
                    h: 8,
                },
            ],
        )
    }

    fn sys_page() -> PageLayout {
        vec_into_page(
            "SYSTEM",
            vec![
                NexusWidget {
                    id: "hdr".into(),
                    kind: WidgetKind::Label {
                        text: "SYSTEM".into(),
                        scale: 1.0,
                        color: WidgetColor::Cyan,
                    },
                    x: 4,
                    y: 2,
                    w: 60,
                    h: 10,
                },
                NexusWidget {
                    id: "bar-cpu".into(),
                    kind: WidgetKind::StatusBar {
                        source: DataSource::CpuUsage,
                        label: "CPU".into(),
                        color: WidgetColor::Cyan,
                        scale: 1.0,
                    },
                    x: 4,
                    y: 14,
                    w: 300,
                    h: 10,
                },
                NexusWidget {
                    id: "s-freq".into(),
                    kind: WidgetKind::Sensor {
                        source: DataSource::CpuFreq,
                        label: String::new(),
                        scale: 1.0,
                        color: WidgetColor::Dim,
                    },
                    x: 320,
                    y: 14,
                    w: 80,
                    h: 10,
                },
                NexusWidget {
                    id: "bar-ram".into(),
                    kind: WidgetKind::StatusBar {
                        source: DataSource::RamUsage,
                        label: "RAM".into(),
                        color: WidgetColor::Cyan,
                        scale: 1.0,
                    },
                    x: 4,
                    y: 30,
                    w: 300,
                    h: 10,
                },
                NexusWidget {
                    id: "s-rtot".into(),
                    kind: WidgetKind::Sensor {
                        source: DataSource::RamTotal,
                        label: String::new(),
                        scale: 1.0,
                        color: WidgetColor::Dim,
                    },
                    x: 320,
                    y: 30,
                    w: 80,
                    h: 10,
                },
                NexusWidget {
                    id: "dots".into(),
                    kind: WidgetKind::PageDots {
                        color: WidgetColor::White,
                    },
                    x: 604,
                    y: 3,
                    w: 32,
                    h: 8,
                },
            ],
        )
    }

    fn clock_page() -> PageLayout {
        vec_into_page(
            "UHR",
            vec![
                NexusWidget {
                    id: "hdr".into(),
                    kind: WidgetKind::Label {
                        text: "UHR".into(),
                        scale: 1.0,
                        color: WidgetColor::Cyan,
                    },
                    x: 4,
                    y: 2,
                    w: 40,
                    h: 10,
                },
                NexusWidget {
                    id: "clock".into(),
                    kind: WidgetKind::Clock {
                        color: WidgetColor::White,
                        scale: 1.0,
                    },
                    x: 180,
                    y: 4,
                    w: 280,
                    h: 42,
                },
                NexusWidget {
                    id: "dots".into(),
                    kind: WidgetKind::PageDots {
                        color: WidgetColor::White,
                    },
                    x: 604,
                    y: 3,
                    w: 32,
                    h: 8,
                },
            ],
        )
    }
}

fn vec_into_page(name: &str, widgets: Vec<NexusWidget>) -> PageLayout {
    PageLayout {
        name: name.into(),
        widgets,
    }
}

// ═════════════════════════════════════════════════════════════
//  Driver
// ═════════════════════════════════════════════════════════════

/// Sentinel value for "no button pressed" in the `AtomicU8`.
const NO_BUTTON: u8 = u8::MAX;

/// Auto-cycle interval: how many poll-ticks (each ~2 s) per page.
const AUTO_CYCLE_TICKS: u8 = 12; // ~6 s per page (12 × 500 ms)

/// iCUE NEXUS touchscreen driver (thread-safe singleton).
pub struct NexusDriver {
    slot: DeviceSlot<NexusInner>,
    last_button: AtomicU8,
    current_page: AtomicU8,
    auto_cycle: AtomicBool,
    cycle_counter: AtomicU8,
    /// Current blade angle (0..359) — advanced each refresh for animation.
    fan_angle: AtomicU16,
}

struct NexusInner {
    handle: HidHandle,
    firmware: String,
    serial: String,
}

/// RAII: restore hardware mode when device is dropped.
impl Drop for NexusInner {
    fn drop(&mut self) {
        for report in NEXUS_LCD_STOP_REPORTS {
            if let Err(e) = self.handle.send_feature_report(report) {
                warn!("NEXUS drop: stop-report failed: {e}");
            }
        }
        info!("NEXUS: hardware mode restored (RAII)");
    }
}

impl NexusDriver {
    pub const fn new() -> Self {
        Self {
            slot: DeviceSlot::empty(),
            last_button: AtomicU8::new(NO_BUTTON),
            current_page: AtomicU8::new(PAGE_FANS),
            auto_cycle: AtomicBool::new(false),
            cycle_counter: AtomicU8::new(0),
            fan_angle: AtomicU16::new(0),
        }
    }

    pub fn connect(&self, serial: &str) -> Result<(), HidError> {
        let handle = HidHandle::open(NEXUS_PRODUCT_ID, serial)?;
        let firmware = read_nexus_firmware(&handle);
        info!("NEXUS: connected, firmware {firmware}");

        let inner = NexusInner {
            handle,
            firmware,
            serial: serial.to_string(),
        };
        let _old = self.slot.connect(inner)?;
        self.current_page.store(PAGE_FANS, Ordering::Relaxed);
        self.cycle_counter.store(0, Ordering::Relaxed);
        Ok(())
    }

    pub fn disconnect(&self) -> Result<(), HidError> {
        let _old = self.slot.disconnect()?;
        info!("NEXUS: disconnected");
        Ok(())
    }

    pub fn status(&self) -> Result<NexusStatus, HidError> {
        self.slot.with(|inner| {
            let btn_raw = self.last_button.load(Ordering::Relaxed);
            let last_button = if btn_raw == NO_BUTTON {
                None
            } else {
                Some(btn_raw)
            };

            Ok(NexusStatus {
                firmware: inner.firmware.clone(),
                serial: inner.serial.clone(),
                product: "iCUE NEXUS".into(),
                connected: true,
                lcd_width: NEXUS_IMG_WIDTH,
                lcd_height: NEXUS_IMG_HEIGHT,
                buttons: default_buttons(),
                last_button,
                current_page: self.current_page.load(Ordering::Relaxed),
                page_count: PAGE_COUNT,
                auto_cycle: self.auto_cycle.load(Ordering::Relaxed),
            })
        })
    }

    // ─── Page Control ───────────────────────────────────────

    pub fn set_page(&self, page: u8) {
        let p = if page >= PAGE_COUNT { 0 } else { page };
        self.current_page.store(p, Ordering::Relaxed);
        self.cycle_counter.store(0, Ordering::Relaxed);
        debug!("NEXUS: page → {}", PAGE_NAMES[p as usize]);
    }

    pub fn next_page(&self) {
        let cur = self.current_page.load(Ordering::Relaxed);
        self.set_page((cur + 1) % PAGE_COUNT);
    }

    pub fn prev_page(&self) {
        let cur = self.current_page.load(Ordering::Relaxed);
        self.set_page(if cur == 0 { PAGE_COUNT - 1 } else { cur - 1 });
    }

    pub fn set_auto_cycle(&self, enabled: bool) {
        self.auto_cycle.store(enabled, Ordering::Relaxed);
        debug!("NEXUS: auto-cycle = {enabled}");
    }

    // ─── Display Refresh (called from poll cycle) ───────────

    /// Refresh the NEXUS LCD.  Call this every poll tick (~2 s).
    /// If auto-cycle is on, advances the page every `AUTO_CYCLE_TICKS`.
    pub fn refresh(
        &self,
        ccxt: Option<&CcxtStatus>,
        sys: Option<&NexusSysData>,
    ) -> Result<(), HidError> {
        if !self.is_connected() {
            return Ok(());
        }

        // Auto-cycle
        if self.auto_cycle.load(Ordering::Relaxed) {
            let tick = self.cycle_counter.fetch_add(1, Ordering::Relaxed);
            if tick >= AUTO_CYCLE_TICKS {
                self.cycle_counter.store(0, Ordering::Relaxed);
                let cur = self.current_page.load(Ordering::Relaxed);
                self.current_page
                    .store((cur + 1) % PAGE_COUNT, Ordering::Relaxed);
            }
        }

        // Advance fan blade angle for animation (~8° per tick @ 500 ms ≈ 16°/s)
        let old_angle = self.fan_angle.load(Ordering::Relaxed);
        self.fan_angle
            .store((old_angle + 8) % 360, Ordering::Relaxed);

        let page = self.current_page.load(Ordering::Relaxed);
        let angle = self.fan_angle.load(Ordering::Relaxed);

        // Render at PV× for the frontend preview (high-res)
        let preview = render_page_scaled(page, ccxt, sys, angle, PV);

        // Cache high-res frame for frontend
        if let Ok(mut cache) = frame_cache().lock() {
            cache.clear();
            cache.extend_from_slice(&preview);
        }

        // Downsample PV× → 1× for the actual LCD hardware (also acts as SSAA)
        let lcd = downsample_frame(&preview);
        self.transfer_image(&lcd)
    }

    /// Return the last rendered RGBA frame as a base64-encoded string.
    pub fn get_last_frame_base64() -> String {
        let guard = frame_cache().lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_empty() {
            return String::new();
        }
        base64_encode(&guard)
    }

    /// Get the current widget layout.
    pub fn get_layout() -> NexusLayout {
        layout_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Replace the widget layout — persists to disk, takes effect on next render tick.
    pub fn set_layout(layout: NexusLayout) {
        if let Err(e) = save_layout(&layout) {
            warn!("NEXUS layout save failed: {e}");
        }
        if let Ok(mut guard) = layout_cache().lock() {
            *guard = layout;
        }
    }

    // ─── Low-level LCD ──────────────────────────────────────

    pub fn transfer_image(&self, rgba: &[u8]) -> Result<(), HidError> {
        let expected = pixel_buf_size();
        if rgba.len() != expected {
            return Err(HidError::Api(format!(
                "Image must be {expected} bytes, got {}",
                rgba.len()
            )));
        }

        // NEXUS LCD expects BGRA pixel order — swap R↔B channels.
        let mut bgra = rgba.to_vec();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        self.slot.with(|inner| {
            for (i, chunk) in bgra.chunks(NEXUS_LCD_MAX_PAYLOAD).enumerate() {
                let mut buf = vec![0u8; NEXUS_LCD_BUF_SIZE];
                buf[0] = 0x02;
                buf[1] = 0x05;
                buf[2] = 0x40;

                if chunk.len() < NEXUS_LCD_MAX_PAYLOAD {
                    buf[3] = 0x01; // last-packet flag
                }
                buf[4] = u8::try_from(i).unwrap_or(0);

                let len = u16::try_from(chunk.len()).unwrap_or(0);
                buf[6..8].copy_from_slice(&len.to_le_bytes());

                let end = 8 + chunk.len();
                if end <= NEXUS_LCD_BUF_SIZE {
                    buf[8..end].copy_from_slice(chunk);
                }

                inner.handle.write_raw(&buf)?;
            }
            Ok(())
        })
    }

    pub fn clear_display(&self) -> Result<(), HidError> {
        let black = vec![0u8; pixel_buf_size()];
        self.transfer_image(&black)
    }

    pub fn display_text(&self, label: &str, value: &str) -> Result<(), HidError> {
        let w = usize_from_u32(NEXUS_IMG_WIDTH);
        let h = usize_from_u32(NEXUS_IMG_HEIGHT);
        let mut rgba = vec![0u8; w * h * 4];

        render_text(&mut rgba, w, h, label, 4, 4, Rgba::WHITE);
        render_text(&mut rgba, w, h, value, 4, 28, Rgba::WHITE);

        self.transfer_image(&rgba)
    }

    pub fn is_connected(&self) -> bool {
        self.slot.is_connected()
    }
}

// ═════════════════════════════════════════════════════════════
//  Page Rendering
// ═════════════════════════════════════════════════════════════

const W: usize = 640;
const H: usize = 48;
/// Preview scale factor: render at PV× for the frontend, downsample for LCD.
const PV: usize = 4;

fn new_frame() -> Vec<u8> {
    vec![0u8; W * H * 4]
}

/// Box-filter downsample from PV× to 1× (acts as supersampled anti-aliasing for the LCD).
#[allow(clippy::cast_possible_truncation)]
fn downsample_frame(preview: &[u8]) -> Vec<u8> {
    let iw = W * PV;
    let s2 = (PV * PV) as u32;
    let mut lcd = vec![0u8; W * H * 4];
    for y in 0..H {
        for x in 0..W {
            let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..PV {
                for sx in 0..PV {
                    let px = ((y * PV + sy) * iw + (x * PV + sx)) * 4;
                    r += u32::from(preview[px]);
                    g += u32::from(preview[px + 1]);
                    b += u32::from(preview[px + 2]);
                    a += u32::from(preview[px + 3]);
                }
            }
            let dst = (y * W + x) * 4;
            lcd[dst] = (r / s2) as u8;
            lcd[dst + 1] = (g / s2) as u8;
            lcd[dst + 2] = (b / s2) as u8;
            lcd[dst + 3] = (a / s2) as u8;
        }
    }
    lcd
}

/// Render a complete page at the given scale (1 = native LCD, PV = preview).
fn render_page_scaled(
    page: u8,
    ccxt: Option<&CcxtStatus>,
    sys: Option<&NexusSysData>,
    fan_angle: u16,
    s: usize,
) -> Vec<u8> {
    let iw = W * s;
    let ih = H * s;
    let mut rgba = vec![0u8; iw * ih * 4];
    // Set alpha to 255 (opaque black background) — prevents transparency
    // artifacts when the frontend composites frames on the canvas.
    for px in rgba.chunks_exact_mut(4) {
        px[3] = 255;
    }

    // Clone the page layout + page count, then DROP the lock immediately
    // to avoid deadlock (render_widget_page_dots must not re-lock).
    let (widgets, page_count) = {
        let guard = layout_cache().lock().unwrap_or_else(|e| e.into_inner());
        let pc = guard.pages.len();
        let ws = guard
            .pages
            .get(usize::from(page))
            .map(|p| p.widgets.clone());
        (ws, pc)
    }; // lock released here

    if let Some(widgets) = &widgets {
        for widget in widgets {
            render_widget(
                &mut rgba, iw, ih, s, widget, ccxt, sys, fan_angle, page, page_count,
            );
        }
    }

    rgba
}

// ─── Widget dispatch ────────────────────────────────────────

/// Render any widget into the frame based on its [`WidgetKind`].
/// Coordinates are scaled by `s` from the layout's 640×48 space.
#[allow(clippy::too_many_arguments)]
fn render_widget(
    rgba: &mut [u8],
    iw: usize,
    ih: usize,
    s: usize,
    widget: &NexusWidget,
    ccxt: Option<&CcxtStatus>,
    sys: Option<&NexusSysData>,
    fan_angle: u16,
    current_page: u8,
    page_count: usize,
) {
    let x = usize::from(widget.x) * s;
    let y = usize::from(widget.y) * s;
    let w = usize::from(widget.w) * s;
    let h = usize::from(widget.h) * s;

    match &widget.kind {
        WidgetKind::FanIcon {
            channel,
            color,
            scale,
        } => {
            render_widget_fan(
                rgba, iw, ih, s, x, y, w, h, *channel, *color, *scale, ccxt, fan_angle,
            );
        }
        WidgetKind::Sensor {
            source,
            label,
            scale,
            color,
        } => {
            render_widget_sensor(
                rgba, iw, ih, s, x, y, w, *source, label, *scale, *color, ccxt, sys,
            );
        }
        WidgetKind::StatusBar {
            source,
            label,
            color,
            scale,
        } => {
            render_widget_status_bar(
                rgba, iw, ih, s, x, y, w, h, *source, label, *color, *scale, ccxt, sys,
            );
        }
        WidgetKind::Label { text, scale, color } => {
            let ts = f32_scale(*scale, s);
            render_text_scaled(rgba, iw, ih, text, x, y, ts, color.to_rgba());
        }
        WidgetKind::Clock { color, scale } => {
            render_widget_clock(rgba, iw, ih, s, x, y, *color, *scale)
        }
        WidgetKind::Divider { color } => render_widget_divider(rgba, iw, ih, x, y, h, *color),
        WidgetKind::PageDots { color } => {
            render_widget_page_dots(rgba, iw, ih, s, x, y, current_page, page_count, *color)
        }
    }
}

// ─── Individual widget renderers ────────────────────────────

/// Convert a fractional `scale` (e.g. 1.5) and base `s` (PV) into an integer pixel-scale.
/// scale=1.0, s=4 → 4;  scale=1.5, s=4 → 6;  scale=2.0, s=4 → 8.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn f32_scale(scale: f32, s: usize) -> usize {
    ((scale.max(0.5) * s as f32).round() as usize).max(1)
}

/// Spinning fan icon with RPM text below.
#[allow(clippy::too_many_arguments)]
fn render_widget_fan(
    rgba: &mut [u8],
    iw: usize,
    ih: usize,
    s: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    channel: u8,
    color: WidgetColor,
    scale: f32,
    ccxt: Option<&CcxtStatus>,
    fan_angle: u16,
) {
    let fan = ccxt.and_then(|data| {
        data.fans
            .iter()
            .filter(|f| f.connected)
            .nth(usize::from(channel))
    });

    let c = color.to_rgba();
    let ts = f32_scale(scale, s);

    // Fan circle centred in the upper portion, leaving room for RPM
    let text_reserve = 12 * s;
    let avail_h = h.saturating_sub(text_reserve);
    let radius = w.min(avail_h) / 2;
    if radius < 3 {
        return;
    }

    let cx = x + w / 2;
    let cy = y + avail_h / 2;
    render_fan_icon(rgba, iw, ih, cx, cy, radius, 5, fan_angle, c);

    // RPM readout
    if let Some(f) = fan {
        let rpm_str = format!("{}", f.rpm);
        let text_w = rpm_str.len() * 6 * ts;
        let text_x = x + w.saturating_sub(text_w) / 2;
        let text_y = y + avail_h + 2 * s;
        if text_y < ih {
            render_text_scaled(rgba, iw, ih, &rpm_str, text_x, text_y, ts, Rgba::DIM);
        }
    }
}

/// Label + scaled sensor value.
#[allow(clippy::too_many_arguments)]
fn render_widget_sensor(
    rgba: &mut [u8],
    iw: usize,
    ih: usize,
    s: usize,
    x: usize,
    y: usize,
    _w: usize,
    source: DataSource,
    label: &str,
    scale: f32,
    color: WidgetColor,
    ccxt: Option<&CcxtStatus>,
    sys: Option<&NexusSysData>,
) {
    let ts = f32_scale(scale, s);
    let value_y = if label.is_empty() {
        y
    } else {
        render_text_scaled(rgba, iw, ih, label, x, y, s, Rgba::DIM);
        y + 10 * s
    };

    match resolve_data_source(source, ccxt, sys) {
        Some(val) => render_text_scaled(rgba, iw, ih, &val, x, value_y, ts, color.to_rgba()),
        None => render_text_scaled(rgba, iw, ih, "--", x, value_y, ts, Rgba::DIM),
    }
}

/// Label + value + horizontal progress bar.
#[allow(clippy::too_many_arguments)]
fn render_widget_status_bar(
    rgba: &mut [u8],
    iw: usize,
    ih: usize,
    s: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    source: DataSource,
    label: &str,
    color: WidgetColor,
    scale: f32,
    ccxt: Option<&CcxtStatus>,
    sys: Option<&NexusSysData>,
) {
    let c = color.to_rgba();
    let ts = f32_scale(scale, s);
    render_text_scaled(rgba, iw, ih, label, x, y, ts, c);

    let val = resolve_data_source(source, ccxt, sys).unwrap_or_default();
    let label_px = label.len() * 6 * ts + 6 * ts;
    render_text_scaled(rgba, iw, ih, &val, x + label_px, y, ts, Rgba::WHITE);

    let bar_x = x + label_px + val.len() * 6 * ts + 6 * ts;
    let bar_w = w.saturating_sub(bar_x - x);
    let bar_h = h.min(6 * ts);
    let bar_y = y + 2 * ts;
    let pct = resolve_data_pct(source, sys);
    let fill = pct_of_f32(bar_w, pct, 100.0);
    render_hbar(rgba, iw, bar_x, bar_y, bar_w, fill, bar_h, c, Rgba::BAR_BG);
}

/// Clock: time + date.
fn render_widget_clock(
    rgba: &mut [u8],
    iw: usize,
    ih: usize,
    s: usize,
    x: usize,
    y: usize,
    color: WidgetColor,
    scale: f32,
) {
    let c = color.to_rgba();
    let ts = f32_scale(scale, s);
    let (time, date) = chrono_now();
    render_text_scaled(rgba, iw, ih, &time, x, y + 2 * s, 3 * ts, c);
    render_text_scaled(rgba, iw, ih, &date, x + 90 * ts, y + 34 * s, ts, Rgba::DIM);
}

/// Vertical divider line.
fn render_widget_divider(
    rgba: &mut [u8],
    iw: usize,
    ih: usize,
    x: usize,
    y: usize,
    h: usize,
    color: WidgetColor,
) {
    let c = color.to_rgba();
    for dy in 0..h {
        let py = y + dy;
        if py >= ih {
            break;
        }
        let px = (py * iw + x) * 4;
        if px + 3 < rgba.len() {
            rgba[px..px + 4].copy_from_slice(&c.0);
        }
    }
}

/// Page indicator dots.
fn render_widget_page_dots(
    rgba: &mut [u8],
    iw: usize,
    ih: usize,
    s: usize,
    x: usize,
    y: usize,
    current: u8,
    page_count: usize,
    base_color: WidgetColor,
) {
    let dot_size = 3 * s;
    let dot_spacing = 8 * s;
    let dot_off = 3 * s;
    let active_c = base_color.to_rgba();
    for i in 0..page_count {
        let dot_x = x + i * dot_spacing + dot_off;
        let dot_y = y;
        let color = if u8::try_from(i).unwrap_or(0) == current {
            active_c
        } else {
            Rgba::DIM
        };
        for dy in 0..dot_size {
            for dx in 0..dot_size {
                let px_y = dot_y + dy;
                if px_y >= ih {
                    continue;
                }
                let px = (px_y * iw + dot_x + dx) * 4;
                if px + 3 < rgba.len() {
                    rgba[px..px + 4].copy_from_slice(&color.0);
                }
            }
        }
    }
}

// ─── Fan icon pixel renderer ────────────────────────────────

/// Draw a spinning fan icon with `num_blades` blades at the given center & radius.
/// `angle_deg` (0..359) controls the current rotation.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn render_fan_icon(
    rgba: &mut [u8],
    iw: usize,
    ih: usize,
    cx: usize,
    cy: usize,
    radius: usize,
    num_blades: u16,
    angle_deg: u16,
    color: Rgba,
) {
    let r = radius as f64;
    let hub_r = r * 0.25;
    let blade_half_angle = std::f64::consts::PI / f64::from(num_blades) * 0.55;
    let base_angle = f64::from(angle_deg) * std::f64::consts::PI / 180.0;

    let min_x = cx.saturating_sub(radius + 1);
    let max_x = (cx + radius + 1).min(iw);
    let min_y = cy.saturating_sub(radius + 1);
    let max_y = (cy + radius + 1).min(ih);

    for py in min_y..max_y {
        for px_x in min_x..max_x {
            let dx = px_x as f64 - cx as f64;
            let dy = py as f64 - cy as f64;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist > r + 0.5 {
                continue;
            }

            let px = (py * iw + px_x) * 4;
            if px + 3 >= rgba.len() {
                continue;
            }

            // Outer ring
            if dist > r - 1.0 {
                rgba[px..px + 4].copy_from_slice(&Rgba::CYAN.0);
                continue;
            }

            // Hub circle
            if dist < hub_r {
                rgba[px..px + 4].copy_from_slice(&color.0);
                continue;
            }

            // Blade test
            let pixel_angle = dy.atan2(dx) - base_angle;
            let two_pi = 2.0 * std::f64::consts::PI;
            let pa = ((pixel_angle % two_pi) + two_pi) % two_pi;

            let blade_spacing = two_pi / f64::from(num_blades);
            let mut on_blade = false;
            for b in 0..num_blades {
                let blade_center = f64::from(b) * blade_spacing;
                let mut diff = (pa - blade_center).abs();
                if diff > std::f64::consts::PI {
                    diff = two_pi - diff;
                }

                let taper = 1.0 - (dist - hub_r) / (r - hub_r) * 0.5;
                if diff < blade_half_angle * taper {
                    on_blade = true;
                    break;
                }
            }

            if on_blade {
                let brightness = 0.6 + 0.4 * (1.0 - dist / r);
                let c = [
                    (f64::from(color.0[0]) * brightness) as u8,
                    (f64::from(color.0[1]) * brightness) as u8,
                    (f64::from(color.0[2]) * brightness) as u8,
                    255,
                ];
                rgba[px..px + 4].copy_from_slice(&c);
            } else {
                rgba[px..px + 4].copy_from_slice(&Rgba::FAN_BG.0);
            }
        }
    }
}

// ─── Data source resolution ─────────────────────────────────

/// Resolve a [`DataSource`] to its formatted display string.
fn resolve_data_source(
    source: DataSource,
    ccxt: Option<&CcxtStatus>,
    sys: Option<&NexusSysData>,
) -> Option<String> {
    match source {
        DataSource::WaterTemp => ccxt?
            .temps
            .iter()
            .find(|t| t.connected)
            .map(|t| format!("{:.0}C", t.temp.get())),
        DataSource::CpuTemp =>
        {
            #[allow(clippy::cast_possible_truncation)]
            sys?.cpu_temp.map(|t| format!("{}C", t as u32))
        }
        DataSource::GpuTemp =>
        {
            #[allow(clippy::cast_possible_truncation)]
            sys?.gpu_temp.map(|t| format!("{}C", t as u32))
        }
        DataSource::TotalPower =>
        {
            #[allow(clippy::cast_possible_truncation)]
            sys?.total_power_w.map(|w| format!("{}W", w as u32))
        }
        DataSource::CpuUsage => sys.map(|s| format!("{:.0}%", s.cpu_usage)),
        DataSource::RamUsage => sys.map(|s| format!("{:.0}%", s.ram_used_pct)),
        DataSource::CpuFreq => sys.map(|s| {
            if s.cpu_freq_mhz > 1000.0 {
                format!("{:.1}GHZ", s.cpu_freq_mhz / 1000.0)
            } else {
                format!("{:.0}MHZ", s.cpu_freq_mhz)
            }
        }),
        DataSource::RamTotal => sys.map(|s| format!("{:.0}G", s.ram_total_gib)),
    }
}

/// Resolve a [`DataSource`] to a 0–100 percentage for bar fill.
fn resolve_data_pct(source: DataSource, sys: Option<&NexusSysData>) -> f32 {
    sys.map_or(0.0, |s| match source {
        DataSource::CpuUsage => s.cpu_usage,
        DataSource::RamUsage => s.ram_used_pct,
        _ => 0.0,
    })
}

/// Get current time as (HH:MM, DD.MM.YYYY).
/// Uses a simple system-time approach without pulling in the `chrono` crate.
fn chrono_now() -> (String, String) {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Apply CET offset (+1h) — good enough for Germany
    let local = secs + 3600;
    // Add another hour if roughly in DST range (last Sunday March – last Sunday October)
    // Simplified: months 4-9 always DST
    let day_of_year = (local / 86400) % 365;
    let in_dst = (90..=273).contains(&day_of_year);
    let local = if in_dst { local + 3600 } else { local };

    let sec_of_day = local % 86400;
    let hour = sec_of_day / 3600;
    let min = (sec_of_day % 3600) / 60;

    // Date (simplified from days since epoch)
    // days since epoch — u64 fits in i64 for any realistic date
    #[allow(clippy::cast_possible_wrap)]
    let days = (local / 86400) as i64;
    let (y, m, d) = days_to_ymd(days + 719_468); // Civil days

    (format!("{hour:02}:{min:02}"), format!("{d:02}.{m:02}.{y}"))
}

/// Convert a civil day number to (year, month, day).
/// Algorithm from Howard Hinnant's `chrono`-compatible date library.
#[allow(clippy::cast_possible_truncation)]
fn days_to_ymd(g: i64) -> (i64, u32, u32) {
    let era = g.div_euclid(146_097);
    let doe = g.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = era * 400 + i64::from(yoe);
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ═════════════════════════════════════════════════════════════
//  Base64 Encoder (avoids external crate under strict lints)
// ═════════════════════════════════════════════════════════════

/// RFC 4648 base64 encoder — no external crate needed.
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0u32, u32::from);
        let b2 = chunk.get(2).copied().map_or(0u32, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;

        let idx = |shift: u32| -> usize {
            // Masked to 0..63, always fits in usize
            usize::try_from((triple >> shift) & 0x3F).unwrap_or(0)
        };
        out.push(char::from(TABLE[idx(18)]));
        out.push(char::from(TABLE[idx(12)]));
        out.push(if chunk.len() > 1 {
            char::from(TABLE[idx(6)])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(TABLE[idx(0)])
        } else {
            '='
        });
    }
    out
}

// ═════════════════════════════════════════════════════════════
//  Drawing Primitives
// ═════════════════════════════════════════════════════════════

/// Safe percentage → pixel width: `pct_of(max_w, 75, 100) == max_w * 75 / 100`.
fn pct_of(max_w: usize, value: u32, scale: u32) -> usize {
    if scale == 0 {
        return 0;
    }
    max_w.saturating_mul(usize::try_from(value.min(scale)).unwrap_or(0))
        / usize::try_from(scale).unwrap_or(1)
}

/// Safe f32 percentage → pixel width.
fn pct_of_f32(max_w: usize, value: f32, scale: f32) -> usize {
    if scale <= 0.0 {
        return 0;
    }
    let ratio = (value.max(0.0) / scale).min(1.0);
    // max_w is at most 640, well within f64 precision
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    {
        (max_w as f64 * f64::from(ratio)) as usize
    }
}

fn pixel_buf_size() -> usize {
    W * H * 4
}

fn usize_from_u32(v: u32) -> usize {
    usize::try_from(v).unwrap_or(0)
}

fn read_nexus_firmware(handle: &HidHandle) -> String {
    let mut buf = vec![0u8; 32];
    buf[0] = 5;
    match handle.send_feature_report(&buf) {
        Ok(()) => "1.0.0".to_string(),
        Err(e) => {
            warn!("NEXUS: firmware read failed: {e}");
            "unknown".to_string()
        }
    }
}

fn default_buttons() -> Vec<NexusButton> {
    let zone = u16::try_from(NEXUS_IMG_WIDTH / 5).unwrap_or(128);
    (0u8..5)
        .map(|i| {
            let i16 = u16::from(i);
            NexusButton {
                index: i,
                pos_min: i16 * zone,
                pos_max: (i16 + 1) * zone - 1,
                label: format!("Button {}", i + 1),
            }
        })
        .collect()
}

/// Horizontal bar primitive.
#[allow(clippy::too_many_arguments)]
fn render_hbar(
    rgba: &mut [u8],
    iw: usize,
    x: usize,
    y: usize,
    max_w: usize,
    fill_w: usize,
    h: usize,
    fill_color: Rgba,
    bg_color: Rgba,
) {
    for dy in 0..h {
        for dx in 0..max_w {
            let color = if dx < fill_w { fill_color } else { bg_color };
            let px = ((y + dy) * iw + x + dx) * 4;
            if px + 3 < rgba.len() {
                rgba[px..px + 4].copy_from_slice(&color.0);
            }
        }
    }
}

/// Render text at integer scale (1 = normal, 2 = double, 3 = triple).
fn render_text_scaled(
    rgba: &mut [u8],
    img_width: usize,
    img_height: usize,
    text: &str,
    start_x: usize,
    start_y: usize,
    scale: usize,
    color: Rgba,
) {
    let char_w = 6 * scale;

    for (ci, ch) in text.chars().enumerate() {
        let x_off = start_x + ci * char_w;
        if x_off + 5 * scale > img_width {
            break;
        }

        let bitmap = glyph(ch);
        for (row, &bits) in bitmap.iter().enumerate() {
            for col in 0u8..5 {
                if bits & (1 << (4 - col)) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px_x = x_off + usize::from(col) * scale + sx;
                            let px_y = start_y + row * scale + sy;
                            if px_y >= img_height {
                                continue;
                            }
                            let px = (px_y * img_width + px_x) * 4;
                            if px + 3 < rgba.len() {
                                rgba[px..px + 4].copy_from_slice(&color.0);
                            }
                        }
                    }
                }
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════
//  Pixel Colour Newtype
// ═════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy)]
struct Rgba([u8; 4]);

impl Rgba {
    const WHITE: Self = Self([255, 255, 255, 255]);
    const CYAN: Self = Self([0, 200, 220, 255]);
    const RED: Self = Self([248, 113, 113, 255]);
    const AMBER: Self = Self([251, 191, 36, 255]);
    const PURPLE: Self = Self([120, 80, 160, 255]);
    const DIM: Self = Self([100, 100, 100, 255]);
    const DIM_DARK: Self = Self([50, 50, 55, 255]);
    const BAR_BG: Self = Self([40, 40, 40, 255]);
    const FAN_BG: Self = Self([20, 20, 24, 255]);
}

// ═════════════════════════════════════════════════════════════
//  5×7 Bitmap Font
// ═════════════════════════════════════════════════════════════

const FONT_5X7: [[u8; 7]; 95] = {
    const F: [u8; 7] = [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F]; // □ fallback
    let mut t = [F; 95];

    // Punctuation & symbols
    t[0x00] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // ' '
    t[0x05] = [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03]; // '%'
    t[0x0D] = [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00]; // '-'
    t[0x0E] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04]; // '.'
    t[0x0F] = [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10]; // '/'

    // Digits 0–9
    t[0x10] = [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E]; // '0'
    t[0x11] = [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E]; // '1'
    t[0x12] = [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F]; // '2'
    t[0x13] = [0x0E, 0x11, 0x01, 0x06, 0x01, 0x11, 0x0E]; // '3'
    t[0x14] = [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02]; // '4'
    t[0x15] = [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E]; // '5'
    t[0x16] = [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E]; // '6'
    t[0x17] = [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]; // '7'
    t[0x18] = [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E]; // '8'
    t[0x19] = [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C]; // '9'

    // ':'
    t[0x1A] = [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x00];

    // Uppercase A–Z
    t[0x21] = [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]; // A
    t[0x22] = [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]; // B
    t[0x23] = [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E]; // C
    t[0x24] = [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E]; // D
    t[0x25] = [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]; // E
    t[0x26] = [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10]; // F
    t[0x27] = [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E]; // G
    t[0x28] = [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]; // H
    t[0x29] = [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]; // I
    t[0x2A] = [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C]; // J
    t[0x2B] = [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]; // K
    t[0x2C] = [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F]; // L
    t[0x2D] = [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]; // M
    t[0x2E] = [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11]; // N
    t[0x2F] = [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]; // O
    t[0x30] = [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10]; // P
    t[0x31] = [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D]; // Q
    t[0x32] = [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11]; // R
    t[0x33] = [0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E]; // S
    t[0x34] = [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]; // T
    t[0x35] = [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]; // U
    t[0x36] = [0x11, 0x11, 0x11, 0x11, 0x0A, 0x0A, 0x04]; // V
    t[0x37] = [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11]; // W
    t[0x38] = [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11]; // X
    t[0x39] = [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04]; // Y
    t[0x3A] = [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]; // Z

    // Lowercase a–z → same as uppercase
    t[0x41] = t[0x21];
    t[0x42] = t[0x22];
    t[0x43] = t[0x23];
    t[0x44] = t[0x24];
    t[0x45] = t[0x25];
    t[0x46] = t[0x26];
    t[0x47] = t[0x27];
    t[0x48] = t[0x28];
    t[0x49] = t[0x29];
    t[0x4A] = t[0x2A];
    t[0x4B] = t[0x2B];
    t[0x4C] = t[0x2C];
    t[0x4D] = t[0x2D];
    t[0x4E] = t[0x2E];
    t[0x4F] = t[0x2F];
    t[0x50] = t[0x30];
    t[0x51] = t[0x31];
    t[0x52] = t[0x32];
    t[0x53] = t[0x33];
    t[0x54] = t[0x34];
    t[0x55] = t[0x35];
    t[0x56] = t[0x36];
    t[0x57] = t[0x37];
    t[0x58] = t[0x38];
    t[0x59] = t[0x39];
    t[0x5A] = t[0x3A];

    t
};

fn glyph(ch: char) -> [u8; 7] {
    u32::from(ch)
        .checked_sub(0x20)
        .and_then(|idx| usize::try_from(idx).ok())
        .and_then(|idx| FONT_5X7.get(idx).copied())
        .unwrap_or([0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F])
}

fn render_text(
    rgba: &mut [u8],
    img_width: usize,
    img_height: usize,
    text: &str,
    start_x: usize,
    start_y: usize,
    color: Rgba,
) {
    const CHAR_WIDTH: usize = 6;

    for (ci, ch) in text.chars().enumerate() {
        let x_off = start_x + ci * CHAR_WIDTH;
        if x_off + 5 > img_width {
            break;
        }

        let bitmap = glyph(ch);
        for (row, &bits) in bitmap.iter().enumerate() {
            let y = start_y + row;
            if y >= img_height {
                break;
            }
            for col in 0u8..5 {
                if bits & (1 << (4 - col)) != 0 {
                    let px = (y * img_width + x_off + usize::from(col)) * 4;
                    if px + 3 < rgba.len() {
                        rgba[px..px + 4].copy_from_slice(&color.0);
                    }
                }
            }
        }
    }
}
