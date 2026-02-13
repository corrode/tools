-- Fix podcast FTS triggers to use correct delete syntax

DROP TRIGGER IF EXISTS podcast_episodes_ai;
DROP TRIGGER IF EXISTS podcast_episodes_ad;
DROP TRIGGER IF EXISTS podcast_episodes_au;

CREATE TRIGGER podcast_episodes_ai AFTER INSERT ON podcast_episodes BEGIN
    INSERT INTO podcast_episodes_fts(rowid, title, category, summary, transcript)
    VALUES (new.id, new.title, new.category, new.summary, new.transcript);
END;

CREATE TRIGGER podcast_episodes_ad AFTER DELETE ON podcast_episodes BEGIN
    DELETE FROM podcast_episodes_fts WHERE rowid = old.id;
END;

CREATE TRIGGER podcast_episodes_au AFTER UPDATE ON podcast_episodes BEGIN
    DELETE FROM podcast_episodes_fts WHERE rowid = old.id;
    INSERT INTO podcast_episodes_fts(rowid, title, category, summary, transcript)
    VALUES (new.id, new.title, new.category, new.summary, new.transcript);
END;