//! RustConf schedule parsers.
//!
//! Each RustConf edition has its own parser implementation.

mod rustconf2016;
mod rustconf2017;
mod rustconf2018;
mod rustconf2019;
mod rustconf2020;
mod rustconf2021;
mod rustconf2022;
mod rustconf2023;
mod rustconf2024;

pub use rustconf2016::RustConf2016;
pub use rustconf2017::RustConf2017;
pub use rustconf2018::RustConf2018;
pub use rustconf2019::RustConf2019;
pub use rustconf2020::RustConf2020;
pub use rustconf2021::RustConf2021;
pub use rustconf2022::RustConf2022;
pub use rustconf2023::RustConf2023;
pub use rustconf2024::RustConf2024;

/// Slugify a string for use in URLs.
///
/// Converts to lowercase, replaces spaces with dashes, and removes punctuation.
pub fn slugify(input: &str) -> String {
    input
        .to_lowercase()
        .replace(' ', "-")
        .replace(['\'', '?', '!', ':', '`', '"', '(', ')', ',', '.', ';'], "")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        "Making Rust Fast, Safe, and Productive!",
        "making-rust-fast-safe-and-productive"
    )]
    #[case(
        "RFC: In Order to Form a More Perfect union",
        "rfc-in-order-to-form-a-more-perfect-union"
    )]
    #[case(
        "RFC: In Order to Form a More Perfect union",
        "rfc-in-order-to-form-a-more-perfect-union"
    )]
    #[case(
        "Class fixes; or, you become the Rust compiler",
        "class-fixes-or-you-become-the-rust-compiler"
    )]
    fn test_slugify(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(slugify(input), expected);
    }
}
