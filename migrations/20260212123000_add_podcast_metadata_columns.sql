-- Migration: Add podcast metadata columns to podcast_episodes
-- Adds podcast/show and author metadata fields to existing table.

ALTER TABLE podcast_episodes
    ADD COLUMN podcast_name TEXT;

ALTER TABLE podcast_episodes
    ADD COLUMN episode_name TEXT;

ALTER TABLE podcast_episodes
    ADD COLUMN podcast_author TEXT;

ALTER TABLE podcast_episodes
    ADD COLUMN episode_author TEXT;