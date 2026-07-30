use std::collections::HashMap;
use std::sync::Arc;

use codex_app_server_protocol::NodeThreadSubscription;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::WorkflowArtifactCreatedNotification;
use codex_app_server_protocol::WorkflowEvent;
use codex_app_server_protocol::WorkflowInteractionRequestedNotification;
use codex_app_server_protocol::WorkflowInteractionResolvedNotification;
use codex_app_server_protocol::WorkflowNodeStatus;
use codex_app_server_protocol::WorkflowNodeUpdatedNotification;
use codex_app_server_protocol::WorkflowRunStatus;
use codex_app_server_protocol::WorkflowRunUpdatedNotification;
use codex_state::WorkflowEventRecord;
use codex_state::WorkflowStore;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;

use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingMessageSender;

const EVENT_PAGE_SIZE: u32 = 1_000;

#[derive(Clone)]
pub(crate) struct WorkflowSubscriptions {
    store: WorkflowStore,
    outgoing: Arc<OutgoingMessageSender>,
    subscriptions: Arc<Mutex<HashMap<(ConnectionId, String), Subscription>>>,
    operation_gate: Arc<Semaphore>,
}

#[derive(Clone, Copy)]
struct Subscription {
    delivered_sequence: u64,
    node_threads: NodeThreadSubscription,
}

pub(crate) struct WorkflowSubscriptionReplay {
    pub(crate) events: Vec<WorkflowEvent>,
    pub(crate) snapshot_required: bool,
}

impl WorkflowSubscriptions {
    pub(crate) fn new(store: WorkflowStore, outgoing: Arc<OutgoingMessageSender>) -> Self {
        Self {
            store,
            outgoing,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            operation_gate: Arc::new(Semaphore::new(1)),
        }
    }

    pub(crate) async fn subscribe(
        &self,
        connection_id: ConnectionId,
        run_id: &str,
        after_sequence: u64,
        node_threads: NodeThreadSubscription,
    ) -> Result<WorkflowSubscriptionReplay, codex_state::WorkflowStoreError> {
        let _operation = self.operation_gate.acquire().await.ok();
        let run = self
            .store
            .read_run(run_id)
            .await?
            .ok_or(codex_state::WorkflowStoreError::RunNotFound)?;
        let latest_sequence = run.next_sequence.saturating_sub(1);
        let mut events = self
            .store
            .events_after(run_id, after_sequence, EVENT_PAGE_SIZE)
            .await?;
        let first_event_gap = events
            .first()
            .is_some_and(|event| event.sequence != after_sequence.saturating_add(1));
        let replay_truncated = events.len() == EVENT_PAGE_SIZE as usize
            && events
                .last()
                .is_some_and(|event| event.sequence < latest_sequence);
        let snapshot_required =
            after_sequence > latest_sequence || first_event_gap || replay_truncated;
        let delivered_sequence = if snapshot_required {
            events.clear();
            latest_sequence
        } else {
            events.last().map_or(after_sequence, |event| event.sequence)
        };
        self.subscriptions.lock().await.insert(
            (connection_id, run_id.to_string()),
            Subscription {
                delivered_sequence,
                node_threads,
            },
        );
        Ok(WorkflowSubscriptionReplay {
            events: events.into_iter().map(event_from_record).collect(),
            snapshot_required,
        })
    }

    pub(crate) async fn unsubscribe(&self, connection_id: ConnectionId, run_id: &str) -> bool {
        let _operation = self.operation_gate.acquire().await.ok();
        self.subscriptions
            .lock()
            .await
            .remove(&(connection_id, run_id.to_string()))
            .is_some()
    }

    pub(crate) async fn connection_closed(&self, connection_id: ConnectionId) {
        let _operation = self.operation_gate.acquire().await.ok();
        self.subscriptions
            .lock()
            .await
            .retain(|(candidate, _), _| *candidate != connection_id);
    }

    pub(crate) async fn connections_including_nodes(&self, run_id: &str) -> Vec<ConnectionId> {
        self.subscriptions
            .lock()
            .await
            .iter()
            .filter_map(|((connection_id, candidate), subscription)| {
                (candidate == run_id
                    && subscription.node_threads == NodeThreadSubscription::Include)
                    .then_some(*connection_id)
            })
            .collect()
    }

    pub(crate) async fn publish_run(
        &self,
        run_id: &str,
    ) -> Result<(), codex_state::WorkflowStoreError> {
        let _operation = self.operation_gate.acquire().await.ok();
        let subscribers = {
            let subscriptions = self.subscriptions.lock().await;
            subscriptions
                .iter()
                .filter(|((_, candidate), _)| candidate == run_id)
                .map(|((connection_id, _), subscription)| (*connection_id, *subscription))
                .collect::<Vec<_>>()
        };
        for (connection_id, subscription) in subscribers {
            let events = self
                .store
                .events_after(run_id, subscription.delivered_sequence, EVENT_PAGE_SIZE)
                .await?;
            for event in &events {
                let notification = self.notification_for_event(event).await?;
                self.outgoing
                    .send_server_notification_to_connections(&[connection_id], notification)
                    .await;
            }
            if let Some(sequence) = events.last().map(|event| event.sequence)
                && let Some(current) = self
                    .subscriptions
                    .lock()
                    .await
                    .get_mut(&(connection_id, run_id.to_string()))
            {
                current.delivered_sequence = current.delivered_sequence.max(sequence);
            }
        }
        Ok(())
    }

