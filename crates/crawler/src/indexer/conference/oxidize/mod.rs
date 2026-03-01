//! Oxidize conference schedule parsers.
//!
//! Oxidize is an embedded Rust conference.
//! Each edition has its own parser implementation.

mod oxidize2025;

pub use oxidize2025::Oxidize2025;
mod oxidize2024;
pub use oxidize2024::Oxidize2024;
