-- Migration: Separate FTS tables for articles and videos
--
-- The previous migration used a single FTS table with a rowid offset (1 billion)
-- to avoid collisions between article and video IDs. This was complex and
-- error-prone, requiring arithmetic in every join.
--
-- This migration creates separate FTS tables for each content type:
--   - articles_fts: Full-text search index for articles
--   - videos_fts: Full-text search index for videos
--
-- Benefits:
--   - No rowid collision issues (each table has independent rowids)
--   - Simpler joins: JOIN articles a ON articles_fts.rowid = a.id
--   - Type-specific queries are straightforward
--   - "All" queries use UNION ALL across both FTS tables
--
-- BM25 Ranking:
--   FTS5 provides a hidden `rank` column using the BM25 algorithm. Lower scores
--   indicate better matches. When searching across both content types:
--
--     SELECT * FROM (
--         SELECT ..., rank FROM articles_fts JOIN articles ...
--         UNION ALL
--         SELECT ..., rank FROM videos_fts JOIN videos ...
--     )
--     ORDER BY rank  -- Best matches from either table bubble to top
--     LIMIT 20
--
--   Since both tables use the same FTS5/BM25 engine, scores are comparable.
--   One caveat: BM25 uses Inverse Document Frequency (IDF), so a rare word
--   in one table may score differently than in another.
--
-- Future: Result Weighting
--   To bias results toward certain content types, multiply the rank:
--     SELECT ..., rank * 0.8 as rank FROM videos_fts ...  -- Boost videos
--   Lower scores = better matches, so multiplying by < 1.0 boosts results.

-- Step 1: Drop old triggers
DROP TRIGGER IF EXISTS articles_ai;
DROP TRIGGER IF EXISTS articles_ad;
DROP TRIGGER IF EXISTS articles_au;
DROP TRIGGER IF EXISTS videos_ai;
DROP TRIGGER IF EXISTS videos_ad;
DROP TRIGGER IF EXISTS videos_au;

-- Step 2: Drop old unified FTS table
DROP TABLE IF EXISTS entries_fts;

-- Step 3: Create FTS table for articles
CREATE VIRTUAL TABLE articles_fts USING fts5(
    title,
    category,
    text,
    tokenize='porter unicode61'
);

-- Step 4: Create FTS table for videos
CREATE VIRTUAL TABLE videos_fts USING fts5(
    title,
    category,
    text,
    tokenize='porter unicode61'
);

-- Step 5: Populate articles FTS
INSERT INTO articles_fts(rowid, title, category, text)
SELECT id, title, category, text
FROM articles;

-- Step 6: Populate videos FTS
INSERT INTO videos_fts(rowid, title, category, text)
SELECT id, title, category, text
FROM videos;

-- Step 7: Create triggers for articles
CREATE TRIGGER articles_ai AFTER INSERT ON articles BEGIN
    INSERT INTO articles_fts(rowid, title, category, text)
    VALUES (new.id, new.title, new.category, new.text);
END;

CREATE TRIGGER articles_ad AFTER DELETE ON articles BEGIN
    INSERT INTO articles_fts(articles_fts, rowid, title, category, text)
    VALUES ('delete', old.id, old.title, old.category, old.text);
END;

CREATE TRIGGER articles_au AFTER UPDATE ON articles BEGIN
    INSERT INTO articles_fts(articles_fts, rowid, title, category, text)
    VALUES ('delete', old.id, old.title, old.category, old.text);
    INSERT INTO articles_fts(rowid, title, category, text)
    VALUES (new.id, new.title, new.category, new.text);
END;

-- Step 8: Create triggers for videos
CREATE TRIGGER videos_ai AFTER INSERT ON videos BEGIN
    INSERT INTO videos_fts(rowid, title, category, text)
    VALUES (new.id, new.title, new.category, new.text);
END;

CREATE TRIGGER videos_ad AFTER DELETE ON videos BEGIN
    INSERT INTO videos_fts(videos_fts, rowid, title, category, text)
    VALUES ('delete', old.id, old.title, old.category, old.text);
END;

CREATE TRIGGER videos_au AFTER UPDATE ON videos BEGIN
    INSERT INTO videos_fts(videos_fts, rowid, title, category, text)
    VALUES ('delete', old.id, old.title, old.category, old.text);
    INSERT INTO videos_fts(rowid, title, category, text)
    VALUES (new.id, new.title, new.category, new.text);
END;

-- Step 9: Optimize FTS tables for faster queries
-- FTS5 tables should be optimized after bulk inserts to merge b-tree segments
INSERT INTO articles_fts(articles_fts) VALUES ('optimize');
INSERT INTO videos_fts(videos_fts) VALUES ('optimize');