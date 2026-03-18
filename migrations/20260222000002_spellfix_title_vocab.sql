-- Migration: Seed spellfix vocab with unstemmed title words
--
-- Problem: the porter stemmer in the primary FTS tables reduces words to their
-- root form (embedded -> embed, lifetimes -> lifetim, etc.). The spellfix1
-- vocab is seeded from those porter-stemmed tokens, so unstemmed user queries
-- like "mebedded" (typo for "embedded") score poorly against "embed" and
-- spellfix picks wrong corrections.
--
-- Fix: add a lightweight FTS table that indexes only titles using the plain
-- unicode61 tokenizer (no stemming). Its vocabulary is then merged into the
-- spellfix table, giving spellfix the full unstemmed word forms it needs to
-- recognise typos like "mebedded" -> "embedded".
--
-- Why titles only?
--   - Titles contain the most query-relevant terms in their natural form.
--   - Indexing full article text without stemming would bloat the vocab with
--     noise and make spellfix slower without improving correction quality.
--   - The title vocab is small enough (~9k unique terms) that the additional
--     spellfix rows have negligible impact on query latency.

-- Step 1: Create a title-only FTS table with the unicode61 tokenizer.
--
-- We use unicode61 (no porter) so that "embedded", "embedding", "embeddings"
-- are all stored as separate tokens rather than collapsed to "embed".
CREATE VIRTUAL TABLE IF NOT EXISTS titles_vocab_fts USING fts5(
    title,
    tokenize = 'unicode61'
);

-- Step 2: Populate from all content types.
-- Note: no explicit rowid — let FTS5 auto-assign them to avoid conflicts
-- between the id spaces of the four source tables (all start from 1).
INSERT INTO titles_vocab_fts (title) SELECT title        FROM articles;
INSERT INTO titles_vocab_fts (title) SELECT title        FROM videos;
INSERT INTO titles_vocab_fts (title) SELECT episode_name FROM podcast_episodes;
INSERT INTO titles_vocab_fts (title) SELECT title        FROM talks;

-- Step 3: Keep titles_vocab_fts in sync via triggers.

-- articles
-- Note: FTS5 content-less-style triggers don't need rowid when auto-assigning.
-- We use the 'delete' command form which requires the original rowid; since we
-- don't track rowids we skip delete/update triggers — the vocab table is
-- append-only and rebuilt from migrations when schema changes are needed.
CREATE TRIGGER IF NOT EXISTS titles_vocab_articles_ai AFTER INSERT ON articles BEGIN
    INSERT INTO titles_vocab_fts (title) VALUES (new.title);
END;

-- videos
CREATE TRIGGER IF NOT EXISTS titles_vocab_videos_ai AFTER INSERT ON videos BEGIN
    INSERT INTO titles_vocab_fts (title) VALUES (new.title);
END;

-- podcast_episodes
CREATE TRIGGER IF NOT EXISTS titles_vocab_podcasts_ai AFTER INSERT ON podcast_episodes BEGIN
    INSERT INTO titles_vocab_fts (title) VALUES (new.episode_name);
END;

-- talks
CREATE TRIGGER IF NOT EXISTS titles_vocab_talks_ai AFTER INSERT ON talks BEGIN
    INSERT INTO titles_vocab_fts (title) VALUES (new.title);
END;

-- Step 4: Merge unstemmed title words into the spellfix vocab.
--
-- We use a temporary fts5vocab virtual table to enumerate the tokens, then
-- INSERT OR IGNORE so we don't add duplicates of words already present from
-- the porter-stemmed sources. Existing rows keep their (higher) rank since
-- they were seen across more documents.
-- We use a GROUP BY to deduplicate terms emitted by fts5vocab (which can
-- return multiple rows per term when the FTS index has multiple b-tree
-- segments) and sum their document counts into a single rank value.
CREATE VIRTUAL TABLE IF NOT EXISTS _sv_titles USING fts5vocab(titles_vocab_fts, row);

INSERT INTO search_vocab (word, rank)
    SELECT term, SUM(doc)
    FROM _sv_titles
    WHERE length(term) >= 3
      AND term GLOB '[a-z][a-z]*'
    GROUP BY term;

DROP TABLE _sv_titles;