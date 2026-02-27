-- Migration: This stores tracing events marked with `target: "monitoring"` for the
-- monitoring dashboard (query analytics, gauges, histograms).
--
-- Add monitoring events table + FTS index + triggers + search queries view

-- Step 1: Create events table
CREATE TABLE IF NOT EXISTS events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    level      TEXT    NOT NULL,            -- 'INFO', 'WARN', 'ERROR'
    message    TEXT    NOT NULL DEFAULT '',  -- e.g. 'Search request', 'Zero results'
    fields     TEXT    NOT NULL DEFAULT '{}' -- JSON blob of structured fields
);

-- Step 2: Add indexes for common query patterns
-- Time-range queries (histograms, recent events)
CREATE INDEX idx_events_timestamp ON events(timestamp);

-- Filtering by message (to quickly find "Search request" events via the view)
CREATE INDEX idx_events_message ON events(message);

-- Filtering by level (WARN/ERROR count queries)
CREATE INDEX idx_events_level ON events(level);

-- Step 3: Create FTS index for full-text search over events
CREATE VIRTUAL TABLE events_fts USING fts5(
    message,
    fields,
    content='events',
    content_rowid='id'
);

-- Step 4: Keep FTS in sync via triggers
CREATE TRIGGER events_ai AFTER INSERT ON events BEGIN
    INSERT INTO events_fts(rowid, message, fields)
    VALUES (new.id, new.message, new.fields);
END;

CREATE TRIGGER events_ad AFTER DELETE ON events BEGIN
    INSERT INTO events_fts(events_fts, rowid, message, fields)
    VALUES ('delete', old.id, old.message, old.fields);
END;

-- Step 5: Typed view for search-query analytics
-- Extracts structured fields from the generic events table via json_extract.
-- Only includes events with message = 'Search request'.
CREATE VIEW search_queries AS
SELECT
    id,
    timestamp,
    json_extract(fields, '$.query')        AS query,
    json_extract(fields, '$.results')      AS result_count,
    json_extract(fields, '$.duration_ms')  AS latency_ms,
    json_extract(fields, '$.content_type') AS content_type,
    json_extract(fields, '$.sort_by')      AS sort_by,
    json_extract(fields, '$.page')         AS page,
    json_extract(fields, '$.start_year')   AS start_year,
    json_extract(fields, '$.end_year')     AS end_year,
    json_extract(fields, '$.referer')      AS referer
FROM events
WHERE message = 'Search request';