use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use codex_state::WorkflowArtifactClassification;
use codex_state::WorkflowArtifactRecord;
use codex_state::WorkflowStore;
use sha2::Digest;
use sha2::Sha256;
use tokio::io::AsyncWriteExt;

use crate::ArtifactClassification;
use crate::ArtifactId;
use crate::ArtifactMetadata;
use crate::WorkflowRunId;

const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

/// Failure while validating, installing, or recording a workflow artifact.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowArtifactStoreError {
    /// Artifact paths must be non-empty, normalized, and relative.
    #[error("artifact path must be a normalized relative path")]
    InvalidPath,
    /// The artifact exceeded the configured local size limit.
    #[error("artifact contains {actual} bytes; maximum is {maximum}")]
    TooLarge {
        /// Submitted byte count.
        actual: u64,
        /// Configured byte limit.
        maximum: u64,
    },
    /// The artifact destination already exists.
    #[error("artifact path already exists")]
    AlreadyExists,
    /// The artifact manifest was absent or belonged to a different run.
    #[error("artifact was not found")]
    NotFound,
    /// Stored bytes no longer match the immutable manifest.
    #[error("artifact bytes do not match their manifest")]
    Corrupt,
    /// Local filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Durable manifest persistence failed.
    #[error(transparent)]
    State(#[from] codex_state::WorkflowStoreError),
}

/// Mode-restricted local artifact storage backed by durable manifests.
#[derive(Clone)]
pub struct WorkflowArtifactStore {
    root: PathBuf,
    state: WorkflowStore,
    max_artifact_bytes: u64,
}

/// Borrowed inputs for one atomic artifact write.
pub struct WorkflowArtifactWrite<'a> {
    /// Run that owns the artifact.
    pub run_id: WorkflowRunId,
    /// New immutable artifact identity.
    pub artifact_id: ArtifactId,
    /// Normalized path relative to the run artifact root.
    pub relative_path: &'a Path,
    /// Data-handling classification.
    pub classification: ArtifactClassification,
    /// Declared media type.
    pub media_type: &'a str,
    /// Complete artifact bytes.
    pub bytes: &'a [u8],
    /// Creation time in Unix epoch milliseconds.
    pub created_at_ms: i64,
}

/// Run-scoped immutable artifact facade exposed to reducer code.
#[derive(Clone)]
pub struct WorkflowArtifactClient {
    store: WorkflowArtifactStore,
    run_id: WorkflowRunId,
}

/// Borrowed inputs for one run-scoped artifact write.
pub struct WorkflowArtifactClientWrite<'a> {
    pub artifact_id: ArtifactId,
    pub relative_path: &'a Path,
    pub classification: ArtifactClassification,
    pub media_type: &'a str,
    pub bytes: &'a [u8],
    pub created_at_ms: i64,
}

impl WorkflowArtifactClient {
    pub(crate) fn new(store: WorkflowArtifactStore, run_id: WorkflowRunId) -> Self {
        Self { store, run_id }
    }

    /// Reads and verifies one immutable artifact owned by this run.
    pub async fn read(
        &self,
        artifact_id: ArtifactId,
    ) -> Result<Vec<u8>, WorkflowArtifactStoreError> {
        self.store.read(self.run_id, artifact_id).await
    }

    /// Writes one immutable artifact owned by this run.
    pub async fn write(
        &self,
        write: WorkflowArtifactClientWrite<'_>,
    ) -> Result<ArtifactMetadata, WorkflowArtifactStoreError> {
        self.store
            .write(WorkflowArtifactWrite {
                run_id: self.run_id,
                artifact_id: write.artifact_id,
                relative_path: write.relative_path,
                classification: write.classification,
                media_type: write.media_type,
                bytes: write.bytes,
                created_at_ms: write.created_at_ms,
            })
            .await
    }
}

impl WorkflowArtifactStore {
    /// Creates an artifact store rooted below the supplied Codex home.
    pub fn new(codex_home: &Path, state: WorkflowStore) -> Self {
        Self {
            root: codex_home.join("workflows").join("artifacts"),
            state,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
        }
    }

    /// Overrides the maximum accepted artifact size.
    pub fn with_max_artifact_bytes(mut self, maximum: u64) -> Self {
        self.max_artifact_bytes = maximum;
        self
    }

