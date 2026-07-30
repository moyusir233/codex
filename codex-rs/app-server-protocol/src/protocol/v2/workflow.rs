use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

/// Arguments supplied to one registered workflow definition.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
#[ts(rename_all = "camelCase", tag = "kind", export_to = "v2/")]
pub enum WorkflowArgumentsInput {
    /// Parse workflow-specific command-line arguments.
    Argv { argv: Vec<String> },
    /// Validate an already structured JSON argument value.
    Json {
        #[ts(type = "unknown")]
        value: Value,
    },
}

/// Stability classification published by a registered workflow.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowDefinitionStability {
    Experimental,
    Stable,
}

/// Discovery metadata for one exact registered workflow version.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowDefinitionSummary {
    pub name: String,
    pub version: String,
    pub description: String,
    pub is_default: bool,
    pub stability: WorkflowDefinitionStability,
    pub state_schema_version: u32,
    pub argv_help: String,
    #[ts(type = "unknown")]
    pub argument_schema: Value,
    #[ts(type = "unknown")]
    pub output_schema: Value,
    pub required_capabilities: Vec<String>,
}

/// Durable workflow run lifecycle state.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Waiting,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    NeedsOperator,
}

/// Durable workflow node lifecycle state.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowNodeStatus {
    Pending,
    Ready,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

/// One node included in a workflow run snapshot.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowNodeSummary {
    pub node_id: String,
    pub node_key: String,
    #[ts(type = "string | null")]
    pub thread_id: Option<String>,
    /// Every persisted thread used by this node's attempts, in attempt order.
    pub thread_ids: Vec<String>,
    pub status: WorkflowNodeStatus,
    #[ts(type = "bigint | null")]
    pub retry_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Durable artifact manifest safe to expose through workflow run snapshots.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowArtifactSummary {
    pub artifact_id: String,
    pub relative_path: String,
    pub classification: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub created_at_ms: i64,
}

