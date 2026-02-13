-- Migration: Add podcast episodes table + FTS index + triggers
-- This adds storage for podcast episodes and keeps an FTS index in sync.

-- Step 1: Create podcast_episodes table
CREATE TABLE IF NOT EXISTS podcast_episodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    date TEXT NOT NULL,
    summary TEXT,
    thumbnail_url TEXT,
    duration_seconds INTEGER,
    transcript TEXT
);

-- Step 2: Create indexes for common queries
CREATE INDEX IF NOT EXISTS idx_podcast_episodes_date ON podcast_episodes(date);
CREATE INDEX IF NOT EXISTS idx_podcast_episodes_category ON podcast_episodes(category);

-- Step 3: Create FTS table for podcasts
CREATE VIRTUAL TABLE IF NOT EXISTS podcast_episodes_fts USING fts5(
    title,
    category,
    summary,
    transcript,
    tokenize='porter unicode61'
);

-- Step 4: Populate FTS from existing rows (if any)
INSERT INTO podcast_episodes_fts(rowid, title, category, summary, transcript)
SELECT id, title, category, summary, transcript
FROM podcast_episodes;

-- Step 5: Create triggers to keep FTS in sync
CREATE TRIGGER IF NOT EXISTS podcast_episodes_ai AFTER INSERT ON podcast_episodes BEGIN
    INSERT INTO podcast_episodes_fts(rowid, title, category, summary, transcript)
    VALUES (new.id, new.title, new.category, new.summary, new.transcript);
END;

CREATE TRIGGER IF NOT EXISTS podcast_episodes_ad AFTER DELETE ON podcast_episodes BEGIN
    INSERT INTO podcast_episodes_fts(podcast_episodes_fts, rowid, title, category, summary, transcript)
    VALUES ('delete', old.id, old.title, old.category, old.summary, old.transcript);
END;

CREATE TRIGGER IF NOT EXISTS podcast_episodes_au AFTER UPDATE ON podcast_episodes BEGIN
    INSERT INTO podcast_episodes_fts(podcast_episodes_fts, rowid, title, category, summary, transcript)
    VALUES ('delete', old.id, old.title, old.category, old.summary, old.transcript);
    INSERT INTO podcast_episodes_fts(rowid, title, category, summary, transcript)
    VALUES (new.id, new.title, new.category, new.summary, new.transcript);
END;

-- Step 6: Optimize FTS for faster queries after bulk inserts
INSERT INTO podcast_episodes_fts(podcast_episodes_fts) VALUES ('optimize');