    async fn notification_for_event(
        &self,
        event: &WorkflowEventRecord,
    ) -> Result<ServerNotification, codex_state::WorkflowStoreError> {
        if event.kind.starts_with("node.") {
            let node = match event.entity_id.as_deref() {
                Some(node_id) => self.store.read_node(node_id).await?,
                None => None,
            };
            return Ok(ServerNotification::WorkflowNodeUpdated(
                WorkflowNodeUpdatedNotification {
                    run_id: event.run_id.clone(),
                    sequence: event.sequence,
                    created_at_ms: event.created_at_ms,
                    node_id: event.entity_id.clone().unwrap_or_default(),
                    thread_id: node.as_ref().and_then(|node| node.thread_id.clone()),
                    status: node.map(|node| node_status(node.status)),
                    metadata: event.metadata.clone(),
                },
            ));
        }
        if event.kind == "interaction.planned" {
            return Ok(ServerNotification::WorkflowInteractionRequested(
                WorkflowInteractionRequestedNotification {
                    run_id: event.run_id.clone(),
                    sequence: event.sequence,
                    created_at_ms: event.created_at_ms,
                    interaction_id: event.entity_id.clone().unwrap_or_default(),
                    metadata: event.metadata.clone(),
                },
            ));
        }
        if event.kind == "interaction.updated"
            && event
                .metadata
                .get("state")
                .and_then(serde_json::Value::as_str)
                == Some("resolved")
        {
            return Ok(ServerNotification::WorkflowInteractionResolved(
                WorkflowInteractionResolvedNotification {
                    run_id: event.run_id.clone(),
                    sequence: event.sequence,
                    created_at_ms: event.created_at_ms,
                    interaction_id: event.entity_id.clone().unwrap_or_default(),
                    metadata: event.metadata.clone(),
                },
            ));
        }
        if event.kind == "artifact.created" {
            return Ok(ServerNotification::WorkflowArtifactCreated(
                WorkflowArtifactCreatedNotification {
                    run_id: event.run_id.clone(),
                    sequence: event.sequence,
                    created_at_ms: event.created_at_ms,
                    artifact_id: event.entity_id.clone().unwrap_or_default(),
                    metadata: event.metadata.clone(),
                },
            ));
        }
        let status = match run_status_for_event(&event.kind) {
            Some(status) => status,
            None => {
                let run = self
                    .store
                    .read_run(&event.run_id)
                    .await?
                    .ok_or(codex_state::WorkflowStoreError::RunNotFound)?;
                run_status(run.status)
            }
        };
        Ok(ServerNotification::WorkflowRunUpdated(
            WorkflowRunUpdatedNotification {
                run_id: event.run_id.clone(),
                sequence: event.sequence,
                created_at_ms: event.created_at_ms,
                status,
                metadata: event.metadata.clone(),
            },
        ))
    }
}

fn event_from_record(event: WorkflowEventRecord) -> WorkflowEvent {
    WorkflowEvent {
        run_id: event.run_id,
        sequence: event.sequence,
        kind: event.kind,
        entity_id: event.entity_id,
        metadata: event.metadata,
        created_at_ms: event.created_at_ms,
    }
}

fn run_status(status: codex_state::WorkflowRunStatus) -> WorkflowRunStatus {
    match status {
        codex_state::WorkflowRunStatus::Pending => WorkflowRunStatus::Pending,
        codex_state::WorkflowRunStatus::Running => WorkflowRunStatus::Running,
        codex_state::WorkflowRunStatus::Waiting => WorkflowRunStatus::Waiting,
        codex_state::WorkflowRunStatus::Cancelling => WorkflowRunStatus::Cancelling,
        codex_state::WorkflowRunStatus::Succeeded => WorkflowRunStatus::Succeeded,
        codex_state::WorkflowRunStatus::Failed => WorkflowRunStatus::Failed,
        codex_state::WorkflowRunStatus::Cancelled => WorkflowRunStatus::Cancelled,
        codex_state::WorkflowRunStatus::NeedsOperator => WorkflowRunStatus::NeedsOperator,
    }
}

fn run_status_for_event(kind: &str) -> Option<WorkflowRunStatus> {
    match kind {
        "run.created" | "run.resumed" => Some(WorkflowRunStatus::Pending),
        "run.continued" | "run.started" => Some(WorkflowRunStatus::Running),
        "run.waiting" => Some(WorkflowRunStatus::Waiting),
        "run.cancellation_requested" => Some(WorkflowRunStatus::Cancelling),
        "run.succeeded" => Some(WorkflowRunStatus::Succeeded),
        "run.failed" => Some(WorkflowRunStatus::Failed),
        "run.cancelled" => Some(WorkflowRunStatus::Cancelled),
        "run.needs_operator" => Some(WorkflowRunStatus::NeedsOperator),
        _ => None,
    }
}

