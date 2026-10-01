CREATE TABLE works (
    id TEXT PRIMARY KEY CHECK (length(id) = 26),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    revision INTEGER NOT NULL CHECK (revision BETWEEN 1 AND 2048),
    status TEXT NOT NULL CHECK (status IN ('draft', 'needs_input', 'plan_ready')),
    objective TEXT NOT NULL CHECK (length(CAST(objective AS BLOB)) BETWEEN 1 AND 8192),
    created_unix_ms INTEGER NOT NULL CHECK (created_unix_ms >= 0),
    updated_unix_ms INTEGER NOT NULL CHECK (updated_unix_ms >= created_unix_ms),
    current_plan INTEGER,
    FOREIGN KEY (id, current_plan) REFERENCES work_plans(work_id, revision)
) STRICT;
CREATE TABLE work_plans (
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision BETWEEN 2 AND 2048),
    plan_id TEXT NOT NULL CHECK (length(plan_id) = 26),
    basis_revision INTEGER NOT NULL CHECK (basis_revision = revision - 1),
    PRIMARY KEY (work_id, revision)
) STRICT, WITHOUT ROWID;
CREATE TABLE work_plan_nodes (
    work_id TEXT NOT NULL,
    plan_revision INTEGER NOT NULL,
    node_id TEXT NOT NULL CHECK (length(node_id) = 26),
    position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 63),
    body TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 131072),
    PRIMARY KEY (work_id, plan_revision, node_id),
    UNIQUE (work_id, plan_revision, position),
    FOREIGN KEY (work_id, plan_revision) REFERENCES work_plans(work_id, revision) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE TABLE work_questions (
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    question_id TEXT NOT NULL CHECK (length(question_id) = 26),
    position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 31),
    body TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 131072),
    PRIMARY KEY (work_id, question_id),
    UNIQUE (work_id, position)
) STRICT, WITHOUT ROWID;
CREATE TABLE work_events (
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision BETWEEN 1 AND 2048),
    recorded_unix_ms INTEGER NOT NULL CHECK (recorded_unix_ms >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('created', 'objective_edited', 'question_opened', 'question_answered', 'draft_replaced')),
    PRIMARY KEY (work_id, revision)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER works_capacity BEFORE INSERT ON works
WHEN (SELECT count(*) FROM works) >= 256
BEGIN SELECT RAISE(ABORT, 'Work capacity exceeded'); END;
CREATE TRIGGER work_plans_capacity BEFORE INSERT ON work_plans
WHEN (SELECT count(*) FROM work_plans WHERE work_id = NEW.work_id) >= 32
BEGIN SELECT RAISE(ABORT, 'Work plan capacity exceeded'); END;
CREATE TRIGGER work_plans_immutable BEFORE UPDATE ON work_plans
BEGIN SELECT RAISE(ABORT, 'Work plan revisions are immutable'); END;
CREATE TRIGGER work_plan_nodes_immutable BEFORE UPDATE ON work_plan_nodes
BEGIN SELECT RAISE(ABORT, 'Work plan nodes are immutable'); END;
CREATE TRIGGER work_events_immutable BEFORE UPDATE ON work_events
BEGIN SELECT RAISE(ABORT, 'Work events are immutable'); END;

CREATE TABLE work_payload_usage (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    bytes INTEGER NOT NULL CONSTRAINT work_payload_budget CHECK (bytes BETWEEN 0 AND 33554432)
) STRICT;
INSERT INTO work_payload_usage(id, bytes) VALUES (1, 0);
CREATE TRIGGER works_payload_insert AFTER INSERT ON works
BEGIN
    UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.objective AS BLOB)) WHERE id = 1;
    SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER works_payload_delete AFTER DELETE ON works
BEGIN
    UPDATE work_payload_usage SET bytes = bytes + -length(CAST(OLD.objective AS BLOB)) WHERE id = 1;
    SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER works_payload_update AFTER UPDATE ON works
BEGIN
    UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.objective AS BLOB)) - length(CAST(OLD.objective AS BLOB)) WHERE id = 1;
    SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_plan_nodes_payload_insert AFTER INSERT ON work_plan_nodes
BEGIN
    UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.body AS BLOB)) WHERE id = 1;
    SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_plan_nodes_payload_delete AFTER DELETE ON work_plan_nodes
BEGIN
    UPDATE work_payload_usage SET bytes = bytes + -length(CAST(OLD.body AS BLOB)) WHERE id = 1;
    SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_questions_payload_insert AFTER INSERT ON work_questions
BEGIN
    UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.body AS BLOB)) WHERE id = 1;
    SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_questions_payload_delete AFTER DELETE ON work_questions
BEGIN
    UPDATE work_payload_usage SET bytes = bytes + -length(CAST(OLD.body AS BLOB)) WHERE id = 1;
    SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
CREATE TRIGGER work_questions_payload_update AFTER UPDATE ON work_questions
BEGIN
    UPDATE work_payload_usage SET bytes = bytes + length(CAST(NEW.body AS BLOB)) - length(CAST(OLD.body AS BLOB)) WHERE id = 1;
    SELECT CASE WHEN changes() != 1 THEN RAISE(ABORT, 'missing Work payload accounting') END;
END;
