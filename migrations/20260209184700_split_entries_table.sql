-- Migration: Split entries_meta into articles and videos tables
-- This migration creates separate tables for articles and videos,
-- migrates existing data, and updates FTS triggers.

-- Step 1: Create articles table
CREATE TABLE articles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    date TEXT NOT NULL,
    text TEXT,
    reference TEXT,
    word_count INTEGER
);

-- Step 2: Create videos table
CREATE TABLE videos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    date TEXT NOT NULL,
    text TEXT,
    thumbnail_url TEXT,
    duration_seconds INTEGER
);

-- Step 3: Migrate articles (non-YouTube URLs)
INSERT INTO articles (title, url, category, date, text, reference, word_count)
SELECT 
    title, 
    url, 
    category, 
    date, 
    text, 
    reference,
    CASE 
        WHEN text IS NOT NULL AND text != '' 
        THEN (LENGTH(text) - LENGTH(REPLACE(text, ' ', '')) + 1)
        ELSE NULL
    END as word_count
FROM entries_meta
WHERE url NOT LIKE '%youtube.com%' 
  AND url NOT LIKE '%youtu.be%';

-- Step 4: Migrate videos (YouTube URLs)
INSERT INTO videos (title, url, category, date, text, thumbnail_url, duration_seconds)
SELECT 
    title, 
    url, 
    category, 
    date, 
    text, 
    thumbnail_url, 
    duration_seconds
FROM entries_meta
WHERE url LIKE '%youtube.com%' 
   OR url LIKE '%youtu.be%';

-- Step 5: Create indexes for the new tables
CREATE INDEX idx_articles_date ON articles(date);
CREATE INDEX idx_articles_category ON articles(category);
CREATE INDEX idx_videos_date ON videos(date);
CREATE INDEX idx_videos_category ON videos(category);

-- Step 6: Drop old FTS triggers (they reference entries_meta)
DROP TRIGGER IF EXISTS entries_ai;
DROP TRIGGER IF EXISTS entries_ad;
DROP TRIGGER IF EXISTS entries_au;

-- Step 7: Drop and recreate FTS table to reference new structure
-- We need to rebuild it since it was tied to entries_meta
DROP TABLE IF EXISTS entries_fts;

CREATE VIRTUAL TABLE entries_fts USING fts5(
    title,
    category,
    text,
    content_type,
    tokenize='porter unicode61'
);

-- Step 8: Populate FTS with articles
INSERT INTO entries_fts(rowid, title, category, text, content_type)
SELECT id, title, category, text, 'article'
FROM articles;

-- Step 9: Populate FTS with videos (offset rowid to avoid collision)
-- We use a large offset (1000000000) to ensure video rowids don't collide with article rowids
INSERT INTO entries_fts(rowid, title, category, text, content_type)
SELECT id + 1000000000, title, category, text, 'video'
FROM videos;

-- Step 10: Create triggers to keep FTS in sync with articles
CREATE TRIGGER articles_ai AFTER INSERT ON articles BEGIN
    INSERT INTO entries_fts(rowid, title, category, text, content_type)
    VALUES (new.id, new.title, new.category, new.text, 'article');
END;

CREATE TRIGGER articles_ad AFTER DELETE ON articles BEGIN
    INSERT INTO entries_fts(entries_fts, rowid, title, category, text, content_type)
    VALUES('delete', old.id, old.title, old.category, old.text, 'article');
END;

CREATE TRIGGER articles_au AFTER UPDATE ON articles BEGIN
    INSERT INTO entries_fts(entries_fts, rowid, title, category, text, content_type)
    VALUES('delete', old.id, old.title, old.category, old.text, 'article');
    INSERT INTO entries_fts(rowid, title, category, text, content_type)
    VALUES (new.id, new.title, new.category, new.text, 'article');
END;

-- Step 11: Create triggers to keep FTS in sync with videos
CREATE TRIGGER videos_ai AFTER INSERT ON videos BEGIN
    INSERT INTO entries_fts(rowid, title, category, text, content_type)
    VALUES (new.id + 1000000000, new.title, new.category, new.text, 'video');
END;

CREATE TRIGGER videos_ad AFTER DELETE ON videos BEGIN
    INSERT INTO entries_fts(entries_fts, rowid, title, category, text, content_type)
    VALUES('delete', old.id + 1000000000, old.title, old.category, old.text, 'video');
END;

CREATE TRIGGER videos_au AFTER UPDATE ON videos BEGIN
    INSERT INTO entries_fts(entries_fts, rowid, title, category, text, content_type)
    VALUES('delete', old.id + 1000000000, old.title, old.category, old.text, 'video');
    INSERT INTO entries_fts(rowid, title, category, text, content_type)
    VALUES (new.id + 1000000000, new.title, new.category, new.text, 'video');
END;

-- Step 12: Drop the old entries_meta table
-- Keeping it commented out for safety - uncomment after verifying migration
-- DROP TABLE IF EXISTS entries_meta;

-- Step 13: Create a view for backward compatibility (optional, can be removed later)
CREATE VIEW entries_meta_compat AS
SELECT 
    id,
    title,
    url,
    category,
    date,
    text,
    'article' as entry_type,
    NULL as thumbnail_url,
    reference,
    NULL as duration_seconds
FROM articles
UNION ALL
SELECT 
    id + 1000000000 as id,
    title,
    url,
    category,
    date,
    text,
    'video' as entry_type,
    thumbnail_url,
    NULL as reference,
    duration_seconds
FROM videos;