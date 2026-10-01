-- The environment owns no execution authority. Existing works/attempt tables
-- retain their identities and are referenced as objective history.
CREATE TABLE work_environments (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id)=26),
    space_id TEXT NOT NULL CHECK(length(space_id)=26),
    revision INTEGER NOT NULL CHECK(revision>0),
    view_revision INTEGER NOT NULL CHECK(view_revision>0),
    body TEXT NOT NULL CHECK(length(CAST(body AS BLOB)) BETWEEN 1 AND 262144),
    view TEXT NOT NULL CHECK(length(CAST(view AS BLOB)) BETWEEN 1 AND 65536)
) STRICT;
CREATE INDEX work_environments_space ON work_environments(space_id,id);
CREATE TABLE work_environment_selection (
    space_id TEXT PRIMARY KEY NOT NULL CHECK(length(space_id)=26),
    environment_id TEXT NOT NULL REFERENCES work_environments(id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE TRIGGER work_environments_capacity BEFORE INSERT ON work_environments
WHEN (SELECT count(*) FROM work_environments)>=512
BEGIN SELECT RAISE(ABORT,'Work environment capacity'); END;
CREATE TRIGGER work_environments_usage_insert AFTER INSERT ON work_environments BEGIN
    UPDATE work_payload_usage SET bytes=bytes+length(CAST(NEW.body AS BLOB))+length(CAST(NEW.view AS BLOB)) WHERE id=1;
    SELECT CASE WHEN changes()!=1 THEN RAISE(ABORT,'missing Work payload accounting') END;
END;
CREATE TRIGGER work_environments_usage_update AFTER UPDATE ON work_environments BEGIN
    UPDATE work_payload_usage SET bytes=bytes-length(CAST(OLD.body AS BLOB))-length(CAST(OLD.view AS BLOB))+length(CAST(NEW.body AS BLOB))+length(CAST(NEW.view AS BLOB)) WHERE id=1;
    SELECT CASE WHEN changes()!=1 THEN RAISE(ABORT,'missing Work payload accounting') END;
END;
CREATE TRIGGER work_environments_usage_delete AFTER DELETE ON work_environments BEGIN
    UPDATE work_payload_usage SET bytes=bytes-length(CAST(OLD.body AS BLOB))-length(CAST(OLD.view AS BLOB)) WHERE id=1;
    SELECT CASE WHEN changes()!=1 THEN RAISE(ABORT,'missing Work payload accounting') END;
END;
CREATE TABLE work_environment_commands (
    command_id TEXT PRIMARY KEY NOT NULL CHECK(length(command_id)=26),
    digest BLOB NOT NULL CHECK(length(digest)=32),
    environment_id TEXT NOT NULL CHECK(length(environment_id)=26),
    revision INTEGER NOT NULL CHECK(revision>0),
    view_revision INTEGER NOT NULL CHECK(view_revision>0)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER work_environment_commands_capacity BEFORE INSERT ON work_environment_commands
WHEN (SELECT count(*) FROM work_environment_commands)>=8192
BEGIN SELECT RAISE(ABORT,'Work environment receipt capacity'); END;
CREATE TRIGGER work_environment_commands_immutable BEFORE UPDATE ON work_environment_commands
BEGIN SELECT RAISE(ABORT,'Work environment receipts are immutable'); END;
