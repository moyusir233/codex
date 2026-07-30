CREATE TABLE workflow_fornax_traces (
    run_id TEXT NOT NULL,
    effect_key TEXT NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    request_hash TEXT NOT NULL,
    span_handle_id TEXT,
    trace_context_id TEXT,
    trace_id TEXT,
    span_id TEXT,
    bridge_instance_id TEXT,
    state TEXT NOT NULL CHECK(state IN (
        'planned', 'live', 'applied', 'finished', 'orphaned', 'ambiguous', 'failed'
    )),
    error_code TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY(run_id, effect_key),
    FOREIGN KEY(run_id, effect_key)
        REFERENCES workflow_effects(run_id, effect_key) ON DELETE CASCADE
);

CREATE INDEX workflow_fornax_traces_state_idx
    ON workflow_fornax_traces(state, updated_at_ms, run_id);
