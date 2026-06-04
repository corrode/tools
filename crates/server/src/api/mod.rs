//! Public, read-only JSON API for the Rust Tool Index.
//!
//! The `OpenAPI` 3.1 spec is built from the route registrations and the
//! `ToSchema` derives on the shared [`types`] structs, and served at:
//!
//! - `GET /api/v1/openapi.json` — the raw spec.
//! - `GET /api/v1/docs` — Swagger UI.

use std::sync::Arc;

use axum::{Router, http::Method};
use tower_http::cors::{Any, CorsLayer};
use types::Catalog;
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa_swagger_ui::SwaggerUi;

mod tools;

/// Top-level `OpenAPI` document.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Rust Tool Index API",
        version = "1.0.0",
        description = "A curated, machine-readable reference of Rust development tooling. \
                       Editorial fields are human-authored; metrics are auto-refreshed daily \
                       from the source forge and crates.io. Read-only and public.",
        license(name = "MIT"),
        contact(name = "corrode", url = "https://corrode.dev"),
    ),
    servers((url = "/api/v1", description = "Current host")),
    tags((name = "tools", description = "The curated tool catalog.")),
)]
struct ApiDoc;

/// Builds the complete `/api/v1/*` router plus Swagger UI and raw spec.
pub(crate) fn build(state: Arc<Catalog>) -> Router {
    let (router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(tools::list_tools))
        .routes(routes!(tools::get_tool))
        .with_state(state)
        .split_for_parts();

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::OPTIONS])
        .allow_headers(Any);

    Router::new()
        .nest("/api/v1", router)
        .merge(SwaggerUi::new("/api/v1/docs").url("/api/v1/openapi.json", openapi))
        .layer(cors)
}
