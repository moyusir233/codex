PRAGMA journal_mode=WAL;
PRAGMA synchronous=FULL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS bridge_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL,
    protocol_version TEXT NOT NULL,
    current_instance_id TEXT NOT NULL,
    clean_shutdown INTEGER NOT NULL,
    sdk_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS operations (
    operation_id TEXT PRIMARY KEY,
    route TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    state TEXT NOT NULL,
    response_json TEXT,
    error_code TEXT,
    instance_id TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS trace_contexts (
    trace_context_id TEXT PRIMARY KEY,
    serialized_header_json TEXT NOT NULL,
    baggage_hash TEXT,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS spans (
    span_handle_id TEXT PRIMARY KEY,
    trace_context_id TEXT NOT NULL UNIQUE
        REFERENCES trace_contexts(trace_context_id),
    trace_id TEXT NOT NULL,
    span_id TEXT NOT NULL,
    w3c TEXT NOT NULL,
    creating_operation_id TEXT NOT NULL UNIQUE
        REFERENCES operations(operation_id),
    instance_id TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    finished_at_ms INTEGER
);
