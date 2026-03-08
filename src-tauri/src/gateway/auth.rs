//! Simple bearer-token authentication middleware.

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};

/// Axum middleware: rejects requests without a valid `Authorization: Bearer <token>`.
/// The `/health` endpoint is excluded (handled before this layer).
pub async fn check_token(
    req: Request,
    next: Next,
    expected: String,
) -> Result<Response, StatusCode> {
    // Allow empty token = no auth required (for local development)
    if expected.is_empty() {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(val) if val.starts_with("Bearer ") => {
            let token = &val[7..];
            if token == expected {
                Ok(next.run(req).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
