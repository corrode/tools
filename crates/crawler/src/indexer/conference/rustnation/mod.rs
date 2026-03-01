//! RustNation schedule parsers.
//!
//! RustNation is a UK-based Rust conference.
//! Each edition has its own parser implementation.

mod rustnation2026;

pub use rustnation2026::RustNation2026;
mod rustnation2024;
pub use rustnation2024::RustNation2024;
