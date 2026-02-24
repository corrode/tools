-- Migration: add thumbnail_url to talks
-- Stores optional thumbnail URL for conference talks.
ALTER TABLE talks
ADD COLUMN thumbnail_url TEXT;