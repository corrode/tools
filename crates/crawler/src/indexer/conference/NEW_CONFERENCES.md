# New Conference Placeholders

This document tracks the newly added conference modules that need to be completed with actual parsing logic.

## Added Conferences

The following conferences have been added to the codebase with placeholder implementations:

### 1. Oxidize
- **Module**: `oxidize/`
- **Current Edition**: 2024
- **Website**: https://oxidizeconf.com/ (placeholder)
- **Description**: Embedded Rust conference
- **Status**: Placeholder - returns empty talk list
- **TODO**: 
  - Verify correct website URL and structure
  - Find YouTube playlist (if available)
  - Implement actual HTML parsing logic
  - Add additional editions if available

### 2. RustFest
- **Module**: `rustfest/`
- **Current Edition**: 2024
- **Website**: https://rustfest.global/ (placeholder)
- **Description**: Community-driven Rust conference series
- **Status**: Placeholder - returns empty talk list
- **TODO**: 
  - Verify correct website URL and structure
  - Find YouTube playlist (if available)
  - Implement actual HTML parsing logic
  - Add additional editions if available (RustFest has had many editions over the years)

### 3. RustNation
- **Module**: `rustnation/`
- **Current Edition**: 2024
- **Website**: https://www.rustnationuk.com/ (placeholder)
- **Description**: UK-based Rust conference
- **Status**: Placeholder - returns empty talk list
- **TODO**: 
  - Verify correct website URL and structure
  - Find YouTube playlist (if available)
  - Implement actual HTML parsing logic
  - Add additional editions if available

### 4. FOSDEM (Rust devroom)
- **Module**: `fosdem/`
- **Current Edition**: 2024
- **Website**: https://fosdem.org/2024/schedule/track/rust/ (placeholder)
- **Description**: Rust devroom track at FOSDEM
- **Status**: Placeholder - returns empty talk list
- **TODO**: 
  - Verify correct website URL and structure
  - Find YouTube playlist or video links (FOSDEM videos are usually on fosdem.org)
  - Implement actual HTML parsing logic
  - Add additional years (FOSDEM has had Rust devrooms in multiple years)

### 5. RustLab
- **Module**: `rustlab/`
- **Current Edition**: 2024
- **Website**: https://rustlab.it/ (placeholder)
- **Description**: Italian Rust conference
- **Status**: Placeholder - returns empty talk list
- **TODO**: 
  - Verify correct website URL and structure
  - Find YouTube playlist (if available)
  - Implement actual HTML parsing logic
  - Add additional editions if available

## Implementation Guide

To complete a conference parser:

1. **Research the conference**:
   - Verify the correct website URL
   - Check if there are multiple editions/years
   - Find YouTube playlists or video sources
   - Examine the HTML structure of the schedule page

2. **Update the parser file** (e.g., `oxidize2024.rs`):
   - Update the `BASE_URL` constant with the correct URL
   - Add `PLAYLIST_URL` if a YouTube playlist exists
   - Implement the `parse()` method to scrape the schedule
   - Parse talks, speakers, and metadata

3. **Follow existing patterns**:
   - See `eurorust/eurorust2024.rs` for a good example
   - Use the `ParsedTalk` struct to return talks
   - Use `NewTalk` and `NewSpeaker` structs
   - Handle errors gracefully

4. **Add multiple editions**:
   - Create new files for each year/edition
   - Export them in `mod.rs`
   - Register them in `../mod.rs` in the `get_all_parsers()` function

5. **Test**:
   - Run `cargo check --package crawler` to verify compilation
   - Run the crawler with `--debug` flag to test parsing
   - Verify talks are correctly extracted

## Registration

All conferences are registered in `/home/runner/work/search/search/crates/crawler/src/indexer/conference/mod.rs`:

```rust
pub fn get_all_parsers() -> Vec<Box<dyn ScheduleParser>> {
    vec![
        // ... existing conferences ...
        // FOSDEM Rust devroom editions
        Box::new(fosdem::FOSDEM2024),
        // Oxidize editions
        Box::new(oxidize::Oxidize2024),
        // RustFest editions
        Box::new(rustfest::RustFest2024),
        // RustLab editions
        Box::new(rustlab::RustLab2024),
        // RustNation editions
        Box::new(rustnation::RustNation2024),
    ]
}
```

## Notes

- All placeholder implementations currently return empty talk lists
- They log an info message when called
- The structure is in place and ready for actual implementation
- These parsers will be called when the conference indexer runs
- No data will be indexed until the parsing logic is implemented
