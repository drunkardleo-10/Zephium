-- Profile-scoped receipts outlive Work deletion, preventing lost-response
-- replay from creating a second Work or resurrecting deleted content.
CREATE TABLE work_authoring_commands (
    command_id TEXT PRIMARY KEY CHECK (length(command_id) = 26),
    request_digest BLOB NOT NULL CHECK (length(request_digest) = 32),
    body TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 512)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER work_authoring_commands_capacity BEFORE INSERT ON work_authoring_commands
WHEN (SELECT count(*) FROM work_authoring_commands) >= 4096
BEGIN SELECT RAISE(ABORT, 'Work authoring command capacity exceeded'); END;
CREATE TRIGGER work_authoring_commands_immutable BEFORE UPDATE ON work_authoring_commands
BEGIN SELECT RAISE(ABORT, 'Work command receipts are immutable'); END;
CREATE TRIGGER work_authoring_commands_payload_insert AFTER INSERT ON work_authoring_commands
BEGIN
 UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.body AS BLOB)) + 32 WHERE id = 1;
 SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_authoring_commands_payload_delete AFTER DELETE ON work_authoring_commands
BEGIN
 UPDATE work_payload_usage SET bytes = bytes - length(CAST(OLD.body AS BLOB)) - 32 WHERE id = 1;
 SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
