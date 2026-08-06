use std::path::Path;

use codex_state::StateRuntime;
use codex_state::WorkflowRunCreate;
use codex_workflow_extension::ArtifactClassification;
use codex_workflow_extension::ArtifactId;
use codex_workflow_extension::WorkflowArtifactStore;
use codex_workflow_extension::WorkflowArtifactStoreError;
use codex_workflow_extension::WorkflowArtifactWrite;
use codex_workflow_extension::WorkflowRunId;
use pretty_assertions::assert_eq;
use serde_json::json;

#[allow(clippy::expect_used)]
async fn setup() -> (tempfile::TempDir, std::sync::Arc<StateRuntime>) {
    let home = tempfile::tempdir().expect("create temporary Codex home");
    let runtime = StateRuntime::init(home.path().to_path_buf(), "test-provider".to_string())
        .await
        .expect("initialize state runtime");
    (home, runtime)
}

#[tokio::test]
async fn store_writes_private_atomic_artifact_and_manifest() {
    let (home, runtime) = setup().await;
    let run_id = WorkflowRunId::new();
    runtime
        .workflows()
        .create_run(WorkflowRunCreate {
            run_id: run_id.to_string(),
            definition_name: "test".to_string(),
            definition_version: "1.0.0".to_string(),
            state_schema_version: 1,
            state: json!({}),
            arguments: json!({}),
            non_interactive: false,
            detached: false,
            concurrency: None,
            created_at_ms: 10,
        })
        .await
        .expect("create run");
    let store = WorkflowArtifactStore::new(home.path(), runtime.workflows().clone());
    let metadata = store
        .write(WorkflowArtifactWrite {
            run_id,
            artifact_id: ArtifactId::new(),
            relative_path: Path::new("reports/final.json"),
            classification: ArtifactClassification::Sensitive,
            media_type: "application/json",
            bytes: br#"{"ok":true}"#,
            created_at_ms: 12,
        })
        .await
        .expect("write artifact");
    let replay = store
        .write(WorkflowArtifactWrite {
            run_id,
            artifact_id: metadata.artifact_id,
            relative_path: Path::new("reports/final.json"),
            classification: ArtifactClassification::Sensitive,
            media_type: "application/json",
            bytes: br#"{"ok":true}"#,
            created_at_ms: 11,
        })
        .await
        .expect("replay identical artifact write");

    assert_eq!(metadata.byte_count, 11);
    assert_eq!(metadata, replay);
    let manifest = runtime
        .workflows()
        .read_artifact(&metadata.artifact_id.to_string())
        .await
        .expect("read artifact manifest")
        .expect("artifact manifest exists");
    assert_eq!(manifest.sha256, metadata.sha256);
    assert_eq!(manifest.byte_count, metadata.byte_count);
    let manifests = runtime
        .workflows()
        .list_artifacts(&run_id.to_string())
        .await
        .expect("list artifact manifests");
    assert_eq!(manifests, vec![manifest]);
    assert_eq!(
        tokio::fs::read(
            store
                .root()
                .join(run_id.to_string())
                .join("reports/final.json")
        )
        .await
        .expect("read stored artifact"),
        br#"{"ok":true}"#
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let root_mode = std::fs::metadata(store.root())
            .expect("artifact root metadata")
            .permissions()
            .mode()
            & 0o777;
        let workflow_mode = std::fs::metadata(home.path().join("workflows"))
            .expect("workflow root metadata")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = std::fs::metadata(
            store
                .root()
                .join(run_id.to_string())
                .join("reports/final.json"),
        )
        .expect("artifact file metadata")
        .permissions()
        .mode()
            & 0o777;
        assert_eq!(workflow_mode, 0o700);
        assert_eq!(root_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
    let events = runtime
        .workflows()
        .events_after(&run_id.to_string(), 0, 10)
        .await
        .expect("read artifact event");
    assert_eq!(
        events.last().expect("artifact event").kind,
        "artifact.created"
    );
    runtime.close().await;
}

#[tokio::test]
async fn store_rejects_traversal_without_creating_files() {
    let (home, runtime) = setup().await;
    let store = WorkflowArtifactStore::new(home.path(), runtime.workflows().clone());
    let error = store
        .write(WorkflowArtifactWrite {
            run_id: WorkflowRunId::new(),
            artifact_id: ArtifactId::new(),
            relative_path: Path::new("../escape"),
            classification: ArtifactClassification::Internal,
            media_type: "text/plain",
            bytes: b"escape",
            created_at_ms: 10,
        })
        .await
        .expect_err("traversal must fail");
    assert!(matches!(error, WorkflowArtifactStoreError::InvalidPath));
    assert!(!home.path().join("workflows").exists());
    runtime.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn store_rejects_symlink_parent_without_writing_outside_root() {
    use std::os::unix::fs::symlink;

    let (home, runtime) = setup().await;
    let run_id = WorkflowRunId::new();
    let store = WorkflowArtifactStore::new(home.path(), runtime.workflows().clone());
    let run_root = store.root().join(run_id.to_string());
    tokio::fs::create_dir_all(&run_root)
        .await
        .expect("create run artifact root");
    let outside = tempfile::tempdir().expect("create outside directory");
    symlink(outside.path(), run_root.join("escape")).expect("create escape symlink");

    let error = store
        .write(WorkflowArtifactWrite {
            run_id,
            artifact_id: ArtifactId::new(),
            relative_path: Path::new("escape/written.txt"),
            classification: ArtifactClassification::Internal,
            media_type: "text/plain",
            bytes: b"escape",
            created_at_ms: 10,
        })
        .await
        .expect_err("symlink parent must fail");
    assert!(matches!(error, WorkflowArtifactStoreError::InvalidPath));
    assert!(!outside.path().join("written.txt").exists());
    runtime.close().await;
}
