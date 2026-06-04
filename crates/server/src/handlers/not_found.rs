use axum::{
    http::StatusCode,
    response::{Html, IntoResponse},
};

/// Fallback handler for unmatched routes.
pub(crate) async fn not_found() -> impl IntoResponse {
    let body = include_str!("../../templates/not_found.html");
    (StatusCode::NOT_FOUND, Html(body))
}
