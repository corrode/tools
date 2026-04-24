//! Authentication for the monitoring dashboard.
//!
//! Protects all `/monitoring` routes behind a token read from the
//! `MONITORING_TOKEN` environment variable. The token can be provided via:
//!
//! 1. `monitoring_session` cookie (set by the `/monitoring/login` endpoint)
//! 2. `?token=<token>` query parameter
//!
//! **Typical browser flow:** visit `/monitoring/login?token=SECRET` once.
//! This sets an `HttpOnly` cookie and redirects to the dashboard. All
//! subsequent requests are authenticated automatically.
//!
//! Returns `401 Unauthorized` on mismatch and `500 Internal Server Error` if
//! the environment variable is not set.

use axum::{
    extract::{Extension, Request},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};

/// Name of the session cookie.
const COOKIE_NAME: &str = "monitoring_session";

/// Axum middleware that enforces token authentication.
///
/// Checks (in order): cookie, `?token=` query param.
/// Intended to be used with [`axum::middleware::from_fn`] on the monitoring
/// router's `route_layer`.
pub async fn require_monitoring_token(
    headers: HeaderMap,
    Extension(expected): Extension<String>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let provided = token_from_cookie(&headers).or_else(|| token_from_query(&request));

    match provided {
        Some(token) if token == expected => Ok(next.run(request).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Login endpoint: validates `?token=`, sets a session cookie, and redirects
/// to the dashboard.
///
/// ```text
/// GET /monitoring/login?token=SECRET
///   → 303 See Other → /monitoring/
///   + Set-Cookie: monitoring_session=SECRET; HttpOnly; SameSite=Lax; Path=/monitoring
/// ```
///
/// If the token is wrong or missing, returns `401`.
pub async fn login(
    headers: HeaderMap,
    Extension(expected): Extension<String>,
    request: Request,
) -> Result<Response, StatusCode> {
    // Accept token from query param or existing cookie
    let provided = token_from_query(&request).or_else(|| token_from_cookie(&headers));

    match provided {
        Some(token) if token == expected => {
            let cookie = format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/monitoring");
            let mut response = Redirect::to("/monitoring").into_response();
            response.headers_mut().insert(
                header::SET_COOKIE,
                cookie
                    .parse()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            );
            Ok(response)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Extract the token from the `monitoring_session` cookie.
fn token_from_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{COOKIE_NAME}=")))
        .map(str::to_string)
}

/// Extract the token from the `?token=<value>` query parameter.
fn token_from_query(request: &Request) -> Option<String> {
    request
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("token=")))
        .map(str::to_string)
}
