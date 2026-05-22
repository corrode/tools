//! `GET /api/v1/health` — lightweight liveness probe for the JSON API.

use serde::Serialize;
use utoipa::ToSchema;

use axum::Json;

/// Response body for `GET /api/v1/health`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HealthResponse {
    /// Always `"ok"` when this endpoint returns `200`.
    #[schema(example = "ok")]
    pub status: &'static str,
    /// API version string. Bumps follow semver of the API contract, not the
    /// server binary.
    #[schema(example = "1.0.0")]
    pub version: &'static str,
}

/// Liveness probe.
///
/// Returns a small JSON document and a `200` status. Intended for load
/// balancers, uptime monitors, and smoke tests of the API surface itself
/// (the top-level `/health` route returns `OK` as `text/plain` for the same
/// purpose but is not part of the documented API).
#[utoipa::path(
    get,
    path = "/health",
    tag = "meta",
    responses(
        (status = 200, description = "Service is up", body = HealthResponse),
    ),
)]
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}
