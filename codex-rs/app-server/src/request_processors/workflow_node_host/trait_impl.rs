use super::*;

impl WorkflowNodeHost for AppServerWorkflowNodeHost {
    fn resolve_node_spec(
        &self,
        spec: codex_workflow_extension::NodeSpec,
    ) -> NodeHostFuture<'_, codex_workflow_extension::NodeSpec> {
        Box::pin(self.resolve_spec(spec))
    }

    fn materialize_node(
        &self,
        request: MaterializeNodeRequest,
    ) -> NodeHostFuture<'_, MaterializedNode> {
        Box::pin(self.materialize(request))
    }

    fn find_materialized_nodes(
        &self,
        binding: WorkflowNodeBinding,
    ) -> NodeHostFuture<'_, Vec<ThreadId>> {
        Box::pin(self.find_materialized(binding))
    }

    fn submit_prepared_turn(
        &self,
        request: PreparedTurnRequest,
    ) -> NodeHostFuture<'_, SubmittedTurn> {
        Box::pin(self.submit_turn(request))
    }

    fn await_terminal_turn(&self, request: AwaitTurnRequest) -> NodeHostFuture<'_, NodeTurnResult> {
        Box::pin(self.await_turn(request))
    }

    fn recover_prepared_turn(
        &self,
        request: RecoverTurnRequest,
    ) -> NodeHostFuture<'_, RecoveredTurnState> {
        Box::pin(self.recover_turn(request))
    }

    fn steer(&self, request: SteerTurnRequest) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            let thread = self
                .thread_manager
                .get_thread(request.thread_id)
                .await
                .map_err(|_| NodeHostError::ThreadNotFound)?;
            let metadata = request
                .input
                .responsesapi_client_metadata
                .map(|metadata| metadata.into_iter().collect());
            thread
                .steer_input(
                    request.input.items,
                    BTreeMap::new(),
                    Some(&request.turn_id),
                    None,
                    metadata,
                )
                .await
                .map_err(|error| NodeHostError::Host(format!("{error:?}")))?;
            Ok(())
        })
    }

    fn status(&self, thread_id: ThreadId) -> NodeHostFuture<'_, NodeRuntimeStatus> {
        Box::pin(async move {
            let Ok(thread) = self.thread_manager.get_thread(thread_id).await else {
                return Ok(NodeRuntimeStatus::Shutdown);
            };
            Ok(match thread.agent_status().await {
                AgentStatus::PendingInit | AgentStatus::Running => NodeRuntimeStatus::Running,
                AgentStatus::Completed(_) | AgentStatus::Errored(_) | AgentStatus::Interrupted => {
                    NodeRuntimeStatus::Idle
                }
                AgentStatus::Shutdown | AgentStatus::NotFound => NodeRuntimeStatus::Shutdown,
            })
        })
    }

    fn interrupt(&self, thread_id: ThreadId, _turn_id: String) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.thread_manager
                .get_thread(thread_id)
                .await
                .map_err(|_| NodeHostError::ThreadNotFound)?
                .submit(Op::Interrupt)
                .await
                .map_err(|error| NodeHostError::Host(error.to_string()))?;
            Ok(())
        })
    }

    fn cancel(&self, thread_id: ThreadId, _reason: CancellationReason) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.thread_manager
                .get_thread(thread_id)
                .await
                .map_err(|_| NodeHostError::ThreadNotFound)?
                .submit(Op::Interrupt)
                .await
                .map_err(|error| NodeHostError::Host(error.to_string()))?;
            Ok(())
        })
    }

    fn shutdown_runtime(
        &self,
        thread_id: ThreadId,
        mode: RuntimeShutdown,
    ) -> NodeHostFuture<'_, ()> {
        Box::pin(self.shutdown(thread_id, mode))
    }

    fn detach_observer(&self, thread_id: ThreadId) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            if self
                .listener_task_context
                .thread_state_manager
                .release_internal_observer(thread_id)
                .await
            {
                Ok(())
            } else {
                Err(NodeHostError::InvalidRequest(
                    "workflow node observer was not retained".to_string(),
                ))
            }
        })
    }

    fn archive(&self, thread_id: ThreadId, archived: bool) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.shutdown(thread_id, RuntimeShutdown::Graceful).await?;
            if archived {
                self.thread_store
                    .archive_thread(ArchiveThreadParams { thread_id })
                    .await
                    .map_err(|error| NodeHostError::Host(error.to_string()))
            } else {
                self.thread_store
                    .unarchive_thread(ArchiveThreadParams { thread_id })
                    .await
                    .map(|_| ())
                    .map_err(|error| NodeHostError::Host(error.to_string()))
            }
        })
    }

    fn delete(&self, confirmation: ConfirmedHistoryDeletion) -> NodeHostFuture<'_, ()> {
        Box::pin(async move {
            self.shutdown(confirmation.thread_id, RuntimeShutdown::Graceful)
                .await?;
            self.thread_store
                .delete_thread(DeleteThreadParams {
                    thread_id: confirmation.thread_id,
                })
                .await
                .map_err(|error| NodeHostError::Host(error.to_string()))
        })
    }
}
