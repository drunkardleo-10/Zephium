CREATE TABLE work_memories (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 26),
    text TEXT NOT NULL CHECK (length(CAST(text AS BLOB)) BETWEEN 1 AND 1120),
    kind TEXT NOT NULL CHECK (kind IN ('preference', 'person', 'project', 'fact')),
    work TEXT REFERENCES works(id) ON DELETE SET NULL,
    execution TEXT CHECK (execution IS NULL OR length(execution) = 26),
    created_ms INTEGER NOT NULL CHECK (created_ms >= 0),
    used_ms INTEGER CHECK (used_ms IS NULL OR used_ms >= 0)
) STRICT;
CREATE INDEX work_memories_work ON work_memories(work, created_ms);
CREATE TRIGGER work_memories_capacity BEFORE INSERT ON work_memories
    WHEN (SELECT count(*) FROM work_memories) >= 500
    BEGIN SELECT RAISE(ABORT, 'work memory capacity'); END;
CREATE VIRTUAL TABLE work_memories_fts USING fts5(text, content='work_memories', content_rowid='rowid', prefix='2 3');
CREATE TRIGGER work_memories_fts_insert AFTER INSERT ON work_memories BEGIN
    INSERT INTO work_memories_fts(rowid, text) VALUES (NEW.rowid, NEW.text);
END;
CREATE TRIGGER work_memories_fts_delete AFTER DELETE ON work_memories BEGIN
    INSERT INTO work_memories_fts(work_memories_fts, rowid, text) VALUES ('delete', OLD.rowid, OLD.text);
END;
CREATE TRIGGER work_memories_fts_update AFTER UPDATE OF text ON work_memories BEGIN
    INSERT INTO work_memories_fts(work_memories_fts, rowid, text) VALUES ('delete', OLD.rowid, OLD.text);
    INSERT INTO work_memories_fts(rowid, text) VALUES (NEW.rowid, NEW.text);
END;
CREATE TABLE work_context_consent (
    work TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (source IN ('history', 'notes', 'tabs')),
    allowed INTEGER NOT NULL CHECK (allowed IN (0, 1)),
    PRIMARY KEY (work, source)
) STRICT, WITHOUT ROWID;
