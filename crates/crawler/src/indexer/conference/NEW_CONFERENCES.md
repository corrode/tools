# New Conferences Implementation Status

This document tracks the recently added conference modules that have been fully implemented with actual parsing logic, including historical backfilling where possible.

## Added Conferences

The following conferences have been added to the codebase and are actively indexing data:

### 1. Oxidize
- **Module**: `oxidize/`
- **Current Edition**: 2025 (Updated from 2024)
- **Website**: https://oxidizeconf.com/
- **Description**: Embedded Rust conference
- **Status**: **Implemented**
- **Details**:
  - Parses custom `.session` HTML components to extract titles and speakers.
  - Integrated the highlight reel YouTube playlist.
  - *Note*: Older editions (e.g., 2024) were backfilled using Wayback Machine snapshots.

### 2. RustFest
- **Module**: `rustfest/`
- **Current Editions**: 2024 (Zurich) & Historical (2016-2019)
- **Website**: https://rustfest.ch/ (2024), https://[city].rustfest.eu/ (Historical)
- **Description**: Community-driven Rust conference series
- **Status**: **Implemented**
- **Details**:
  - 2024 uses a specific HTML parser for the RustFest Zürich website.
  - Created a generic `RustFestEuParser` to backfill historical editions: Berlin 2016, Kyiv 2017, Zurich 2017, Paris 2018, Rome 2018, and Barcelona 2019.

### 3. RustNation
- **Module**: `rustnation/`
- **Current Edition**: 2026 (Updated from 2024)
- **Website**: https://www.rustnationuk.com/
- **Description**: UK-based Rust conference
- **Status**: **Implemented**
- **Details**:
  - Extracts the internal `__NEXT_DATA__` Next.js JSON payload instead of fragile HTML scraping.
  - Added the official YouTube playlist.
  - *Note*: Older years relied on Sessionize API endpoints that have been taken offline (404), but 2024 was successfully backfilled using Wayback Machine snapshots.

### 4. FOSDEM (Rust devroom)
- **Module**: `fosdem/`
- **Current Editions**: 2018-2025
- **Website**: https://fosdem.org/ (Current) and https://archive.fosdem.org/ (Historical)
- **Description**: Rust devroom track at FOSDEM
- **Status**: **Implemented**
- **Details**:
  - FOSDEM maintains a highly consistent HTML schedule structure.
  - Built a generic `FosdemParser` that parses standard bootstrap-like `table` rows.
  - Registered all 8 years of the Rust devroom track (2018 through 2025). Video links are naturally available in the HTML.

### 5. RustLab
- **Module**: `rustlab/`
- **Current Edition**: 2026 (Updated from 2024)
- **Website**: https://rustlab.it/
- **Description**: Italian Rust conference
- **Status**: **Implemented**
- **Details**:
  - Extracts the internal `__NEXT_DATA__` Next.js JSON payload.
  - Sourced their main YouTube channel.
  - *Note*: Currently safely logs a warning and returns an empty list for 2026 as the schedule data is not yet published by the organizers. Past editions (2023, 2024) were successfully backfilled using Wayback Machine snapshots.

## Implementation Approaches Used

1. **Next.js Payload Extraction**:
   - For React-heavy sites (RustNation, RustLab), scraping `__NEXT_DATA__` script tags and parsing them with `serde_json` proved far more reliable than DOM scraping.
2. **Generic Parsers & Macros**:
   - Built generalized struct parsers (`FosdemParser`, `RustFestEuParser`) paired with Rust macros (`define_fosdem_year!`, `define_rustfest_eu!`) to rapidly backfill historical years without duplicating code.
3. **Domain Redirects**:
   - Accounted for specific event routing, such as RustFest 2024 taking place at `rustfest.ch` rather than `rustfest.global`.
4. **Wayback Machine Integration**:
   - Resurrected lost endpoints (such as those replacing 404ing Sessionize APIs) using specific Wayback Machine snapshots to cleanly backfill data for Oxidize 2024, RustNation 2024, and RustLab 2023-2024.

## Registration

All conferences and their historical backfills are registered in `crates/crawler/src/indexer/conference/mod.rs`.
