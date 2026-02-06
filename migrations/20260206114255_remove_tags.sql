-- Remove tags system (tags table and entry_tags junction table)
-- The reference field (e.g., "TWiR #541", "RFC #123") provides sufficient source attribution

-- Drop the junction table first (has foreign key references)
DROP TABLE IF EXISTS entry_tags;

-- Drop the tags table
DROP TABLE IF EXISTS tags;

-- Note: The 'tags' column in entries_meta should be removed manually if it exists:
-- ALTER TABLE entries_meta DROP COLUMN tags;
-- This requires SQLite 3.35.0+ (2021-03-12) and cannot be made idempotent in pure SQL.