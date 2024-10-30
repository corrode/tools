-- Create the FTS5 virtual table with full-text search capabilities
CREATE VIRTUAL TABLE entries USING fts5(
    title,           -- Title of the article
    url UNINDEXED,   -- URL is not searchable but stored
    category,        -- Category for grouping
    date UNINDEXED,  -- Date stored but not searchable
    text,            -- Main content for full-text search
    -- Configure FTS5 options
    tokenize="porter unicode61",  -- Use porter stemming with Unicode support
    content='',                   -- Contentless table for efficiency
    columnsize=0                  -- Save space by not storing column sizes
);

-- Create regular table for metadata and additional indexes
CREATE TABLE entries_meta (
    url TEXT PRIMARY KEY,
    date TEXT NOT NULL,
    category TEXT NOT NULL
);

-- Create index on date for efficient date-based queries
CREATE INDEX entries_meta_date_idx ON entries_meta(date);