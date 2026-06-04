use std::sync::Arc;

use askama::Template;
use axum::{extract::State, response::Html};
use types::Catalog;

use crate::error::AppError;
use crate::view::IndexView;

/// The single dense reference page.
#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    view: IndexView,
}

/// Renders the full, grouped tool index.
pub(crate) async fn index(State(catalog): State<Arc<Catalog>>) -> Result<Html<String>, AppError> {
    let template = IndexTemplate {
        view: IndexView::build(&catalog),
    };
    Ok(Html(template.render()?))
}
