# Research Paper Indexing - Implementation Guide

## Overview

This implementation adds support for searching Rust research papers from arXiv. Papers are indexed with full-text search capabilities and displayed in a dedicated "Research" tab.

## Setup

### 1. Run Database Migrations

The migration `20260224144300_add_research_papers.sql` will create the necessary tables and indexes:

```bash
# Migrations are automatically applied when the application starts
# Or you can run them manually with sqlx-cli if needed
```

### 2. Index Research Papers

Run the research paper indexer to fetch papers from arXiv:

```bash
RUST_LOG=crawler=info cargo run -p crawler -- --indexer research
```

Options:
- `--dry-run`: Preview what would be indexed without actually indexing
- `--overwrite`: Re-index existing papers
- `--debug`: Enable debug logging

### 3. Start the Server

```bash
cargo run -p server
```

The server will be available at `http://localhost:3000`.

## Usage

### Searching Research Papers

1. Go to the search page
2. Enter your query (e.g., "memory safety", "async rust", "type system")
3. Click the "Research" tab to filter results to only research papers
4. Papers will be displayed with:
   - Title (linked to arXiv)
   - Authors
   - Publication venue (e.g., "arXiv")
   - Publication date
   - ArXiv ID
   - Abstract
   - Search snippet (highlighted matching text)

### Search Queries Used

The indexer searches arXiv for papers matching these queries:
- `rust+AND+programming` - General Rust programming papers
- `rust+AND+language` - Rust language design and features
- `rust+AND+systems` - Systems programming with Rust
- `rust+AND+memory+AND+safety` - Memory safety research

Papers are fetched in batches of 100 per query, sorted by submission date.

## Architecture

### Database Schema

```sql
CREATE TABLE research_papers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    date TEXT NOT NULL,
    authors TEXT NOT NULL,
    abstract_text TEXT NOT NULL,
    text TEXT,
    paper_id TEXT,
    publication TEXT
);

CREATE VIRTUAL TABLE research_papers_fts USING fts5(
    title,
    category,
    authors,
    abstract_text,
    text,
    tokenize='porter unicode61'
);
```

### Data Flow

1. **Indexer** (`crates/crawler/src/indexer/research.rs`)
   - Queries arXiv API
   - Parses Atom XML feeds
   - Extracts metadata (title, authors, abstract, etc.)
   - Inserts into database via Repository

2. **Repository** (`crates/storage/src/lib.rs`)
   - `insert_research_paper()` - Stores papers with upsert logic
   - `search_research_papers()` - Full-text search with filters
   - `search_all()` - Includes papers in combined results

3. **Server** (`crates/server/src/handlers/search.rs`)
   - Converts database results to view models
   - Renders research template

4. **Templates**
   - `results.html` - Research tab button
   - `result/research.html` - Paper display template

## API Details

### arXiv API

The indexer uses the arXiv API v1:
- **Endpoint**: `http://export.arxiv.org/api/query`
- **Format**: Atom XML
- **Rate Limiting**: 3-second delay between queries
- **Fields**: id, title, summary, published, author[], category[]

Example query:
```
http://export.arxiv.org/api/query?search_query=all:rust+AND+all:programming&start=0&max_results=100&sortBy=submittedDate&sortOrder=descending
```

### Paper ID Format

Papers are identified by their arXiv ID, extracted from the URL:
- URL: `http://arxiv.org/abs/2301.00000v1`
- Stored as: `arXiv:2301.00000`

## Extending the Feature

### Adding More Sources

To add additional research paper sources:

1. Create a new indexer module (e.g., `acm.rs`, `ieee.rs`)
2. Implement the `Indexer` trait
3. Add to `indexer/mod.rs`
4. Register in `main.rs`

### Custom Search Queries

Modify `SEARCH_QUERIES` in `crates/crawler/src/indexer/research.rs`:

```rust
const SEARCH_QUERIES: &[&str] = &[
    "all:rust+AND+all:concurrent",
    "all:rust+AND+all:verification",
    // Add more queries...
];
```

### Filtering Results

Add additional filters to the search UI by:
1. Extending `RawParams` in `crates/types/src/params.rs`
2. Updating search query building in `crates/storage/src/lib.rs`
3. Adding UI controls in templates

## Troubleshooting

### No Papers Found

- Check internet connectivity
- Verify arXiv API is accessible
- Look for rate limiting errors in logs
- Try with `--dry-run` first to test API access

### Duplicate Papers

- Use `--overwrite` flag to re-index
- Check for URL uniqueness constraints
- Verify paper_id extraction logic

### Search Not Working

- Ensure migrations have run
- Check FTS triggers are created
- Verify text content is being indexed
- Test with simple queries first

## Performance Notes

- Initial indexing may take several minutes (4 queries × 100 papers × 3-second delay)
- FTS5 provides fast full-text search even with thousands of papers
- Search results are paginated (20 per page by default)
- Top-N optimization ensures fast queries even with large datasets

## Future Enhancements

Potential improvements:
- [ ] Add more research databases (ACM, IEEE, Google Scholar)
- [ ] Implement citation tracking
- [ ] Add PDF download/caching
- [ ] Enable advanced search filters (year range, specific authors)
- [ ] Add related papers suggestions
- [ ] Implement paper recommendations based on search history
- [ ] Add export to BibTeX format
