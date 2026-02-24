-- Migration: Simplify speakers table to only id + name

PRAGMA foreign_keys = OFF;



CREATE TABLE IF NOT EXISTS speakers_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE CHECK (length(name) > 0)
);

INSERT INTO speakers_new (id, name)
SELECT id, name
FROM speakers;

DROP TABLE speakers;

ALTER TABLE speakers_new RENAME TO speakers;

CREATE INDEX IF NOT EXISTS idx_speakers_name ON speakers(name);



PRAGMA foreign_keys = ON;