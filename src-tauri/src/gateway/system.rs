//! System monitoring endpoint — exposes sysmon data as JSON.

use axum::{routing::get, Json, Router};

async fn get_system_status() -> Result<Json<crate::sysmon::SystemMonitorData>, String> {
    let data = tokio::task::spawn_blocking(crate::sysmon::read_system_monitor)
        .await
        .map_err(|e| format!("Thread panic: {e}"))?;
    Ok(Json(data))
}

pub fn routes() -> Router {
    Router::new().route("/system/status", get(get_system_status))
}
