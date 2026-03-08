use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::config::load_config;

const DEFAULT_PI4_HOST: &str = "192.168.0.21:8001";
const MAX_HISTORY_POINTS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiTentSensor {
    pub temp: f64,
    pub humi: f64,
    pub vpd: f64,
    pub batt: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiTentLight {
    pub power: bool,
    pub brightness: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiTentTank {
    pub percent: f64,
    pub liters: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiTentStatus {
    pub ok: bool,
    pub base_url: String,
    pub sensor: Option<PiTentSensor>,
    pub light: Option<PiTentLight>,
    pub tank: Option<PiTentTank>,
    pub temp_history: Vec<f64>,
    pub humi_history: Vec<f64>,
    pub vpd_history: Vec<f64>,
    pub brightness_history: Vec<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiTentHistory {
    pub ok: bool,
    pub temp_history: Vec<f64>,
    pub humi_history: Vec<f64>,
    pub vpd_history: Vec<f64>,
    pub brightness_history: Vec<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SensorCurrentResponse {
    status: String,
    temp: f64,
    humi: f64,
    vpd: f64,
    batt: u8,
}

#[derive(Debug, Deserialize)]
struct LightEnvelope {
    success: bool,
    status: LightStatus,
}

#[derive(Debug, Deserialize)]
struct LightStatus {
    power: bool,
    brightness: u8,
}

#[derive(Debug, Deserialize)]
struct TankResponse {
    percent: f64,
    liters: f64,
}

#[derive(Debug, Deserialize)]
struct SensorHistoryPoint {
    temp: f64,
    humi: f64,
    vpd: f64,
}

#[derive(Debug, Deserialize)]
struct BrightnessHistoryPoint {
    brightness: f64,
}

fn detect_pi4_host() -> String {
    let Ok(config) = load_config() else {
        return DEFAULT_PI4_HOST.to_string();
    };

    let found = config.pi_remote.devices.into_iter().find(|device| {
        let id = device.id.to_lowercase();
        let label = device.label.to_lowercase();
        let model = device.model.to_lowercase();
        id == "pi4"
            || label.contains("pi4")
            || label.contains("pi 4")
            || model.contains("pi4")
            || model.contains("pi 4")
            || device.ip == "192.168.0.21"
    });

    found
        .map(|device| format!("{}:8001", device.ip))
        .unwrap_or_else(|| DEFAULT_PI4_HOST.to_string())
}

fn build_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(4))
        .connect_timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))
}

fn sample_history<T, F>(items: &[T], max_points: usize, mut map: F) -> Vec<f64>
where
    F: FnMut(&T) -> f64,
{
    if items.is_empty() {
        return Vec::new();
    }

    if items.len() <= max_points {
        return items.iter().map(map).collect();
    }

    let last_index = items.len() - 1;
    let max_index = max_points - 1;
    (0..max_points)
        .map(|i| {
            let idx = i * last_index / max_index;
            map(&items[idx])
        })
        .collect()
}

#[tauri::command]
pub async fn get_pi_tent_status() -> PiTentStatus {
    let host = detect_pi4_host();
    let base_url = format!("http://{host}");

    let client = match build_client() {
        Ok(client) => client,
        Err(error) => {
            return PiTentStatus {
                ok: false,
                base_url,
                sensor: None,
                light: None,
                tank: None,
                temp_history: Vec::new(),
                humi_history: Vec::new(),
                vpd_history: Vec::new(),
                brightness_history: Vec::new(),
                error: Some(error),
            };
        }
    };

    let sensor_url = format!("{base_url}/api/sensor/sensor");
    let light_url = format!("{base_url}/api/marshydro/status");
    let tank_url = format!("{base_url}/api/distance/tank");

    let mut errors = Vec::new();

    let sensor = match client.get(&sensor_url).send().await {
        Ok(response) => match response.json::<SensorCurrentResponse>().await {
            Ok(data) if data.status == "ok" => Some(PiTentSensor {
                temp: data.temp,
                humi: data.humi,
                vpd: data.vpd,
                batt: data.batt,
            }),
            Ok(_) => {
                errors.push("sensor endpoint returned non-ok status".to_string());
                None
            }
            Err(e) => {
                errors.push(format!("sensor parse failed: {e}"));
                None
            }
        },
        Err(e) => {
            errors.push(format!("sensor request failed: {e}"));
            None
        }
    };

    let light = match client.get(&light_url).send().await {
        Ok(response) => match response.json::<LightEnvelope>().await {
            Ok(data) if data.success => Some(PiTentLight {
                power: data.status.power,
                brightness: data.status.brightness,
            }),
            Ok(_) => {
                errors.push("light endpoint returned success=false".to_string());
                None
            }
            Err(e) => {
                errors.push(format!("light parse failed: {e}"));
                None
            }
        },
        Err(e) => {
            errors.push(format!("light request failed: {e}"));
            None
        }
    };

    let tank = match client.get(&tank_url).send().await {
        Ok(response) => match response.json::<TankResponse>().await {
            Ok(data) => Some(PiTentTank {
                percent: data.percent,
                liters: data.liters,
            }),
            Err(e) => {
                errors.push(format!("tank parse failed: {e}"));
                None
            }
        },
        Err(e) => {
            errors.push(format!("tank request failed: {e}"));
            None
        }
    };

    let ok = sensor.is_some() || light.is_some() || tank.is_some();
    let error = if errors.is_empty() {
        None
    } else {
        Some(errors.join(" | "))
    };

    PiTentStatus {
        ok,
        base_url,
        sensor,
        light,
        tank,
        temp_history: Vec::new(),
        humi_history: Vec::new(),
        vpd_history: Vec::new(),
        brightness_history: Vec::new(),
        error,
    }
}

#[tauri::command]
pub async fn get_pi_tent_history() -> PiTentHistory {
    let host = detect_pi4_host();
    let base_url = format!("http://{host}");

    let client = match build_client() {
        Ok(client) => client,
        Err(error) => {
            return PiTentHistory {
                ok: false,
                temp_history: Vec::new(),
                humi_history: Vec::new(),
                vpd_history: Vec::new(),
                brightness_history: Vec::new(),
                error: Some(error),
            };
        }
    };

    let sensor_history_url = format!("{base_url}/api/sensor/sensor/history?hours=1");
    let brightness_history_url = format!("{base_url}/api/marshydro/brightness/history?hours=1");
    let mut errors = Vec::new();

    let (temp_history, humi_history, vpd_history) =
        match client.get(&sensor_history_url).send().await {
            Ok(response) => match response.json::<Vec<SensorHistoryPoint>>().await {
                Ok(items) => (
                    sample_history(&items, MAX_HISTORY_POINTS, |item| item.temp),
                    sample_history(&items, MAX_HISTORY_POINTS, |item| item.humi),
                    sample_history(&items, MAX_HISTORY_POINTS, |item| item.vpd * 20.0),
                ),
                Err(e) => {
                    errors.push(format!("sensor history parse failed: {e}"));
                    (Vec::new(), Vec::new(), Vec::new())
                }
            },
            Err(e) => {
                errors.push(format!("sensor history request failed: {e}"));
                (Vec::new(), Vec::new(), Vec::new())
            }
        };

    let brightness_history = match client.get(&brightness_history_url).send().await {
        Ok(response) => match response.json::<Vec<BrightnessHistoryPoint>>().await {
            Ok(items) => sample_history(&items, MAX_HISTORY_POINTS, |item| item.brightness),
            Err(e) => {
                errors.push(format!("brightness history parse failed: {e}"));
                Vec::new()
            }
        },
        Err(e) => {
            errors.push(format!("brightness history request failed: {e}"));
            Vec::new()
        }
    };

    let ok = !(temp_history.is_empty() && humi_history.is_empty() && brightness_history.is_empty());
    let error = if errors.is_empty() {
        None
    } else {
        Some(errors.join(" | "))
    };

    PiTentHistory {
        ok,
        temp_history,
        humi_history,
        vpd_history,
        brightness_history,
        error,
    }
}
