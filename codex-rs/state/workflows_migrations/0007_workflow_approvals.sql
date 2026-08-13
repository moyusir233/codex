CREATE TABLE workflow_approvals (
    approval_id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    effect_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    gate TEXT NOT NULL,
    subject_json TEXT NOT NULL,
    allowed_approvers_json TEXT NOT NULL,
    quorum INTEGER NOT NULL CHECK (quorum > 0),
    deadline_ms INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    UNIQUE(run_id, effect_key)
);

CREATE INDEX workflow_approvals_run_idx
    ON workflow_approvals(run_id, created_at_ms, approval_id);

CREATE TABLE workflow_approval_decisions (
    decision_id TEXT PRIMARY KEY NOT NULL,
    approval_id TEXT NOT NULL REFERENCES workflow_approvals(approval_id) ON DELETE CASCADE,
    sender_id TEXT NOT NULL,
    message_id TEXT,
    response_artifact_id TEXT REFERENCES workflow_artifacts(artifact_id),
    decision TEXT NOT NULL CHECK (decision IN ('approve', 'changes_requested', 'revoke')),
    reason TEXT,
    created_at_ms INTEGER NOT NULL,
    UNIQUE(approval_id, message_id)
);

CREATE INDEX workflow_approval_decisions_history_idx
    ON workflow_approval_decisions(approval_id, created_at_ms, decision_id);
