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

CREATE TABLE IF NOT EXISTS span_contracts (
    span_handle_id TEXT PRIMARY KEY REFERENCES spans(span_handle_id) ON DELETE CASCADE,
    protocol_version INTEGER NOT NULL,
    span_type TEXT NOT NULL,
    input_recorded INTEGER NOT NULL DEFAULT 0,
    output_recorded INTEGER NOT NULL DEFAULT 0,
    status_code INTEGER,
    error_recorded INTEGER NOT NULL DEFAULT 0,
    prompt_provider INTEGER NOT NULL DEFAULT 0,
    prompt_key INTEGER NOT NULL DEFAULT 0,
    prompt_version INTEGER NOT NULL DEFAULT 0,
    model_provider INTEGER NOT NULL DEFAULT 0,
    model_name INTEGER NOT NULL DEFAULT 0,
    tool_name INTEGER NOT NULL DEFAULT 0,
    agent_name INTEGER NOT NULL DEFAULT 0,
    agent_run_id INTEGER NOT NULL DEFAULT 0,
    retriever_provider INTEGER NOT NULL DEFAULT 0
);
