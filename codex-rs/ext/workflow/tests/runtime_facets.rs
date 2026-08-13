#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use codex_workflow_extension::ArtifactClassification;
use codex_workflow_extension::ArtifactId;
use codex_workflow_extension::DriveOutcome;
use codex_workflow_extension::EffectKey;
use codex_workflow_extension::NodeKey;
use codex_workflow_extension::NodeSpec;
use codex_workflow_extension::Workflow;
use codex_workflow_extension::WorkflowArguments;
use codex_workflow_extension::WorkflowArtifactClientWrite;
use codex_workflow_extension::WorkflowArtifactStore;
use codex_workflow_extension::WorkflowAuditKind;
use codex_workflow_extension::WorkflowAuditRecord;
use codex_workflow_extension::WorkflowContext;
use codex_workflow_extension::WorkflowDriver;
use codex_workflow_extension::WorkflowError;
use codex_workflow_extension::WorkflowMetadata;
use codex_workflow_extension::WorkflowName;
use codex_workflow_extension::WorkflowOutput;
use codex_workflow_extension::WorkflowRegistryBuilder;
use codex_workflow_extension::WorkflowRunId;
use codex_workflow_extension::WorkflowState;
use codex_workflow_extension::WorkflowTransition;
use codex_workflow_extension::WorkflowVersion;
use pretty_assertions::assert_eq;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

mod support;

#[derive(Clone, Debug, Parser, Serialize, Deserialize, JsonSchema)]
struct FacetArguments {}

impl WorkflowArguments for FacetArguments {
    fn validate(&self) -> Result<(), codex_workflow_extension::ArgumentError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FacetState {}

impl WorkflowState for FacetState {
    const SCHEMA_VERSION: u32 = 1;
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
struct FacetOutput {
    node_id: String,
    artifact_id: String,
}

impl WorkflowOutput for FacetOutput {}

struct FacetWorkflow;

impl Workflow for FacetWorkflow {
    type Arguments = FacetArguments;
    type State = FacetState;
    type Output = FacetOutput;

    fn metadata(&self) -> WorkflowMetadata {
        WorkflowMetadata::new(
            WorkflowName::new("runtime-facets").expect("workflow name"),
            WorkflowVersion::parse("1.0.0").expect("workflow version"),
            "Exercises safe run-scoped workflow runtime facets.",
        )
        .with_default(true)
    }

    fn initialize(&self, _args: Self::Arguments) -> Result<Self::State, WorkflowError> {
        Ok(FacetState {})
    }

    async fn step(
        &self,
        ctx: WorkflowContext<'_>,
        _state: Self::State,
    ) -> Result<WorkflowTransition<Self::State, Self::Output>, WorkflowError> {
        let nodes = ctx
            .nodes()
            .ok_or_else(|| WorkflowError::definition("nodes unavailable"))?;
        let spec = NodeSpec::builder(NodeKey::new("facet-node").expect("node key"))
            .build()
            .expect("node spec");
        let node = nodes
            .ensure(
                EffectKey::new("facet-node-ensure").expect("effect key"),
                spec.clone(),
            )
            .await
            .map_err(|error| WorkflowError::definition(error.to_string()))?;
        let replay = nodes
            .ensure(
                EffectKey::new("facet-node-ensure").expect("effect key"),
                spec,
            )
            .await
            .map_err(|error| WorkflowError::definition(error.to_string()))?;
        if node.id() != replay.id() {
            return Err(WorkflowError::definition("node replay changed identity"));
        }

        let artifacts = ctx
            .artifacts()
            .ok_or_else(|| WorkflowError::definition("artifacts unavailable"))?;
        let artifact_id = ArtifactId::new();
        let metadata = artifacts
            .write(WorkflowArtifactClientWrite {
                artifact_id,
                relative_path: &PathBuf::from("facets/result.txt"),
                classification: ArtifactClassification::Internal,
                media_type: "text/plain",
                bytes: b"facet result",
                created_at_ms: 120,
            })
            .await
            .map_err(|error| WorkflowError::definition(error.to_string()))?;
        let replayed = artifacts
            .write(WorkflowArtifactClientWrite {
                artifact_id,
                relative_path: &PathBuf::from("facets/result.txt"),
                classification: ArtifactClassification::Internal,
                media_type: "text/plain",
                bytes: b"facet result",
                created_at_ms: 121,
            })
            .await
            .map_err(|error| WorkflowError::definition(error.to_string()))?;
        if metadata.sha256 != replayed.sha256
            || artifacts
                .read(artifact_id)
                .await
                .map_err(|error| WorkflowError::definition(error.to_string()))?
                != b"facet result"
        {
            return Err(WorkflowError::definition("artifact replay mismatch"));
        }

        let audit = WorkflowAuditRecord {
            effect_key: EffectKey::new("facet-audit").expect("effect key"),
            kind: WorkflowAuditKind::StageCompleted,
            subject_id: Some("token=top-secret".to_string()),
            metadata: BTreeMap::from([(
                "diagnostic".to_string(),
                "authorization:top-secret".to_string(),
            )]),
        };
        ctx.audit()
            .append(audit.clone(), 130)
            .await
            .map_err(|error| WorkflowError::definition(error.to_string()))?;
        ctx.audit()
            .append(audit, 131)
            .await
            .map_err(|error| WorkflowError::definition(error.to_string()))?;
        if ctx.approvals().is_some() {
            return Err(WorkflowError::definition(
                "approval capability was unexpectedly installed",
            ));
        }
        Ok(WorkflowTransition::Complete {
            output: FacetOutput {
                node_id: node.id().to_string(),
                artifact_id: artifact_id.to_string(),
            },
        })
    }
}

#[tokio::test]
async fn context_facets_are_run_scoped_idempotent_and_redacted() {
    let (home, runtime) = support::runtime().await;
    let mut builder = WorkflowRegistryBuilder::new();
    builder.register(FacetWorkflow).expect("register workflow");
    let registry = Arc::new(builder.build().expect("build registry"));
    let host = Arc::new(support::TestHost::default());
    let service = support::service(&runtime, host.clone(), Arc::clone(&registry))
        .with_artifact_store(WorkflowArtifactStore::new(
            home.path(),
            runtime.workflows().clone(),
        ))
        .with_audit_secrets(vec!["top-secret".to_string()]);
    let run_id = WorkflowRunId::new();
    support::create_run(&runtime, run_id, "runtime-facets", "1.0.0", 1, json!({})).await;

    let driver = WorkflowDriver::new(runtime.workflows().clone(), registry, "facet-driver", 1_000)
        .with_runtime_facets(service);
    assert_eq!(
        DriveOutcome::Completed,
        driver.step_once(run_id, 200).await.expect("drive workflow")
    );
    assert_eq!(
        1,
        host.materializations
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert_eq!(
        1,
        runtime
            .workflows()
            .list_nodes(&run_id.to_string())
            .await
            .expect("nodes")
            .len()
    );
    assert_eq!(
        1,
        runtime
            .workflows()
            .list_artifacts(&run_id.to_string())
            .await
            .expect("artifacts")
            .len()
    );
    let audit = runtime
        .workflows()
        .read_effect(&run_id.to_string(), "facet-audit")
        .await
        .expect("read audit")
        .expect("audit effect");
    assert_eq!("audit.append", audit.kind);
    let serialized = serde_json::to_string(&audit.request).expect("serialize audit");
    assert!(!serialized.contains("top-secret"));
    assert!(serialized.contains("[REDACTED]"));
    runtime.close().await;
}
