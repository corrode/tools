//! RustFest schedule parsers.
//! RustFest conference schedule parsers.
//!
//! RustFest is a community-driven Rust conference series.
//! Each year/edition has its own parser implementation.

mod rustfest2024;
#[allow(missing_docs)]
mod rustfest_eu;

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser};
use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDate;

pub use rustfest_eu::RustFestEuParser;
pub use rustfest2024::RustFest2024;

macro_rules! define_rustfest_eu {
    ($struct_name:ident, $year:expr, $city:expr, $month:expr, $day:expr) => {
        #[doc = concat!("Parser for RustFest ", stringify!($year), " (", $city, ")")]
        pub struct $struct_name;

        #[async_trait]
        impl ScheduleParser for $struct_name {
            fn metadata(&self) -> ConferenceMetadata {
                RustFestEuParser::new(
                    $year,
                    $city,
                    NaiveDate::from_ymd_opt($year, $month, $day).unwrap(),
                )
                .metadata()
            }

            async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
                RustFestEuParser::new(
                    $year,
                    $city,
                    NaiveDate::from_ymd_opt($year, $month, $day).unwrap(),
                )
                .parse(client)
                .await
            }
        }
    };
}

define_rustfest_eu!(RustFest2016, 2016, "Berlin", 9, 17);
define_rustfest_eu!(RustFest2017Kyiv, 2017, "Kyiv", 4, 29);
define_rustfest_eu!(RustFest2017Zurich, 2017, "Zurich", 9, 30);
define_rustfest_eu!(RustFest2018Paris, 2018, "Paris", 5, 26);
define_rustfest_eu!(RustFest2018Rome, 2018, "Rome", 11, 24);
define_rustfest_eu!(RustFest2019, 2019, "Barcelona", 11, 9);
