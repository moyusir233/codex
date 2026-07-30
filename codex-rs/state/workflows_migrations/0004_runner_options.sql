ALTER TABLE workflow_runs ADD COLUMN non_interactive INTEGER NOT NULL DEFAULT 0
    CHECK(non_interactive IN (0, 1));
ALTER TABLE workflow_runs ADD COLUMN detached INTEGER NOT NULL DEFAULT 0
    CHECK(detached IN (0, 1));
ALTER TABLE workflow_runs ADD COLUMN concurrency INTEGER
    CHECK(concurrency IS NULL OR concurrency > 0);
