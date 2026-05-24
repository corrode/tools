use askama::Template;
use axum::{http::StatusCode, response::Html};

use crate::error::AppError;

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundTemplate;

/// Branded 404 page. Served by axum's `fallback` for any unmatched route.
pub(crate) async fn not_found() -> Result<(StatusCode, Html<String>), AppError> {
    let body = NotFoundTemplate.render()?;
    Ok((StatusCode::NOT_FOUND, Html(body)))
}
