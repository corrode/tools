use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Application error that wraps [`anyhow::Error`] and renders as a 500.
///
/// Lets handlers use `?` on any error while keeping client responses generic.
#[derive(Debug)]
pub(crate) struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("internal server error: {:#}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
