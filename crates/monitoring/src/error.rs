use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// A common error type that can be used throughout the application.
/// It wraps `anyhow::Error` and implements `IntoResponse`, so it can be returned
/// directly from Axum handlers.
pub struct AppError(anyhow::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Log the error including the full context trail provided by anyhow
        tracing::error!("Internal server error: {:#}", self.0);

        // We return a generic error message to the client to avoid leaking internals
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>`
// or any other error that can be converted into `anyhow::Error`, turning them
// into `Result<_, AppError>` automatically.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
