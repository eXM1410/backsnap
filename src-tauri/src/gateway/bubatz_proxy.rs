//! Reverse proxy to the Bubatz FastAPI on Pi4 (192.168.0.21:8001).
//! Forwards requests under /bubatz/* to the Pi's /api/* endpoints.
//! Govee requests are routed to the Pi5 govee-controller (192.168.0.8:4080).

use axum::{
    body::Body,
    extract::{Path, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};

const PI4_BASE: &str = "http://192.168.0.21:8001";
const PI5_GOVEE: &str = "http://192.168.0.8:4080";

/// Proxy any request to Pi4's API.
async fn proxy_bubatz(Path(path): Path<String>, req: Request) -> Result<Response, StatusCode> {
    // Route govee requests to Pi5 govee-controller
    let url = if path.starts_with("govee") {
        format!("{PI5_GOVEE}/api/{path}")
    } else {
        format!("{PI4_BASE}/api/{path}")
    };
    let method = req.method().clone();
    let headers = req.headers().clone();

    // Read the body
    let body_bytes = axum::body::to_bytes(req.into_body(), 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Forward via reqwest (blocking → spawn_blocking)
    let resp = tokio::task::spawn_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Client: {e}"))?;

        let mut req_builder = client.request(method, &url);

        // Forward content-type header
        if let Some(ct) = headers.get("content-type") {
            req_builder = req_builder.header("content-type", ct);
        }

        if !body_bytes.is_empty() {
            req_builder = req_builder.body(body_bytes.to_vec());
        }

        let resp = req_builder.send().map_err(|e| format!("Pi4 proxy: {e}"))?;
        let status = resp.status().as_u16();
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        let body = resp.bytes().map_err(|e| format!("Body: {e}"))?;

        Ok::<_, String>((status, ct, body.to_vec()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let (status, content_type, body) = resp;
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    Ok(Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

/// Camera stream: return Pi4's WHEP player URL for WebView embedding.
async fn camera_url() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "playerUrl": format!("{PI4_BASE}/api/camera/player"),
        "whepProxy": format!("{PI4_BASE}/api/camera/whep-proxy"),
    }))
}

pub fn routes() -> Router {
    Router::new()
        .route("/bubatz/{*path}", any(proxy_bubatz))
        .route("/stream/camera", get(camera_url))
}
