-- Migration: Pre-materialise search suggestion phrases
--
-- Builds a table of common 2–3 word phrases extracted from all titles using a
-- recursive CTE word-splitter + self-join. These become the autocomplete
-- suggestions shown as the user types.
--
-- Query pattern at runtime:
--   SELECT phrase FROM suggestions WHERE phrase LIKE ? || '%'
--   ORDER BY cnt DESC LIMIT 5
-- This is a simple index-range scan — sub-millisecond.

CREATE TABLE IF NOT EXISTS suggestions (
    phrase TEXT NOT NULL,
    cnt    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_suggestions_phrase ON suggestions(phrase);

-- Populate via a recursive word-split CTE over all titles.
-- titles_vocab_fts already contains all titles from articles, videos,
-- podcast episodes, and talks (populated in migration 20260222000002).
WITH RECURSIVE
words(id, word, rest, pos) AS (
  -- Seed: first word of each title
  SELECT rowid,
         CASE WHEN instr(lower(title), ' ') > 0
              THEN substr(lower(title), 1, instr(lower(title), ' ') - 1)
              ELSE lower(title) END,
         CASE WHEN instr(lower(title), ' ') > 0
              THEN substr(lower(title), instr(lower(title), ' ') + 1)
              ELSE '' END,
         1
  FROM titles_vocab_fts
  UNION ALL
  -- Step: next word
  SELECT id,
         CASE WHEN instr(rest, ' ') > 0
              THEN substr(rest, 1, instr(rest, ' ') - 1)
              ELSE rest END,
         CASE WHEN instr(rest, ' ') > 0
              THEN substr(rest, instr(rest, ' ') + 1)
              ELSE '' END,
         pos + 1
  FROM words WHERE rest != ''
),
stopwords(w) AS (
  SELECT 'the' UNION ALL SELECT 'and' UNION ALL SELECT 'for' UNION ALL
  SELECT 'with' UNION ALL SELECT 'from' UNION ALL SELECT 'are' UNION ALL
  SELECT 'this' UNION ALL SELECT 'that' UNION ALL SELECT 'its' UNION ALL
  SELECT 'but' UNION ALL SELECT 'not' UNION ALL SELECT 'yet' UNION ALL
  SELECT 'our' UNION ALL SELECT 'get' UNION ALL SELECT 'let' UNION ALL
  SELECT 'use' UNION ALL SELECT 'can' UNION ALL SELECT 'has' UNION ALL
  SELECT 'was' UNION ALL SELECT 'have' UNION ALL SELECT 'will' UNION ALL
  SELECT 'into' UNION ALL SELECT 'via' UNION ALL SELECT 'your' UNION ALL
  SELECT 'more' UNION ALL SELECT 'what' UNION ALL SELECT 'when' UNION ALL
  SELECT 'which' UNION ALL SELECT 'about' UNION ALL SELECT 'using'
),
bigrams AS (
  SELECT w1.word || ' ' || w2.word AS phrase, COUNT(*) AS cnt
  FROM words w1
  JOIN words w2 ON w1.id = w2.id AND w2.pos = w1.pos + 1
  WHERE length(w1.word) >= 3 AND length(w2.word) >= 3
    AND w1.word GLOB '[a-z]*' AND w2.word GLOB '[a-z]*'
    AND w1.word NOT IN stopwords
    AND w2.word NOT IN stopwords
  GROUP BY phrase
  HAVING cnt >= 3
),
trigrams AS (
  SELECT w1.word || ' ' || w2.word || ' ' || w3.word AS phrase, COUNT(*) AS cnt
  FROM words w1
  JOIN words w2 ON w1.id = w2.id AND w2.pos = w1.pos + 1
  JOIN words w3 ON w1.id = w3.id AND w3.pos = w1.pos + 2
  WHERE length(w1.word) >= 3 AND length(w2.word) >= 2 AND length(w3.word) >= 3
    AND w1.word GLOB '[a-z]*' AND w3.word GLOB '[a-z]*'
    AND w1.word NOT IN stopwords
  GROUP BY phrase
  HAVING cnt >= 3
)
INSERT INTO suggestions (phrase, cnt)
SELECT phrase, cnt FROM bigrams
UNION ALL
SELECT phrase, cnt FROM trigrams;