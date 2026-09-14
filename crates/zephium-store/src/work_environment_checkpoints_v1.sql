-- Scoped checkpoint identities cannot be replayed once their monotonic expected
-- revision is stale. Keep only the latest exact replay window per environment.
CREATE TABLE work_environment_checkpoints (
    environment_id TEXT NOT NULL REFERENCES work_environments(id) ON DELETE CASCADE,
    expected_revision INTEGER NOT NULL CHECK(expected_revision>0),
    digest BLOB NOT NULL CHECK(length(digest)=32),
    applied_revision INTEGER NOT NULL CHECK(applied_revision=expected_revision+1),
    PRIMARY KEY(environment_id,expected_revision)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER work_environment_checkpoints_capacity BEFORE INSERT ON work_environment_checkpoints
WHEN (SELECT count(*) FROM work_environment_checkpoints WHERE environment_id=NEW.environment_id)>=64
BEGIN SELECT RAISE(ABORT,'Work checkpoint replay window capacity'); END;
CREATE TRIGGER work_environment_checkpoints_immutable BEFORE UPDATE ON work_environment_checkpoints
BEGIN SELECT RAISE(ABORT,'Work checkpoint receipts are immutable'); END;
