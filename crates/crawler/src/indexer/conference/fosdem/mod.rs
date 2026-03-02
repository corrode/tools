//! FOSDEM Rust devroom schedule parsers.
//!
//! FOSDEM is a large open-source conference with a Rust devroom track.
//! Each year has its own parser implementation.

#[allow(missing_docs)]
mod fosdem_parser;

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser};
use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDate;

pub use fosdem_parser::FosdemParser;

macro_rules! define_fosdem_year {
    ($struct_name:ident, $year:expr, $month:expr, $day:expr) => {
        #[doc = concat!("Parser for FOSDEM ", stringify!($year))]
        pub struct $struct_name;

        #[async_trait]
        impl ScheduleParser for $struct_name {
            fn metadata(&self) -> ConferenceMetadata {
                FosdemParser::new($year, NaiveDate::from_ymd_opt($year, $month, $day).unwrap())
                    .metadata()
            }

            async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
                FosdemParser::new($year, NaiveDate::from_ymd_opt($year, $month, $day).unwrap())
                    .parse(client)
                    .await
            }
        }
    };
}

define_fosdem_year!(FOSDEM2018, 2018, 2, 3);
define_fosdem_year!(FOSDEM2019, 2019, 2, 2);
define_fosdem_year!(FOSDEM2020, 2020, 2, 1);
define_fosdem_year!(FOSDEM2023, 2023, 2, 4);
define_fosdem_year!(FOSDEM2024, 2024, 2, 3);
define_fosdem_year!(FOSDEM2025, 2025, 2, 1);
