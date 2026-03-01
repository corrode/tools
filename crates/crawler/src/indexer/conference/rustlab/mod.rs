//! RustLab schedule parsers.
//!
//! RustLab is an Italian Rust conference.
//! Each edition has its own parser implementation.

mod rustlab2026;

pub use rustlab2026::RustLab2026;
mod rustlab2024;
pub use rustlab2024::RustLab2024;
mod rustlab2023;
pub use rustlab2023::RustLab2023;
