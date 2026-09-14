-- Widen the resource kind check for Work objects and media. The referenced
-- table cannot be altered in place, so both tables are rebuilt and renamed
-- back; SQLite rewrites the receipts foreign key to the final name.
DROP TRIGGER user_resource_usage_insert;
DROP TRIGGER user_resource_usage_update;
DROP TRIGGER user_resource_usage_delete;
DROP TRIGGER user_resources_capacity;
DROP TRIGGER user_resource_receipts_capacity;
DROP INDEX user_resources_listing;
CREATE TABLE user_resources_v21 (
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
    search_text TEXT NOT NULL
) STRICT;
INSERT INTO user_resources_v21(id,kind,revision,title,completed,due_date,pinned,trashed,created_at,updated_at,body,search_text)
    SELECT id,kind,revision,title,completed,due_date,pinned,trashed,created_at,updated_at,body,search_text FROM user_resources;
CREATE TABLE user_resource_receipts_v21 (
    request_id TEXT PRIMARY KEY NOT NULL,
    digest BLOB NOT NULL CHECK(length(digest)=32),
    retained INTEGER NOT NULL CHECK(retained IN (0,1)),
    resource_id TEXT NOT NULL REFERENCES user_resources_v21(id),
    revision INTEGER NOT NULL CHECK(revision>0)
) STRICT;
INSERT INTO user_resource_receipts_v21(request_id,digest,retained,resource_id,revision)
    SELECT request_id,digest,retained,resource_id,revision FROM user_resource_receipts;
DROP TABLE user_resource_receipts;
DROP TABLE user_resources;
ALTER TABLE user_resources_v21 RENAME TO user_resources;
ALTER TABLE user_resource_receipts_v21 RENAME TO user_resource_receipts;
CREATE TRIGGER user_resource_usage_insert AFTER INSERT ON user_resources BEGIN
    UPDATE user_resource_usage SET bytes=bytes+length(CAST(NEW.body AS BLOB)) WHERE id=1; END;
CREATE TRIGGER user_resource_usage_update AFTER UPDATE OF body ON user_resources BEGIN
    UPDATE user_resource_usage SET bytes=bytes-length(CAST(OLD.body AS BLOB))+length(CAST(NEW.body AS BLOB)) WHERE id=1; END;
CREATE TRIGGER user_resource_usage_delete AFTER DELETE ON user_resources BEGIN
    UPDATE user_resource_usage SET bytes=bytes-length(CAST(OLD.body AS BLOB)) WHERE id=1; END;
CREATE INDEX user_resources_listing ON user_resources(kind,trashed,pinned DESC,id DESC);
CREATE TRIGGER user_resources_capacity BEFORE INSERT ON user_resources
WHEN (SELECT count(*) FROM user_resources)>=10000
BEGIN SELECT RAISE(ABORT,'resource capacity'); END;
CREATE TRIGGER user_resource_receipts_capacity BEFORE INSERT ON user_resource_receipts
WHEN (SELECT count(*) FROM user_resource_receipts)>=100000
BEGIN SELECT RAISE(ABORT,'resource receipt capacity'); END;
