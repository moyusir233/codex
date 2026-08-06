use super::*;

impl AppServerWorkflowNodeHost {
    pub(crate) async fn rehydrate_nonterminal_threads(
        &self,
        now_ms: i64,
    ) -> Result<usize, NodeHostError> {
        let mut loaded = 0usize;
        let runs = self
            .workflow_store
            .list_recoverable_runs(now_ms)
            .await
            .map_err(|error| NodeHostError::Host(error.to_string()))?;
        for run in runs {
            for node in self
                .workflow_store
                .list_nodes(&run.run_id)
                .await
                .map_err(|error| NodeHostError::Host(error.to_string()))?
            {
                let spec = serde_json::from_value(node.spec)
                    .map_err(|error| NodeHostError::Host(error.to_string()))?;
                let mut config = self.config.as_ref().clone();
                apply_node_spec(&mut config, &spec)?;
                let host_skills = self.host_skills_snapshot(&config).await;
                apply_host_skill_restrictions(&mut config, &spec, &host_skills)?;
                config.ephemeral = false;
                for record in self
                    .workflow_store
                    .list_node_threads(&node.node_id)
                    .await
                    .map_err(|error| NodeHostError::Host(error.to_string()))?
                {
                    let thread_id = ThreadId::from_string(&record.thread_id)
                        .map_err(|error| NodeHostError::Host(error.to_string()))?;
                    if self.thread_manager.get_thread(thread_id).await.is_ok() {
                        continue;
                    }
                    let stored = self
                        .thread_store
                        .read_thread(ReadThreadParams {
                            thread_id,
                            include_archived: true,
                            include_history: true,
                        })
                        .await
                        .map_err(|error| NodeHostError::Host(error.to_string()))?;
                    let history = stored.history.ok_or_else(|| {
                        NodeHostError::Host("persisted workflow thread has no history".to_string())
                    })?;
                    let resumed = self
                        .thread_manager
                        .resume_thread_with_history(
                            config.clone(),
                            InitialHistory::Resumed(ResumedHistory {
                                conversation_id: thread_id,
                                history: Arc::new(history.items),
                                rollout_path: stored.rollout_path,
                            }),
                            self.thread_manager.auth_manager(),
                            None,
                            false,
                        )
                        .await
                        .map_err(|error| NodeHostError::Host(error.to_string()))?;
                    self.listener_task_context
                        .thread_state_manager
                        .retain_internal_observer(thread_id)
                        .await;
                    self.listener_task_context
                        .thread_watch_manager
                        .note_thread_loaded(&thread_id.to_string())
                        .await;
                    let thread_state = self
                        .listener_task_context
                        .thread_state_manager
                        .thread_state(thread_id)
                        .await;
                    ensure_listener_task_running(
                        self.listener_task_context.clone(),
                        thread_id,
                        resumed.thread,
                        thread_state,
                    )
                    .await
                    .map_err(|error| NodeHostError::Host(error.message))?;
                    if let Some(subscriptions) = &self.workflow_subscriptions {
                        for connection_id in
                            subscriptions.connections_including_nodes(&run.run_id).await
                        {
                            self.listener_task_context
                                .thread_state_manager
                                .try_ensure_connection_subscribed(thread_id, connection_id, false)
                                .await;
                        }
                    }
                    loaded += 1;
                }
            }
        }
        Ok(loaded)
    }

    pub(super) async fn find_materialized(
        &self,
        binding: WorkflowNodeBinding,
    ) -> Result<Vec<ThreadId>, NodeHostError> {
        let expected = ThreadSource::Feature(binding.thread_source());
        let mut matches = Vec::new();
        for archived in [false, true] {
            let mut cursor = None;
            loop {
                let page = self
                    .thread_store
                    .list_threads(ListThreadsParams {
                        page_size: 100,
                        cursor,
                        sort_key: ThreadSortKey::CreatedAt,
                        sort_direction: SortDirection::Asc,
                        allowed_sources: Vec::new(),
                        model_providers: Some(Vec::new()),
                        cwd_filters: None,
                        archived,
                        search_term: None,
                        relation_filter: None,
                        use_state_db_only: false,
                    })
                    .await
                    .map_err(|error| NodeHostError::Host(error.to_string()))?;
                matches.extend(
                    page.items
                        .into_iter()
                        .filter(|thread| thread.thread_source.as_ref() == Some(&expected))
                        .map(|thread| thread.thread_id),
                );
                let Some(next) = page.next_cursor else {
                    break;
                };
                cursor = Some(next);
            }
        }
        Ok(matches)
    }
}
