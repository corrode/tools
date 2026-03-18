-- Migration: Add spellfix1 vocabulary table for typo-tolerant search
--
-- spellfix1 is a SQLite loadable extension that finds close matches for
-- misspelled words using a phonetic hash + edit-distance algorithm.
-- See: https://www.sqlite.org/spellfix1.html
--
-- ## How it fits into search
--
-- The primary FTS tables use the `porter unicode61` tokenizer, which is
-- exact-token-based: "asyncronous" will never match "asynchronous".
-- When a porter FTS query returns zero results, the application queries
-- the spellfix1 table to get corrected spellings for each search term,
-- then retries the FTS query with the corrections applied.
--
-- ## Vocabulary source
--
-- The vocabulary is extracted from the FTS indexes themselves using the
-- built-in `fts5vocab` virtual table, which enumerates every unique token
-- stored in a given FTS5 index together with its document frequency.
-- We use the document frequency as the `rank` hint to spellfix1 so that
-- common words are slightly preferred over rare ones when edit distances
-- are equal.
--
-- Only tokens that:
--   - are at least 3 characters long (single/double-char tokens produce
--     noisy corrections and are unlikely to be mistyped search terms), and
--   - consist entirely of lowercase ASCII letters (numbers and symbols are
--     not useful spell-correction targets)
-- are inserted into the vocabulary.
--
-- ## Keeping the vocabulary in sync
--
-- Unlike the FTS tables, the spellfix1 table does NOT need triggers for
-- every insert/update/delete on the source tables.  The vocabulary is a
-- best-effort spelling aid; a slightly stale word list causes no
-- correctness problems, only minor quality degradation.
--
-- The vocabulary is refreshed by the application on startup (see
-- `Repository::refresh_spellfix_vocab`) if the source FTS indexes have
-- grown since the last refresh.  This is much cheaper than per-row
-- triggers on tables with potentially thousands of rows.
--
-- ## Extension requirement
--
-- The spellfix1 module is compiled into the binary via `ext/spellfix.c`
-- and registered with `sqlite3_auto_extension` before the connection pool
-- is created, so it is available for every connection without needing a
-- separate `.so`/`.dylib` file at runtime.

-- Step 1: Create the spellfix1 virtual table.
--
-- The shadow table it creates (`search_vocab_vocab`) persists the
-- vocabulary between restarts, so we only need to populate it once
-- (or refresh it periodically).
CREATE VIRTUAL TABLE IF NOT EXISTS search_vocab USING spellfix1;

-- Step 2: Populate the vocabulary from all FTS indexes.
--
-- We use temporary `fts5vocab` views (they don't persist) to read the
-- token lists from each FTS table.  Each SELECT is its own INSERT so
-- that a failure in one table doesn't roll back the others.
--
-- articles_fts
CREATE VIRTUAL TABLE IF NOT EXISTS _vocab_articles USING fts5vocab(articles_fts, row);
INSERT INTO search_vocab(word, rank)
    SELECT term, doc
    FROM _vocab_articles
    WHERE length(term) >= 3
      AND term GLOB '[a-z][a-z]*';
DROP TABLE _vocab_articles;

-- videos_fts
CREATE VIRTUAL TABLE IF NOT EXISTS _vocab_videos USING fts5vocab(videos_fts, row);
INSERT INTO search_vocab(word, rank)
    SELECT term, doc
    FROM _vocab_videos
    WHERE length(term) >= 3
      AND term GLOB '[a-z][a-z]*';
DROP TABLE _vocab_videos;

-- podcast_episodes_fts
CREATE VIRTUAL TABLE IF NOT EXISTS _vocab_podcasts USING fts5vocab(podcast_episodes_fts, row);
INSERT INTO search_vocab(word, rank)
    SELECT term, doc
    FROM _vocab_podcasts
    WHERE length(term) >= 3
      AND term GLOB '[a-z][a-z]*';
DROP TABLE _vocab_podcasts;

-- talks_fts
CREATE VIRTUAL TABLE IF NOT EXISTS _vocab_talks USING fts5vocab(talks_fts, row);
INSERT INTO search_vocab(word, rank)
    SELECT term, doc
    FROM _vocab_talks
    WHERE length(term) >= 3
      AND term GLOB '[a-z][a-z]*';
DROP TABLE _vocab_talks;