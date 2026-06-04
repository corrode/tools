//! `/api/v1/tools` endpoints.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use types::{Catalog, Tool};

/// List every tool in the index.
///
/// Returns the full catalog — editorial fields plus the latest auto-refreshed
/// metrics — in load order (sorted by id). This is the bulk feed for external
/// consumers and LLM clients.
#[utoipa::path(
    get,
    path = "/tools",
    tag = "tools",
    responses((status = 200, description = "The full tool catalog", body = [Tool])),
)]
pub(crate) async fn list_tools(State(catalog): State<Arc<Catalog>>) -> Json<Vec<Tool>> {
    Json(catalog.tools().to_vec())
}

/// Fetch a single tool by id.
#[utoipa::path(
    get,
    path = "/tools/{id}",
    tag = "tools",
    params(("id" = String, Path, description = "The tool's stable slug, e.g. `cargo-nextest`")),
    responses(
        (status = 200, description = "The requested tool", body = Tool),
        (status = 404, description = "No tool with that id"),
    ),
)]
pub(crate) async fn get_tool(
    State(catalog): State<Arc<Catalog>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match catalog.get(&id) {
        Some(tool) => Json(tool.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
