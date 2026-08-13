CREATE TABLE workflow_fornax_delivery_proofs (
    run_id TEXT NOT NULL,
    effect_key TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    remote_trace_id TEXT NOT NULL,
    remote_span_id TEXT NOT NULL,
    proof_kind TEXT NOT NULL,
    proof_digest TEXT NOT NULL,
    reconciled_at_ms INTEGER NOT NULL,
    PRIMARY KEY(run_id, effect_key),
    FOREIGN KEY(run_id, effect_key)
        REFERENCES workflow_fornax_traces(run_id, effect_key) ON DELETE CASCADE
);

CREATE INDEX workflow_fornax_delivery_proofs_operation_idx
    ON workflow_fornax_delivery_proofs(operation_id, request_hash);
