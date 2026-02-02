-- Create main table for metadata
CREATE TABLE IF NOT EXISTS entries_meta (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    date TEXT NOT NULL,
    text TEXT,
    entry_type TEXT NOT NULL DEFAULT 'article',
    thumbnail_url TEXT,
    tags TEXT
);

-- Create tags table
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

-- Create entry_tags junction table
CREATE TABLE IF NOT EXISTS entry_tags (
    entry_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (entry_id, tag_id),
    FOREIGN KEY (entry_id) REFERENCES entries_meta(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
);

-- Create index for tag lookups
CREATE INDEX IF NOT EXISTS idx_entry_tags_tag ON entry_tags(tag_id);

-- Create quotes table
CREATE TABLE IF NOT EXISTS quotes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL,
    author TEXT NOT NULL,
    url TEXT,
    date TEXT NOT NULL
);

-- Create FTS5 virtual table
CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
    title,
    category,
    text,
    content='entries_meta',
    content_rowid=id,
    tokenize='porter unicode61'
);

-- Create triggers to keep FTS index in sync
CREATE TRIGGER IF NOT EXISTS entries_ai AFTER INSERT ON entries_meta BEGIN
    INSERT INTO entries_fts(rowid, title, category, text)
    VALUES (new.id, new.title, new.category, new.text);
END;

CREATE TRIGGER IF NOT EXISTS entries_ad AFTER DELETE ON entries_meta BEGIN
    INSERT INTO entries_fts(entries_fts, rowid, title, category, text)
    VALUES('delete', old.id, old.title, old.category, old.text);
END;

CREATE TRIGGER IF NOT EXISTS entries_au AFTER UPDATE ON entries_meta BEGIN
    INSERT INTO entries_fts(entries_fts, rowid, title, category, text)
    VALUES('delete', old.id, old.title, old.category, old.text);
    INSERT INTO entries_fts(rowid, title, category, text)
    VALUES (new.id, new.title, new.category, new.text);
END;

-- Create index for date-based queries
CREATE INDEX IF NOT EXISTS idx_entries_date ON entries_meta(date);