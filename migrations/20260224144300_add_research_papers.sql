-- Migration: Add research papers table + FTS index + triggers
-- This adds storage for research papers and keeps an FTS index in sync.

-- Step 1: Create research_papers table
CREATE TABLE IF NOT EXISTS research_papers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    date TEXT NOT NULL,
    authors TEXT NOT NULL,
    abstract_text TEXT NOT NULL,
    text TEXT,
    paper_id TEXT,
    publication TEXT
);

-- Step 2: Create indexes for common queries
CREATE INDEX IF NOT EXISTS idx_research_papers_date ON research_papers(date);
CREATE INDEX IF NOT EXISTS idx_research_papers_category ON research_papers(category);

-- Step 3: Create FTS table for research papers
CREATE VIRTUAL TABLE IF NOT EXISTS research_papers_fts USING fts5(
    title,
    category,
    authors,
    abstract_text,
    text,
    tokenize='porter unicode61'
);

-- Step 4: Populate FTS from existing rows (if any)
INSERT INTO research_papers_fts(rowid, title, category, authors, abstract_text, text)
SELECT id, title, category, authors, abstract_text, text
FROM research_papers;

-- Step 5: Create triggers to keep FTS in sync
CREATE TRIGGER IF NOT EXISTS research_papers_ai AFTER INSERT ON research_papers BEGIN
    INSERT INTO research_papers_fts(rowid, title, category, authors, abstract_text, text)
    VALUES (new.id, new.title, new.category, new.authors, new.abstract_text, new.text);
END;

CREATE TRIGGER IF NOT EXISTS research_papers_ad AFTER DELETE ON research_papers BEGIN
    INSERT INTO research_papers_fts(research_papers_fts, rowid, title, category, authors, abstract_text, text)
    VALUES ('delete', old.id, old.title, old.category, old.authors, old.abstract_text, old.text);
END;

CREATE TRIGGER IF NOT EXISTS research_papers_au AFTER UPDATE ON research_papers BEGIN
    INSERT INTO research_papers_fts(research_papers_fts, rowid, title, category, authors, abstract_text, text)
    VALUES ('delete', old.id, old.title, old.category, old.authors, old.abstract_text, old.text);
    INSERT INTO research_papers_fts(rowid, title, category, authors, abstract_text, text)
    VALUES (new.id, new.title, new.category, new.authors, new.abstract_text, new.text);
END;

-- Step 6: Optimize FTS for faster queries after bulk inserts
INSERT INTO research_papers_fts(research_papers_fts) VALUES ('optimize');
