pub trait AppErrorExt<T> {
    fn into_internal_server_error(self) -> Result<T, axum::http::StatusCode>;
}
impl<T, E: std::fmt::Display> AppErrorExt<T> for Result<T, E> {
    fn into_internal_server_error(self) -> Result<T, axum::http::StatusCode> {
        self.map_err(|e| {
            tracing::error!("Internal server error: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })
    }
}
