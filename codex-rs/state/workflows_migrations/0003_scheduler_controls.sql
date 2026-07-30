ALTER TABLE workflow_runs ADD COLUMN wake_json TEXT;
ALTER TABLE workflow_runs ADD COLUMN cancellation_requested_at_ms INTEGER;
ALTER TABLE workflow_runs ADD COLUMN deadline_ms INTEGER;

ALTER TABLE workflow_nodes ADD COLUMN retry_at_ms INTEGER;
ALTER TABLE workflow_nodes ADD COLUMN failure_policy TEXT NOT NULL DEFAULT 'fail_fast'
    CHECK(failure_policy IN ('fail_fast', 'continue_independent', 'skip_dependents'));

ALTER TABLE workflow_node_dependencies ADD COLUMN policy TEXT NOT NULL DEFAULT 'all_succeeded'
    CHECK(policy IN ('all_succeeded', 'all_terminal', 'at_least'));
ALTER TABLE workflow_node_dependencies ADD COLUMN at_least INTEGER
    CHECK(at_least IS NULL OR at_least > 0);

ALTER TABLE workflow_node_attempts ADD COLUMN thread_id TEXT;

CREATE TABLE workflow_node_threads (
    node_id TEXT NOT NULL REFERENCES workflow_nodes(node_id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal > 0),
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY(node_id, ordinal),
    UNIQUE(thread_id)
);

INSERT INTO workflow_node_threads (node_id, thread_id, ordinal, created_at_ms)
SELECT node_id, thread_id, 1, created_at_ms
FROM workflow_nodes
WHERE thread_id IS NOT NULL;

CREATE INDEX workflow_runs_recovery_idx
    ON workflow_runs(status, lease_expires_at_ms, updated_at_ms);
CREATE INDEX workflow_nodes_retry_idx
    ON workflow_nodes(run_id, status, retry_at_ms, node_key);
CREATE INDEX workflow_node_dependencies_upstream_idx
    ON workflow_node_dependencies(run_id, depends_on_node_id, node_id);