    /// Returns the local artifact root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads and verifies one immutable artifact owned by the supplied run.
    pub async fn read(
        &self,
        run_id: WorkflowRunId,
        artifact_id: ArtifactId,
    ) -> Result<Vec<u8>, WorkflowArtifactStoreError> {
        let manifest = self
            .state
            .read_artifact(&artifact_id.to_string())
            .await?
            .filter(|manifest| manifest.run_id == run_id.to_string())
            .ok_or(WorkflowArtifactStoreError::NotFound)?;
        let relative = validate_relative_path(Path::new(&manifest.relative_path))?;
        let path = self.root.join(run_id.to_string()).join(relative);
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(WorkflowArtifactStoreError::Corrupt);
        }
        let bytes = tokio::fs::read(path).await?;
        let byte_count =
            u64::try_from(bytes.len()).map_err(|_| WorkflowArtifactStoreError::Corrupt)?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        if byte_count != manifest.byte_count || sha256 != manifest.sha256 {
            return Err(WorkflowArtifactStoreError::Corrupt);
        }
        Ok(bytes)
    }

    /// Atomically installs bytes and records their immutable manifest.
    pub async fn write(
        &self,
        write: WorkflowArtifactWrite<'_>,
    ) -> Result<ArtifactMetadata, WorkflowArtifactStoreError> {
        let byte_count =
            u64::try_from(write.bytes.len()).map_err(|_| WorkflowArtifactStoreError::TooLarge {
                actual: u64::MAX,
                maximum: self.max_artifact_bytes,
            })?;
        if byte_count > self.max_artifact_bytes {
            return Err(WorkflowArtifactStoreError::TooLarge {
                actual: byte_count,
                maximum: self.max_artifact_bytes,
            });
        }
        let normalized = validate_relative_path(write.relative_path)?;
        let run_root = self.root.join(write.run_id.to_string());
        let destination = run_root.join(&normalized);
        let parent = destination
            .parent()
            .ok_or(WorkflowArtifactStoreError::InvalidPath)?;
        let workflow_root = self
            .root
            .parent()
            .ok_or(WorkflowArtifactStoreError::InvalidPath)?;
        create_private_directory(workflow_root).await?;
        create_private_directory(&self.root).await?;
        create_private_directory(&run_root).await?;
        create_private_relative_directories(
            &run_root,
            normalized.parent().unwrap_or_else(|| Path::new("")),
        )
        .await?;

        let artifact_id = write.artifact_id;
        let temporary = parent.join(format!(".artifact-{artifact_id}.tmp"));
        let install_result = self
            .write_and_install(&temporary, &destination, write.bytes)
            .await;
        if install_result.is_err() {
            remove_abandoned_temporary(&temporary).await;
        }
        install_result?;

        let sha256 = format!("{:x}", Sha256::digest(write.bytes));
        let relative_path = normalized
            .to_str()
            .ok_or(WorkflowArtifactStoreError::InvalidPath)?
            .to_string();
        let media_type = write.media_type.to_string();
        let metadata = ArtifactMetadata {
            artifact_id,
            run_id: write.run_id,
            relative_path: relative_path.clone(),
            classification: write.classification,
            media_type: media_type.clone(),
            byte_count,
            sha256: sha256.clone(),
            created_at_ms: write.created_at_ms,
        };
        let manifest = WorkflowArtifactRecord {
            artifact_id: artifact_id.to_string(),
            run_id: write.run_id.to_string(),
            relative_path,
            classification: write.classification.into(),
            media_type,
            byte_count,
            sha256,
            created_at_ms: write.created_at_ms,
        };
        if let Err(error) = self.state.record_artifact(&manifest).await {
            if matches!(error, codex_state::WorkflowStoreError::DuplicateArtifact)
                && let Some(existing) = self.state.read_artifact(&manifest.artifact_id).await?
                && artifact_manifests_match(&existing, &manifest)
            {
                return Ok(ArtifactMetadata {
                    created_at_ms: existing.created_at_ms,
                    ..metadata
                });
            }
            return Err(error.into());
        }
        Ok(metadata)
    }

    async fn write_and_install(
        &self,
        temporary: &Path,
        destination: &Path,
        bytes: &[u8],
    ) -> Result<(), WorkflowArtifactStoreError> {
        let mut options = tokio::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(temporary).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        match tokio::fs::hard_link(temporary, destination).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let installed = tokio::fs::read(destination).await?;
                if installed != bytes {
                    return Err(WorkflowArtifactStoreError::AlreadyExists);
                }
            }
            Err(error) => return Err(error.into()),
        }
        tokio::fs::remove_file(temporary).await?;
        Ok(())
    }
}

fn artifact_manifests_match(
    existing: &WorkflowArtifactRecord,
    replay: &WorkflowArtifactRecord,
) -> bool {
    existing.artifact_id == replay.artifact_id
        && existing.run_id == replay.run_id
        && existing.relative_path == replay.relative_path
        && existing.classification == replay.classification
        && existing.media_type == replay.media_type
        && existing.byte_count == replay.byte_count
        && existing.sha256 == replay.sha256
}

impl From<ArtifactClassification> for WorkflowArtifactClassification {
    fn from(value: ArtifactClassification) -> Self {
        match value {
            ArtifactClassification::Public => Self::Public,
            ArtifactClassification::Internal => Self::Internal,
            ArtifactClassification::Sensitive => Self::Sensitive,
        }
    }
}

fn validate_relative_path(path: &Path) -> Result<PathBuf, WorkflowArtifactStoreError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(WorkflowArtifactStoreError::InvalidPath);
    }
    Ok(path.to_path_buf())
}

async fn create_private_directory(path: &Path) -> Result<(), WorkflowArtifactStoreError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(WorkflowArtifactStoreError::InvalidPath);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match tokio::fs::create_dir(path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = tokio::fs::symlink_metadata(path).await?;
                    if !metadata.is_dir() || metadata.file_type().is_symlink() {
                        return Err(WorkflowArtifactStoreError::InvalidPath);
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(())
}

async fn create_private_relative_directories(
    root: &Path,
    relative: &Path,
) -> Result<(), WorkflowArtifactStoreError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(WorkflowArtifactStoreError::InvalidPath);
        };
        current.push(component);
        create_private_directory(&current).await?;
    }
    Ok(())
}

async fn remove_abandoned_temporary(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}
