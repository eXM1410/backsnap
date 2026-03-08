//! Embedded HTTP API gateway — exposes Arclight's RGB, system monitoring,
//! and Bubatz proxy as REST endpoints for the mobile app.
//!
//! Runs as a background Axum server on port 3100, intended to be tunneled
//! via ngrok or loca.lt for remote access.

mod assistant;
mod auth;
mod bubatz_proxy;
mod lighting;
mod system;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};

const GATEWAY_PORT: u16 = 3100;

/// Build the full Axum router with all sub-routes.
fn build_router(token: String) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api = Router::new()
        .merge(lighting::routes())
        .merge(system::routes())
        .merge(bubatz_proxy::routes())
        .merge(assistant::routes())
        .layer(axum::middleware::from_fn(move |req, next| {
            let t = token.clone();
            auth::check_token(req, next, t)
        }));

    Router::new()
        .nest("/api", api)
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/tunnel-url", axum::routing::get(tunnel_url))
        .layer(cors)
}

/// Return the current tunnel URL from ~/tunnel.url (written by cloudflared start script).
async fn tunnel_url() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let path = std::path::PathBuf::from(home).join("tunnel.url");
    match tokio::fs::read_to_string(&path).await {
        Ok(url) => url.trim().to_string(),
        Err(_) => String::new(),
    }
}

/// Spawn the gateway HTTP server on a dedicated OS thread with its own tokio runtime.
/// Called from Tauri's `setup()` hook (which runs outside tokio).
pub fn spawn_gateway(token: String) {
    std::thread::Builder::new()
        .name("gateway".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("gateway: cannot create tokio runtime");

            rt.block_on(async move {
                let app = build_router(token);
                let addr = std::net::SocketAddr::from(([0, 0, 0, 0], GATEWAY_PORT));
                eprintln!("Gateway: listening on http://{addr}");

                let listener = match tokio::net::TcpListener::bind(addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("Gateway: bind fehlgeschlagen: {e}");
                        return;
                    }
                };

                if let Err(e) = axum::serve(listener, app).await {
                    eprintln!("Gateway: server error: {e}");
                }
            });
        })
        .expect("gateway: cannot spawn thread");
}
