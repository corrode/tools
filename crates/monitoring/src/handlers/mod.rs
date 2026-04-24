//! Monitoring dashboard handlers and authentication.
//!
//! All routes under `/monitoring` are gated by [`require_monitoring_token`].

pub mod auth;
pub mod dashboard;
pub mod queries;

pub use auth::{login, require_monitoring_token};
pub use dashboard::dashboard;
pub use queries::queries;
