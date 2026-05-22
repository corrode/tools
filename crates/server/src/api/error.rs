//! Error type for the JSON API.
//!
//! Unlike [`crate::error::AppError`], which returns `text/html`, [`ApiError`]
//! always returns a structured JSON body so machine clients can parse the
//! failure reason.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

use types::params::ParamsError;

/// Structured error response returned by every non-2xx response from the
/// public JSON API.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ApiError {
    /// Stable machine-readable error code.
    ///
    /// Clients should branch on this field, **not** on `message`, which is
    /// allowed to change between releases.
    #[schema(example = "invalid_params")]
    pub code: &'static str,

    /// Human-readable English explanation of the failure.
    #[schema(example = "start year 1850 is out of range 1900–2050")]
    pub message: String,

    /// HTTP status code, omitted from `IntoResponse` mapping (it's redundant
    /// with the response status line but useful when the body is logged
    /// independently).
    #[serde(skip)]
    #[schema(value_type = u16)]
    pub status: StatusCode,
}

impl ApiError {
    pub(crate) fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_params",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: "not_found",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
        }
    }

    pub(crate) fn internal() -> Self {
        Self {
            code: "internal",
            message: "Internal server error".to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        (status, Json(self)).into_response()
    }
}

impl From<ParamsError> for ApiError {
    fn from(err: ParamsError) -> Self {
        Self::invalid_params(err.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        tracing::error!("API internal error: {err:#}");
        Self::internal()
    }
}
