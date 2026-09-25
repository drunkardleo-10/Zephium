-- Widen the resource kind check for Work objects and media. The referenced
-- table cannot be altered in place, so both tables are rebuilt and renamed
-- back; SQLite rewrites the receipts foreign key to the final name. Rowids are
-- kept so the external-content title index stays aligned.
DROP TRIGGER user_resource_usage_insert;
DROP TRIGGER user_resource_usage_update;
DROP TRIGGER user_resource_usage_delete;
DROP TRIGGER user_resources_capacity;
DROP TRIGGER user_resource_receipts_capacity;
DROP TRIGGER resource_titles_insert;
DROP TRIGGER resource_titles_delete;
DROP TRIGGER resource_titles_update;
DROP INDEX user_resources_listing;
DROP INDEX user_tasks_list;
DROP INDEX user_tasks_date;
DROP INDEX user_tasks_deadline;
CREATE TABLE user_resources_v28 (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id)=26),
    kind TEXT NOT NULL CHECK(kind IN ('note','task','object','media')),
    revision INTEGER NOT NULL CHECK(revision>0),
    title TEXT NOT NULL,
    completed INTEGER CHECK(completed IS NULL OR completed IN (0,1)),
    due_date TEXT,
    pinned INTEGER NOT NULL CHECK(pinned IN (0,1)),
    trashed INTEGER NOT NULL CHECK(trashed IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=524288),
    search_text TEXT NOT NULL,
    status TEXT,
    assignee TEXT,
    origin TEXT,
    context_url TEXT,
    context_title TEXT,
    sort_key TEXT,
    work TEXT,
    due_time TEXT,
    task_list TEXT,
    task_inbox INTEGER NOT NULL DEFAULT 0 CHECK(task_inbox IN (0,1)),
    task_priority TEXT NOT NULL DEFAULT 'none',
    task_steps INTEGER NOT NULL DEFAULT 0,
    task_steps_done INTEGER NOT NULL DEFAULT 0,
    task_completed_at TEXT,
    task_deadline TEXT,
    task_duration INTEGER
) STRICT;
INSERT INTO user_resources_v28(rowid,id,kind,revision,title,completed,due_date,pinned,trashed,created_at,updated_at,body,search_text,status,assignee,origin,context_url,context_title,sort_key,work,due_time,task_list,task_inbox,task_priority,task_steps,task_steps_done,task_completed_at,task_deadline,task_duration)
    SELECT rowid,id,kind,revision,title,completed,due_date,pinned,trashed,created_at,updated_at,body,search_text,status,assignee,origin,context_url,context_title,sort_key,work,due_time,task_list,task_inbox,task_priority,task_steps,task_steps_done,task_completed_at,task_deadline,task_duration FROM user_resources;
CREATE TABLE user_resource_receipts_v28 (
    request_id TEXT PRIMARY KEY NOT NULL,
    digest BLOB NOT NULL CHECK(length(digest)=32),
    retained INTEGER NOT NULL CHECK(retained IN (0,1)),
    resource_id TEXT NOT NULL REFERENCES user_resources_v28(id),
    revision INTEGER NOT NULL CHECK(revision>0)
) STRICT;
INSERT INTO user_resource_receipts_v28(request_id,digest,retained,resource_id,revision)
    SELECT request_id,digest,retained,resource_id,revision FROM user_resource_receipts;
DROP TABLE user_resource_receipts;
DROP TABLE user_resources;
ALTER TABLE user_resources_v28 RENAME TO user_resources;
ALTER TABLE user_resource_receipts_v28 RENAME TO user_resource_receipts;
CREATE TRIGGER user_resource_usage_insert AFTER INSERT ON user_resources BEGIN
    UPDATE user_resource_usage SET bytes=bytes+length(CAST(NEW.body AS BLOB)) WHERE id=1; END;
CREATE TRIGGER user_resource_usage_update AFTER UPDATE OF body ON user_resources BEGIN
    UPDATE user_resource_usage SET bytes=bytes-length(CAST(OLD.body AS BLOB))+length(CAST(NEW.body AS BLOB)) WHERE id=1; END;
CREATE TRIGGER user_resource_usage_delete AFTER DELETE ON user_resources BEGIN
    UPDATE user_resource_usage SET bytes=bytes-length(CAST(OLD.body AS BLOB)) WHERE id=1; END;
CREATE INDEX user_resources_listing ON user_resources(kind,trashed,pinned DESC,id DESC);
CREATE INDEX user_tasks_list ON user_resources(task_list,trashed,completed);
CREATE INDEX user_tasks_date ON user_resources(kind,trashed,completed,due_date);
CREATE INDEX user_tasks_deadline ON user_resources(kind,trashed,completed,task_deadline);
CREATE TRIGGER user_resources_capacity BEFORE INSERT ON user_resources
WHEN (SELECT count(*) FROM user_resources)>=10000
BEGIN SELECT RAISE(ABORT,'resource capacity'); END;
CREATE TRIGGER user_resource_receipts_capacity BEFORE INSERT ON user_resource_receipts
WHEN (SELECT count(*) FROM user_resource_receipts)>=100000
BEGIN SELECT RAISE(ABORT,'resource receipt capacity'); END;
CREATE TRIGGER resource_titles_insert AFTER INSERT ON user_resources BEGIN INSERT INTO resource_titles_fts(rowid,title) VALUES(NEW.rowid,NEW.title); END;
CREATE TRIGGER resource_titles_delete AFTER DELETE ON user_resources BEGIN INSERT INTO resource_titles_fts(resource_titles_fts,rowid,title) VALUES('delete',OLD.rowid,OLD.title); END;
CREATE TRIGGER resource_titles_update AFTER UPDATE OF title ON user_resources WHEN OLD.title IS NOT NEW.title BEGIN
    INSERT INTO resource_titles_fts(resource_titles_fts,rowid,title) VALUES('delete',OLD.rowid,OLD.title);
    INSERT INTO resource_titles_fts(rowid,title) VALUES(NEW.rowid,NEW.title); END;