/// Durable run snapshot returned by workflow requests.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRun {
    pub run_id: String,
    pub workflow_name: String,
    pub workflow_version: String,
    pub status: WorkflowRunStatus,
    #[ts(type = "unknown")]
    pub arguments: Value,
    pub non_interactive: bool,
    pub detached: bool,
    #[ts(type = "number | null")]
    pub concurrency: Option<u32>,
    #[ts(type = "unknown")]
    pub output: Option<Value>,
    #[ts(type = "string | null")]
    pub error_code: Option<String>,
    #[ts(type = "unknown")]
    pub wake: Option<Value>,
    pub next_sequence: u64,
    pub nodes: Vec<WorkflowNodeSummary>,
    pub artifacts: Vec<WorkflowArtifactSummary>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Redacted, monotonically sequenced workflow orchestration event.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowEvent {
    pub run_id: String,
    pub sequence: u64,
    pub kind: String,
    #[ts(type = "string | null")]
    pub entity_id: Option<String>,
    #[ts(type = "unknown")]
    pub metadata: Value,
    pub created_at_ms: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowListParams {}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowListResponse {
    pub data: Vec<WorkflowDefinitionSummary>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunParams {
    pub workflow_name: String,
    #[ts(optional = nullable)]
    pub workflow_version: Option<String>,
    pub arguments: WorkflowArgumentsInput,
    /// Reject rather than auto-approve interactive node requests.
    #[serde(default)]
    pub non_interactive: bool,
    /// The caller will disconnect after the run is accepted.
    #[serde(default)]
    pub detached: bool,
    /// Optional caller-requested node concurrency cap.
    #[ts(optional = nullable)]
    pub concurrency: Option<u32>,
    #[serde(default)]
    pub subscribe: bool,
    #[serde(default)]
    pub node_threads: NodeThreadSubscription,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunResponse {
    pub run: WorkflowRun,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunListParams {
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunListResponse {
    pub data: Vec<WorkflowRun>,
    #[ts(optional = nullable)]
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunReadParams {
    pub run_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunReadResponse {
    pub run: WorkflowRun,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunResumeParams {
    pub run_id: String,
    /// The resuming client will disconnect after acceptance.
    #[serde(default)]
    pub detached: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunResumeResponse {
    pub run: WorkflowRun,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunCancelParams {
    pub run_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunCancelResponse {
    pub run: WorkflowRun,
}

/// Whether workflow subscribers also receive normal node thread events.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum NodeThreadSubscription {
    Include,
    #[default]
    ReferencesOnly,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunSubscribeParams {
    pub run_id: String,
    #[ts(optional = nullable)]
    pub after_sequence: Option<u64>,
    #[serde(default)]
    pub node_threads: NodeThreadSubscription,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunSubscribeResponse {
    pub run: WorkflowRun,
    pub events: Vec<WorkflowEvent>,
    pub snapshot_required: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunUnsubscribeParams {
    pub run_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowRunUnsubscribeResponse {
    pub removed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowInteractionRespondParams {
    pub interaction_id: String,
    pub idempotency_key: String,
    #[ts(type = "unknown")]
    pub response: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WorkflowInteractionRespondResponse {
    pub run: WorkflowRun,
    pub accepted: bool,
}

macro_rules! workflow_notification {
    ($name:ident { $($field:ident : $type:ty),* $(,)? }) => {
        #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "v2/")]
        pub struct $name {
            pub run_id: String,
            pub sequence: u64,
            pub created_at_ms: i64,
            $(pub $field: $type,)*
            #[ts(type = "unknown")]
            pub metadata: Value,
        }
    };
}

workflow_notification!(WorkflowRunUpdatedNotification {
    status: WorkflowRunStatus,
});
workflow_notification!(WorkflowNodeUpdatedNotification {
    node_id: String,
    thread_id: Option<String>,
    status: Option<WorkflowNodeStatus>,
});
workflow_notification!(WorkflowInteractionRequestedNotification {
    interaction_id: String,
});
workflow_notification!(WorkflowInteractionResolvedNotification {
    interaction_id: String,
});
workflow_notification!(WorkflowArtifactCreatedNotification {
    artifact_id: String,
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientRequest;
    use crate::ClientRequestSerializationScope;
    use crate::ServerNotification;
    use crate::experimental_api::ExperimentalApi;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn workflow_arguments_are_an_explicit_tagged_union() {
        let argv = serde_json::from_value::<WorkflowArgumentsInput>(json!({
            "kind": "argv",
            "argv": ["--topic", "release"]
        }))
        .expect("deserialize argv arguments");
        assert_eq!(
            argv,
            WorkflowArgumentsInput::Argv {
                argv: vec!["--topic".to_string(), "release".to_string()]
            }
        );

        let structured = serde_json::from_value::<WorkflowArgumentsInput>(json!({
            "kind": "json",
            "value": {"topic": "release"}
        }))
        .expect("deserialize structured arguments");
        assert_eq!(
            structured,
            WorkflowArgumentsInput::Json {
                value: json!({"topic": "release"})
            }
        );

        assert!(
            serde_json::from_value::<WorkflowArgumentsInput>(json!({
                "argv": [],
                "value": {}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<WorkflowArgumentsInput>(json!({
                "kind": "argv",
                "argv": [],
                "value": {}
            }))
            .is_err()
        );
    }

    #[test]
    fn workflow_request_wire_shape_scope_and_experimental_marker_are_stable() {
        let value = json!({
            "method": "workflowRun/subscribe",
            "id": 7,
            "params": {
                "runId": "wfr_01",
                "afterSequence": 12,
                "nodeThreads": "include"
            }
        });
        let request =
            serde_json::from_value::<ClientRequest>(value.clone()).expect("deserialize request");

        assert_eq!(
            serde_json::to_value(&request).expect("serialize request"),
            value
        );
        assert_eq!(
            request.serialization_scope(),
            Some(ClientRequestSerializationScope::WorkflowRun {
                run_id: "wfr_01".to_string()
            })
        );
        assert_eq!(
            ExperimentalApi::experimental_reason(&request),
            Some("workflowRun/subscribe")
        );
    }

    #[test]
    fn workflow_run_request_keeps_runner_policy_out_of_definition_argv() {
        let request = ClientRequest::WorkflowRun {
            request_id: crate::RequestId::Integer(8),
            params: WorkflowRunParams {
                workflow_name: "release".to_string(),
                workflow_version: Some("1.2.3".to_string()),
                arguments: WorkflowArgumentsInput::Argv {
                    argv: vec!["--topic".to_string(), "launch".to_string()],
                },
                non_interactive: true,
                detached: true,
                concurrency: Some(3),
                subscribe: false,
                node_threads: NodeThreadSubscription::ReferencesOnly,
            },
        };
        assert_eq!(
            serde_json::to_value(request).expect("serialize workflow run"),
            json!({
                "method": "workflow/run",
                "id": 8,
                "params": {
                    "workflowName": "release",
                    "workflowVersion": "1.2.3",
                    "arguments": {
                        "kind": "argv",
                        "argv": ["--topic", "launch"]
                    },
                    "nonInteractive": true,
                    "detached": true,
                    "concurrency": 3,
                    "subscribe": false,
                    "nodeThreads": "referencesOnly"
                }
            })
        );
    }

    #[test]
    fn workflow_notification_wire_shape_and_experimental_marker_are_stable() {
        let notification =
            ServerNotification::WorkflowNodeUpdated(WorkflowNodeUpdatedNotification {
                run_id: "wfr_01".to_string(),
                sequence: 13,
                created_at_ms: 42,
                node_id: "wfn_01".to_string(),
                thread_id: Some("thr_01".to_string()),
                status: Some(WorkflowNodeStatus::Running),
                metadata: json!({"attempt": 1}),
            });

        assert_eq!(
            serde_json::to_value(&notification).expect("serialize notification"),
            json!({
                "method": "workflowNode/updated",
                "params": {
                    "runId": "wfr_01",
                    "sequence": 13,
                    "createdAtMs": 42,
                    "nodeId": "wfn_01",
                    "threadId": "thr_01",
                    "status": "running",
                    "metadata": {"attempt": 1}
                }
            })
        );
        assert_eq!(
            ExperimentalApi::experimental_reason(&notification),
            Some("workflowNode/updated")
        );
    }
}
