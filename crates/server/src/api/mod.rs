//! Public JSON API for the search index.
//!
//! Mounted by the caller via [`Router::merge`]; all routes carry their full
//! `/api/v1/...` path internally so that Swagger UI's `index.html` redirect
//! resolves correctly. See `crates/server/src/api/README.md` for the design
//! rationale.
//!
//! The OpenAPI 3.1 specification is built at startup from the route
//! registrations and the `ToSchema` / `IntoParams` derives on the DTOs and
//! query types, and served at:
//!
//! - `GET /api/v1/openapi.json` — the raw spec.
//! - `GET /api/v1/docs` — Swagger UI rendered docs page.

use std::sync::Arc;

use axum::{Router, http::Method};
use storage::Repository;
use tower_http::cors::{Any, CorsLayer};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa_swagger_ui::SwaggerUi;

mod dto;
mod error;
mod health;
mod podcast;
mod search;
mod stats;
mod suggestions;

/// Top-level OpenAPI document. The description is rendered by Swagger UI on
/// the docs landing page and serves as the public reference for API
/// consumers.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Corrode Rust Search API",
        version = "1.0.0",
        description = include_str!("description.md"),
        license(
            name = "MIT",
            url = "https://github.com/corrode/search/blob/main/LICENSE",
        ),
        contact(
            name = "corrode",
            url = "https://corrode.dev",
        ),
    ),
    servers(
        (url = "/api/v1", description = "Current host"),
    ),
    tags(
        (name = "search",      description = "Full-text search across the corpus."),
        (name = "suggestions", description = "Query autocomplete."),
        (name = "stats",       description = "Aggregate index statistics."),
        (name = "podcasts",    description = "Podcast episode detail with transcript."),
        (name = "meta",        description = "Service metadata (health, etc.)."),
    ),
)]
struct ApiDoc;

/// Builds the complete `/api/v1/*` router (JSON endpoints + Swagger UI +
/// raw OpenAPI spec). The returned router is meant to be `merge`d into the
/// top-level app router, not nested: Swagger UI's internal redirect target
/// is absolute, so the path it's registered under must match the path it's
/// served from.
pub(crate) fn build(state: Arc<Repository>) -> Router {
    let (router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health::health))
        .routes(routes!(search::search))
        .routes(routes!(suggestions::suggestions))
        .routes(routes!(stats::stats))
        .routes(routes!(podcast::get_podcast))
        .with_state(state)
        .split_for_parts();

    // Permissive CORS — the API is read-only and meant to be consumed from
    // arbitrary origins.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET])
        .allow_headers(Any);

    Router::new()
        .nest("/api/v1", router)
        .merge(SwaggerUi::new("/api/v1/docs").url("/api/v1/openapi.json", openapi))
        .layer(cors)
}
