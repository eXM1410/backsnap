//! Lighting REST endpoints — wraps existing Tauri commands as HTTP routes.

use axum::{
    extract::Path,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::commands::{corsair, lighting, openrgb};

// ── Request types ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PowerRequest {
    power: bool,
}

#[derive(Deserialize)]
pub struct ColorRequest {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Deserialize)]
pub struct EffectRequest {
    mode_id: usize,
    #[serde(default)]
    speed: Option<u32>,
    #[serde(default)]
    colors: Option<Vec<openrgb::RgbColor>>,
}

// ── Handlers ─────────────────────────────────────────────────

async fn master_power(Json(req): Json<PowerRequest>) -> Json<lighting::MasterLightResult> {
    let result = tokio::task::spawn_blocking(move || lighting::lighting_master_power(req.power))
        .await
        .unwrap_or_else(|_| lighting::MasterLightResult {
            power: false,
            corsair_ok: false,
            corsair_msg: "Thread panic".into(),
            openrgb_ok: false,
            openrgb_msg: "Thread panic".into(),
            govee_ok: false,
            govee_msg: "Thread panic".into(),
        });
    Json(result)
}

async fn get_devices() -> Result<Json<openrgb::RgbStatus>, String> {
    let status = tokio::task::spawn_blocking(openrgb::openrgb_status)
        .await
        .map_err(|e| format!("Thread panic: {e}"))?
        .map_err(|e| e.to_string())?;
    Ok(Json(status))
}

async fn connect_devices() -> Result<Json<openrgb::RgbStatus>, String> {
    let status = tokio::task::spawn_blocking(openrgb::openrgb_connect)
        .await
        .map_err(|e| format!("Thread panic: {e}"))?
        .map_err(|e| e.to_string())?;
    Ok(Json(status))
}

async fn set_device_color(
    Path(device_id): Path<String>,
    Json(req): Json<ColorRequest>,
) -> Result<Json<String>, String> {
    let msg = tokio::task::spawn_blocking(move || {
        openrgb::openrgb_set_color(device_id, req.r, req.g, req.b)
    })
    .await
    .map_err(|e| format!("Thread panic: {e}"))?
    .map_err(|e| e.to_string())?;
    Ok(Json(msg))
}

async fn set_device_effect(
    Path(device_id): Path<String>,
    Json(req): Json<EffectRequest>,
) -> Result<Json<String>, String> {
    let msg = tokio::task::spawn_blocking(move || {
        openrgb::openrgb_set_mode(device_id, req.mode_id, req.speed, None, None, req.colors)
    })
    .await
    .map_err(|e| format!("Thread panic: {e}"))?
    .map_err(|e| e.to_string())?;
    Ok(Json(msg))
}

async fn device_off(Path(device_id): Path<String>) -> Result<Json<String>, String> {
    let msg = tokio::task::spawn_blocking(move || openrgb::openrgb_off(device_id))
        .await
        .map_err(|e| format!("Thread panic: {e}"))?
        .map_err(|e| e.to_string())?;
    Ok(Json(msg))
}

async fn all_off() -> Result<Json<String>, String> {
    let msg = tokio::task::spawn_blocking(openrgb::openrgb_all_off)
        .await
        .map_err(|e| format!("Thread panic: {e}"))?
        .map_err(|e| e.to_string())?;
    Ok(Json(msg))
}

async fn get_corsair() -> Result<Json<serde_json::Value>, String> {
    let status = tokio::task::spawn_blocking(corsair::corsair_ccxt_poll)
        .await
        .map_err(|e| format!("Thread panic: {e}"))?
        .map_err(|e| e.to_string())?;
    serde_json::to_value(status)
        .map(Json)
        .map_err(|e| e.to_string())
}

async fn set_corsair_rgb(Json(req): Json<ColorRequest>) -> Result<Json<String>, String> {
    let msg = tokio::task::spawn_blocking(move || {
        corsair::corsair_set_rgb(corsair::SetRgbRequest {
            r: req.r,
            g: req.g,
            b: req.b,
        })
    })
    .await
    .map_err(|e| format!("Thread panic: {e}"))?
    .map_err(|e| e.to_string())?;
    Ok(Json(msg))
}

// ── Router ───────────────────────────────────────────────────

pub fn routes() -> Router {
    Router::new()
        .route("/lighting/power", post(master_power))
        .route("/lighting/devices", get(get_devices))
        .route("/lighting/connect", post(connect_devices))
        .route("/lighting/all-off", post(all_off))
        .route("/lighting/device/{id}/color", post(set_device_color))
        .route("/lighting/device/{id}/effect", post(set_device_effect))
        .route("/lighting/device/{id}/off", post(device_off))
        .route("/corsair/status", get(get_corsair))
        .route("/corsair/rgb", post(set_corsair_rgb))
}
