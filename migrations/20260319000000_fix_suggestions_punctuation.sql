-- Migration: Fix suggestion phrases that contain trailing punctuation and
-- missed word boundaries from non-space separators.
--
-- Two bugs in the original migration (20260222000003):
--
-- 1. GLOB '[a-z]*' only checked that words START with a letter, so tokens
--    with trailing punctuation like "linz," or "rust." passed through.
--    Fix: use GLOB '[a-z]*[a-z]' to require words end with a letter too.
--
-- 2. The word-splitter only split on spaces, so titles like "Cologne/Bonn"
--    or "rust.cologne" or "async-std" were kept as single unsplittable tokens
--    and never produced useful bigrams/trigrams.
--    Fix: normalize common punctuation-as-separator characters (-  /  .  :
--    (  ) ) to spaces before splitting, so those titles yield real words.
--
-- Also adds the missing GLOB guard on w2 in the trigrams CTE.

DELETE FROM suggestions;

WITH RECURSIVE
-- Normalise each title: replace punctuation that acts as a word boundary
-- with a space, then collapse any resulting double-spaces.  SQLite has no
-- regex replace, so we chain REPLACE() calls.
normalized(id, title) AS (
  SELECT rowid,
    -- second pass: collapse double spaces introduced by replacements above
    replace(replace(replace(replace(replace(replace(replace(replace(replace(
      -- first pass: swap separator punctuation for spaces
      replace(lower(title), '&', ' '),
    '-', ' '), '/', ' '), '.', ' '), ':', ' '), '(', ' '), ')', ' '),
    '  ', ' '), '  ', ' '), '  ', ' ')
  FROM titles_vocab_fts
),
words(id, word, rest, pos) AS (
  -- Seed: first word of each normalised title
  SELECT id,
         CASE WHEN instr(title, ' ') > 0
              THEN substr(title, 1, instr(title, ' ') - 1)
              ELSE title END,
         CASE WHEN instr(title, ' ') > 0
              THEN substr(title, instr(title, ' ') + 1)
              ELSE '' END,
         1
  FROM normalized
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
    AND w1.word GLOB '[a-z]*[a-z]' AND w2.word GLOB '[a-z]*[a-z]'
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
    AND w1.word GLOB '[a-z]*[a-z]' AND w2.word GLOB '[a-z]*[a-z]' AND w3.word GLOB '[a-z]*[a-z]'
    AND w1.word NOT IN stopwords
  GROUP BY phrase
  HAVING cnt >= 3
)
INSERT INTO suggestions (phrase, cnt)
SELECT phrase, cnt FROM bigrams
UNION ALL
SELECT phrase, cnt FROM trigrams;