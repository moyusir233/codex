CREATE TABLE workflow_lark_interactions (
    interaction_id TEXT PRIMARY KEY NOT NULL
        REFERENCES workflow_interactions(interaction_id) ON DELETE CASCADE,
    run_id TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
    effect_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    thread_id TEXT,
    request_message_id TEXT,
    correlation_token TEXT NOT NULL,
    allowed_senders_json TEXT NOT NULL,
    watermark_ms INTEGER NOT NULL,
    poll_page_token TEXT,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE(run_id, effect_key),
    UNIQUE(correlation_token)
);

CREATE INDEX workflow_lark_interactions_message_idx
    ON workflow_lark_interactions(chat_id, request_message_id, interaction_id);
