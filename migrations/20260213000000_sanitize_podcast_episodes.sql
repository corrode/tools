-- Migration: Sanitize podcast episodes schema and add guest tables
-- This migration enforces required fields, removes deprecated columns,
-- rebuilds FTS, and introduces normalized guest tables.



-- Phase 1: Quarantine invalid rows (keep them for inspection)
CREATE TABLE IF NOT EXISTS podcast_episodes_quarantine AS
SELECT * FROM podcast_episodes WHERE 0;

INSERT INTO podcast_episodes_quarantine
SELECT * FROM podcast_episodes
WHERE podcast_name IS NULL OR podcast_name = ''
   OR episode_name IS NULL OR episode_name = ''
   OR summary IS NULL OR summary = ''
   OR transcript IS NULL OR transcript = '';

-- Phase 2: Create new sanitized table
CREATE TABLE podcast_episodes_new (
    id INTEGER PRIMARY KEY,
    podcast_name TEXT NOT NULL CHECK (length(podcast_name) > 0),
    episode_name TEXT NOT NULL CHECK (length(episode_name) > 0),
    date TEXT NOT NULL,
    duration_seconds INTEGER,
    summary TEXT NOT NULL CHECK (length(summary) > 0),
    url TEXT NOT NULL UNIQUE CHECK (length(url) > 0),
    thumbnail_url TEXT,
    transcript TEXT NOT NULL CHECK (length(transcript) > 0)
);

-- Phase 3: Migrate data
INSERT INTO podcast_episodes_new (
    id,
    podcast_name,
    episode_name,
    date,
    duration_seconds,
    summary,
    url,
    thumbnail_url,
    transcript
)
SELECT
    id,
    podcast_name,
    episode_name,
    date,
    duration_seconds,
    summary,
    url,
    thumbnail_url,
    transcript
FROM podcast_episodes
WHERE podcast_name IS NOT NULL AND podcast_name <> ''
  AND episode_name IS NOT NULL AND episode_name <> ''
  AND summary IS NOT NULL AND summary <> ''
  AND transcript IS NOT NULL AND transcript <> '';

-- Phase 4: Swap tables
DROP TRIGGER IF EXISTS podcast_episodes_ai;
DROP TRIGGER IF EXISTS podcast_episodes_ad;
DROP TRIGGER IF EXISTS podcast_episodes_au;

DROP TABLE IF EXISTS podcast_episodes_fts;

DROP INDEX IF EXISTS idx_podcast_episodes_date;
DROP INDEX IF EXISTS idx_podcast_episodes_category;

DROP TABLE podcast_episodes;
ALTER TABLE podcast_episodes_new RENAME TO podcast_episodes;

-- Recreate indexes
CREATE INDEX idx_podcast_episodes_date ON podcast_episodes(date);

-- Phase 5: Rebuild FTS and triggers
CREATE VIRTUAL TABLE podcast_episodes_fts USING fts5(
    podcast_name,
    episode_name,
    summary,
    transcript,
    tokenize='porter unicode61'
);

INSERT INTO podcast_episodes_fts(rowid, podcast_name, episode_name, summary, transcript)
SELECT id, podcast_name, episode_name, summary, transcript
FROM podcast_episodes;

CREATE TRIGGER podcast_episodes_ai AFTER INSERT ON podcast_episodes BEGIN
    INSERT INTO podcast_episodes_fts(rowid, podcast_name, episode_name, summary, transcript)
    VALUES (new.id, new.podcast_name, new.episode_name, new.summary, new.transcript);
END;

CREATE TRIGGER podcast_episodes_ad AFTER DELETE ON podcast_episodes BEGIN
    DELETE FROM podcast_episodes_fts WHERE rowid = old.id;
END;

CREATE TRIGGER podcast_episodes_au AFTER UPDATE ON podcast_episodes BEGIN
    DELETE FROM podcast_episodes_fts WHERE rowid = old.id;
    INSERT INTO podcast_episodes_fts(rowid, podcast_name, episode_name, summary, transcript)
    VALUES (new.id, new.podcast_name, new.episode_name, new.summary, new.transcript);
END;

INSERT INTO podcast_episodes_fts(podcast_episodes_fts) VALUES ('optimize');

-- Phase 6: Create guest tables
CREATE TABLE IF NOT EXISTS podcast_guests (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS podcast_episode_guests (
    episode_id INTEGER NOT NULL,
    guest_id INTEGER NOT NULL,
    PRIMARY KEY (episode_id, guest_id),
    FOREIGN KEY (episode_id) REFERENCES podcast_episodes(id) ON DELETE CASCADE,
    FOREIGN KEY (guest_id) REFERENCES podcast_guests(id) ON DELETE CASCADE
);

