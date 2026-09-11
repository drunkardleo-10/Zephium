-- Add execution events while retaining the immutable, contiguous event suffix.
DROP TRIGGER work_events_immutable;
DROP TRIGGER work_events_capacity;
ALTER TABLE work_events RENAME TO work_events_v2;
CREATE TABLE work_events (
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    author TEXT NOT NULL CHECK (author IN ('user', 'primary_agent', 'other_agent', 'legacy_unknown')),
    recorded_unix_ms INTEGER NOT NULL CHECK (recorded_unix_ms >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('created', 'objective_edited', 'question_opened', 'question_answered', 'draft_replaced', 'archived', 'restored', 'history_compacted', 'question_dismissed', 'runtime_changed')),
    PRIMARY KEY (work_id, revision)
) STRICT, WITHOUT ROWID;
INSERT INTO work_events SELECT * FROM work_events_v2;
DROP TABLE work_events_v2;
CREATE TRIGGER work_events_immutable BEFORE UPDATE ON work_events
BEGIN SELECT RAISE(ABORT, 'Work events are immutable'); END;
CREATE TRIGGER work_events_capacity BEFORE INSERT ON work_events
WHEN (SELECT count(*) FROM work_events WHERE work_id = NEW.work_id) >= 2048
BEGIN SELECT RAISE(ABORT, 'Work event capacity exceeded'); END;

CREATE TABLE work_executions (
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    execution_id TEXT NOT NULL CHECK (length(execution_id) = 26),
    plan_revision INTEGER NOT NULL,
    owner_session TEXT NOT NULL CHECK (length(owner_session) = 26),
    approved_unix_ms INTEGER NOT NULL CHECK (approved_unix_ms >= 0),
    expires_unix_ms INTEGER NOT NULL CHECK (expires_unix_ms > approved_unix_ms),
    approved_tick_ms INTEGER NOT NULL DEFAULT 0 CHECK (approved_tick_ms >= 0),
    expires_tick_ms INTEGER NOT NULL DEFAULT 1 CHECK (expires_tick_ms > approved_tick_ms),
    body TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 524288),
    PRIMARY KEY (work_id, execution_id),
    FOREIGN KEY (work_id, plan_revision) REFERENCES work_plans(work_id, revision) DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;
CREATE TRIGGER work_executions_binding_immutable BEFORE UPDATE ON work_executions
WHEN NEW.work_id != OLD.work_id OR NEW.execution_id != OLD.execution_id
    OR NEW.plan_revision != OLD.plan_revision OR NEW.owner_session != OLD.owner_session
    OR NEW.approved_unix_ms != OLD.approved_unix_ms OR NEW.expires_unix_ms != OLD.expires_unix_ms
    OR NEW.approved_tick_ms != OLD.approved_tick_ms OR NEW.expires_tick_ms != OLD.expires_tick_ms
BEGIN SELECT RAISE(ABORT, 'Work execution binding is immutable'); END;
CREATE TRIGGER work_executions_capacity BEFORE INSERT ON work_executions
WHEN (SELECT count(*) FROM work_executions WHERE work_id = NEW.work_id) >= 16
BEGIN SELECT RAISE(ABORT, 'Work execution capacity exceeded'); END;
CREATE TABLE work_commands (
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    command_id TEXT NOT NULL CHECK (length(command_id) = 26),
    request_digest BLOB NOT NULL CHECK (length(request_digest) = 32),
    body TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 512),
    PRIMARY KEY (work_id, command_id)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER work_commands_capacity BEFORE INSERT ON work_commands
WHEN (SELECT count(*) FROM work_commands WHERE work_id = NEW.work_id) >= 256
BEGIN SELECT RAISE(ABORT, 'Work command capacity exceeded'); END;
CREATE TRIGGER work_commands_immutable BEFORE UPDATE ON work_commands
BEGIN SELECT RAISE(ABORT, 'Work command receipts are immutable'); END;

CREATE TRIGGER work_executions_payload_insert AFTER INSERT ON work_executions
BEGIN
 UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.body AS BLOB)) WHERE id = 1;
 SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_executions_payload_update AFTER UPDATE ON work_executions
BEGIN
 UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.body AS BLOB)) - length(CAST(OLD.body AS BLOB)) WHERE id = 1;
 SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_executions_payload_delete AFTER DELETE ON work_executions
BEGIN
 UPDATE work_payload_usage SET bytes = bytes - length(CAST(OLD.body AS BLOB)) WHERE id = 1;
 SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_commands_payload_insert AFTER INSERT ON work_commands
BEGIN
 UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.body AS BLOB)) + 32 WHERE id = 1;
 SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_commands_payload_delete AFTER DELETE ON work_commands
BEGIN
 UPDATE work_payload_usage SET bytes = bytes - length(CAST(OLD.body AS BLOB)) - 32 WHERE id = 1;
 SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
