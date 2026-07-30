PRAGMA foreign_keys = ON;

CREATE TABLE workflow_runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    definition_name TEXT NOT NULL,
    definition_version TEXT NOT NULL,
    state_schema_version INTEGER NOT NULL CHECK(state_schema_version > 0),
    state_json TEXT NOT NULL,
    arguments_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN (
        'pending', 'running', 'waiting', 'cancelling',
        'succeeded', 'failed', 'cancelled', 'needs_operator'
    )),
    output_json TEXT,
    error_code TEXT,
    row_version INTEGER NOT NULL DEFAULT 1 CHECK(row_version > 0),
    next_sequence INTEGER NOT NULL DEFAULT 1 CHECK(next_sequence > 0),
    lease_owner TEXT,
    lease_expires_at_ms INTEGER,
    lease_fence INTEGER NOT NULL DEFAULT 0 CHECK(lease_fence >= 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX workflow_runs_status_updated_idx
    ON workflow_runs(status, updated_at_ms, run_id);
CREATE INDEX workflow_runs_lease_idx
    ON workflow_runs(lease_expires_at_ms, status);

CREATE TABLE workflow_nodes (
    node_id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    node_key TEXT NOT NULL,
    thread_id TEXT,
    spec_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN (
        'pending', 'ready', 'running', 'waiting',
        'succeeded', 'failed', 'cancelled', 'blocked'
    )),
    row_version INTEGER NOT NULL DEFAULT 1 CHECK(row_version > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE(run_id, node_key)
);

CREATE INDEX workflow_nodes_run_status_idx
    ON workflow_nodes(run_id, status, node_key);

CREATE TABLE workflow_node_dependencies (
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    node_id TEXT NOT NULL REFERENCES workflow_nodes(node_id) ON DELETE CASCADE,
    depends_on_node_id TEXT NOT NULL REFERENCES workflow_nodes(node_id) ON DELETE CASCADE,
    PRIMARY KEY(node_id, depends_on_node_id),
    CHECK(node_id <> depends_on_node_id)
);

CREATE TABLE workflow_node_attempts (
    attempt_id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    node_id TEXT NOT NULL REFERENCES workflow_nodes(node_id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL CHECK(attempt_number > 0),
    submission_id TEXT,
    input_hash TEXT,
    turn_id TEXT,
    status TEXT NOT NULL CHECK(status IN (
        'planned', 'submitted', 'running', 'succeeded',
        'failed', 'interrupted', 'cancelled', 'ambiguous'
    )),
    started_at_ms INTEGER,
    completed_at_ms INTEGER,
    error_code TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE(node_id, attempt_number),
    UNIQUE(submission_id)
);

CREATE INDEX workflow_node_attempts_run_node_idx
    ON workflow_node_attempts(run_id, node_id, attempt_number);

CREATE TABLE workflow_effects (
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    effect_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    request_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN (
        'planned', 'dispatched', 'applied', 'ambiguous', 'cancelled'
    )),
    response_json TEXT,
    error_code TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY(run_id, effect_key)
);

CREATE INDEX workflow_effects_state_idx
    ON workflow_effects(state, updated_at_ms, run_id);

CREATE TABLE workflow_interactions (
    interaction_id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    request_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN (
        'planned', 'waiting', 'resolved', 'timed_out', 'cancelled'
    )),
    response_artifact_id TEXT,
    deadline_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE(run_id, dedupe_key)
);

CREATE TABLE workflow_events (
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    kind TEXT NOT NULL,
    entity_id TEXT,
    metadata_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY(run_id, sequence)
);

CREATE INDEX workflow_events_created_idx
    ON workflow_events(run_id, created_at_ms, sequence);

CREATE TABLE workflow_artifacts (
    artifact_id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    classification TEXT NOT NULL CHECK(classification IN (
        'public', 'internal', 'sensitive'
    )),
    media_type TEXT NOT NULL,
    byte_count INTEGER NOT NULL CHECK(byte_count >= 0),
    sha256 TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    UNIQUE(run_id, relative_path)
);

CREATE TABLE workflow_external_events (
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    external_id TEXT NOT NULL,
    received_at_ms INTEGER NOT NULL,
    metadata_json TEXT NOT NULL,
    PRIMARY KEY(source, external_id)
);
