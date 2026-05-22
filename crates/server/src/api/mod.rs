//! Public JSON API for the search index.
//!
//! Mounted under `/api/v1`. See [`docs/PUBLIC_API.md`] for the design
//! rationale. All routes here return JSON; the HTML/HTMX handlers live in
//! [`crate::handlers`] and are mounted at the top-level paths used by the
//! browser.
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

/// Builds the API sub-router and the OpenAPI document, returning them both
/// so the caller can `nest` the router and serve the spec from a fixed path.
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
        .merge(router)
        .merge(SwaggerUi::new("/docs").url("/openapi.json", openapi))
        .layer(cors)
}
