CREATE UNIQUE INDEX workflow_nodes_thread_id_unique_idx
    ON workflow_nodes(thread_id)
    WHERE thread_id IS NOT NULL;
