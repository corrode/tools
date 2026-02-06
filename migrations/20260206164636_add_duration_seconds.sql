-- Add duration_seconds column for video duration storage
-- For videos: actual duration in seconds from YouTube API
-- For articles: NULL (reading time is calculated from word count)
ALTER TABLE entries_meta ADD COLUMN duration_seconds INTEGER;