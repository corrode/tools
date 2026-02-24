-- Migration: Add talks tables for conference presentation indexing
-- This adds storage for conference talks with speakers and FTS index.

-- Step 1: Create main talks table
CREATE TABLE IF NOT EXISTS talks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL CHECK (length(title) > 0),
    summary TEXT NOT NULL CHECK (length(summary) > 0),
    transcript TEXT,
    conference TEXT NOT NULL CHECK (length(conference) > 0),
    date TEXT NOT NULL,
    website_url TEXT NOT NULL UNIQUE CHECK (length(website_url) > 0),
    video_url TEXT,
    slides_url TEXT,
    duration_seconds INTEGER
);

-- Step 2: Create speakers table (normalized)
CREATE TABLE IF NOT EXISTS speakers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    website_url TEXT,
    twitter_handle TEXT,
    github_handle TEXT,
    mastodon_handle TEXT,
    bluesky_handle TEXT
);

-- Step 3: Create junction table for talk-speaker relationship (many-to-many)
CREATE TABLE IF NOT EXISTS talk_speakers (
    talk_id INTEGER NOT NULL,
    speaker_id INTEGER NOT NULL,
    PRIMARY KEY (talk_id, speaker_id),
    FOREIGN KEY (talk_id) REFERENCES talks(id) ON DELETE CASCADE,
    FOREIGN KEY (speaker_id) REFERENCES speakers(id) ON DELETE CASCADE
);

-- Step 4: Create indexes for common queries
CREATE INDEX IF NOT EXISTS idx_talks_date ON talks(date);
CREATE INDEX IF NOT EXISTS idx_talks_conference ON talks(conference);
CREATE INDEX IF NOT EXISTS idx_speakers_name ON speakers(name);

-- Step 5: Create FTS table for full-text search on talks
CREATE VIRTUAL TABLE IF NOT EXISTS talks_fts USING fts5(
    title,
    summary,
    transcript,
    conference,
    tokenize='porter unicode61'
);

-- Step 6: Populate FTS from existing rows (if any)
INSERT INTO talks_fts(rowid, title, summary, transcript, conference)
SELECT id, title, summary, transcript, conference
FROM talks;

-- Step 7: Create triggers to keep FTS in sync

-- Insert trigger
CREATE TRIGGER IF NOT EXISTS talks_ai AFTER INSERT ON talks BEGIN
    INSERT INTO talks_fts(rowid, title, summary, transcript, conference)
    VALUES (new.id, new.title, new.summary, new.transcript, new.conference);
END;

-- Delete trigger
CREATE TRIGGER IF NOT EXISTS talks_ad AFTER DELETE ON talks BEGIN
    DELETE FROM talks_fts WHERE rowid = old.id;
END;

-- Update trigger
CREATE TRIGGER IF NOT EXISTS talks_au AFTER UPDATE ON talks BEGIN
    DELETE FROM talks_fts WHERE rowid = old.id;
    INSERT INTO talks_fts(rowid, title, summary, transcript, conference)
    VALUES (new.id, new.title, new.summary, new.transcript, new.conference);
END;

-- Step 8: Optimize FTS for faster queries after bulk inserts
INSERT INTO talks_fts(talks_fts) VALUES ('optimize');