fn node_status(status: codex_state::WorkflowNodeStatus) -> WorkflowNodeStatus {
    match status {
        codex_state::WorkflowNodeStatus::Pending => WorkflowNodeStatus::Pending,
        codex_state::WorkflowNodeStatus::Ready => WorkflowNodeStatus::Ready,
        codex_state::WorkflowNodeStatus::Running => WorkflowNodeStatus::Running,
        codex_state::WorkflowNodeStatus::Waiting => WorkflowNodeStatus::Waiting,
        codex_state::WorkflowNodeStatus::Succeeded => WorkflowNodeStatus::Succeeded,
        codex_state::WorkflowNodeStatus::Failed => WorkflowNodeStatus::Failed,
        codex_state::WorkflowNodeStatus::Cancelled => WorkflowNodeStatus::Cancelled,
        codex_state::WorkflowNodeStatus::Blocked => WorkflowNodeStatus::Blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_transport::OutgoingMessage;
    use codex_state::StateRuntime;
    use codex_state::WorkflowRunCreate;
    use serde_json::json;
    use tempfile::tempdir;
    use tokio::sync::mpsc;

    use crate::outgoing_message::OutgoingEnvelope;

    #[tokio::test]
    async fn workflow_subscription_detects_invalid_cursor_and_publishes_each_sequence_once() {
        let home = tempdir().expect("create temporary codex home");
        let runtime = StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string())
            .await
            .expect("initialize state runtime");
        let store = runtime.workflows().clone();
        store
            .create_run(WorkflowRunCreate {
                run_id: "run-subscription".to_string(),
                definition_name: "test-workflow".to_string(),
                definition_version: "1.0.0".to_string(),
                state_schema_version: 1,
                state: json!({}),
                arguments: json!({}),
                created_at_ms: 100,
            })
            .await
            .expect("create workflow run");
        let (tx, mut rx) = mpsc::channel(8);
        let subscriptions = WorkflowSubscriptions::new(
            store.clone(),
            Arc::new(OutgoingMessageSender::new(
                tx,
                codex_analytics::AnalyticsEventsClient::disabled(),
            )),
        );
        let connection_id = ConnectionId(7);

        let invalid_cursor = subscriptions
            .subscribe(
                connection_id,
                "run-subscription",
                99,
                NodeThreadSubscription::ReferencesOnly,
            )
            .await
            .expect("subscribe with invalid future cursor");
        assert!(invalid_cursor.snapshot_required);
        assert!(invalid_cursor.events.is_empty());

        store
            .mark_run_needs_operator("run-subscription", None, "manual_review", json!({}), 101)
            .await
            .expect("append event after invalid cursor");
        subscriptions
            .publish_run("run-subscription")
            .await
            .expect("publish after invalid cursor");
        let invalid_cursor_envelope = rx.recv().await.expect("receive event after cursor reset");
        let OutgoingEnvelope::ToConnection {
            message:
                OutgoingMessage::AppServerNotification(ServerNotification::WorkflowRunUpdated(
                    invalid_cursor_notification,
                )),
            ..
        } = invalid_cursor_envelope
        else {
            panic!("expected workflow run notification after cursor reset");
        };
        assert_eq!(invalid_cursor_notification.sequence, 2);
        assert_eq!(
            invalid_cursor_notification.status,
            WorkflowRunStatus::NeedsOperator
        );

        let replay = subscriptions
            .subscribe(
                connection_id,
                "run-subscription",
                2,
                NodeThreadSubscription::Include,
            )
            .await
            .expect("subscribe at latest sequence");
        assert!(!replay.snapshot_required);
        assert!(replay.events.is_empty());
        assert_eq!(
            subscriptions
                .connections_including_nodes("run-subscription")
                .await,
            vec![connection_id]
        );

        store
            .resume_run("run-subscription", 102)
            .await
            .expect("append workflow event");
        let (first, second) = tokio::join!(
            subscriptions.publish_run("run-subscription"),
            subscriptions.publish_run("run-subscription")
        );
        first.expect("first publication");
        second.expect("second publication");

        let envelope = rx.recv().await.expect("receive workflow notification");
        let OutgoingEnvelope::ToConnection {
            connection_id: actual_connection_id,
            message:
                OutgoingMessage::AppServerNotification(ServerNotification::WorkflowRunUpdated(
                    notification,
                )),
            ..
        } = envelope
        else {
            panic!("expected targeted workflow run notification");
        };
        assert_eq!(actual_connection_id, connection_id);
        assert_eq!(notification.run_id, "run-subscription");
        assert_eq!(notification.sequence, 3);
        assert_eq!(notification.status, WorkflowRunStatus::Pending);
        assert!(rx.try_recv().is_err());

        runtime.close().await;
    }
}
