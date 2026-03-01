//! Monitoring dashboard handlers and authentication.
//!
//! All routes under `/monitoring` are gated by [`require_monitoring_token`].

pub(crate) mod auth;
pub(crate) mod dashboard;
pub(crate) mod queries;

pub(crate) use auth::{login, require_monitoring_token};
pub(crate) use dashboard::dashboard;
pub(crate) use queries::queries;
