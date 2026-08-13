use std::collections::BTreeMap;

use codex_state::WorkflowFornaxDeliveryProofPlan;
use codex_state::WorkflowFornaxDeliveryProofRecord;
use codex_state::WorkflowFornaxTraceRecord;
use codex_state::WorkflowFornaxTraceState;
use codex_state::WorkflowStore;

/// What the workflow can truthfully claim about a locally journaled trace mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FornaxDeliveryState {
    AcceptedLocal,
    DeliveredRemote,
    DeliveryFailedRetryable,
    DeliveryAmbiguous,
    DeliveryFailedPermanent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FornaxDeliveryReport {
    pub effect_key: String,
    pub operation_id: String,
    pub state: FornaxDeliveryState,
    pub error_code: Option<String>,
    pub remote_trace_id: Option<String>,
    pub remote_span_id: Option<String>,
    pub proof_digest: Option<String>,
}

/// Read-only view over the durable Fornax delivery backlog.
#[derive(Clone)]
pub struct FornaxDeliveryReporter {
    store: WorkflowStore,
    run_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FornaxDeliveryError {
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error("invalid Fornax delivery proof field `{0}`")]
    InvalidProof(&'static str),
    #[error("Fornax delivery correlation was not found")]
    CorrelationNotFound,
}

impl FornaxDeliveryReporter {
    pub fn new(store: WorkflowStore, run_id: impl Into<String>) -> Self {
        Self {
            store,
            run_id: run_id.into(),
        }
    }

    pub async fn backlog(
        &self,
    ) -> Result<Vec<FornaxDeliveryReport>, codex_state::WorkflowStoreError> {
        let records = self.store.list_fornax_traces(&self.run_id).await?;
        let proofs = self
            .store
            .list_fornax_delivery_proofs(&self.run_id)
            .await?
            .into_iter()
            .map(|proof| (proof.effect_key.clone(), proof))
            .collect::<BTreeMap<_, _>>();
        Ok(records
            .iter()
            .map(|record| FornaxDeliveryReport::new(record, proofs.get(&record.effect_key)))
            .collect())
    }

    /// Records proof from a trace read/backfill operation, bound to the local request identity.
    pub async fn reconcile_remote(
        &self,
        effect_key: &str,
        remote_trace_id: &str,
        remote_span_id: &str,
        proof_kind: &str,
        proof_digest: &str,
        reconciled_at_ms: i64,
    ) -> Result<WorkflowFornaxDeliveryProofRecord, FornaxDeliveryError> {
        for (name, value) in [
            ("effect_key", effect_key),
            ("remote_trace_id", remote_trace_id),
            ("remote_span_id", remote_span_id),
            ("proof_kind", proof_kind),
        ] {
            if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                return Err(FornaxDeliveryError::InvalidProof(name));
            }
        }
        if proof_digest.len() != 64
            || !proof_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(FornaxDeliveryError::InvalidProof("proof_digest"));
        }
        let correlation = self
            .store
            .read_fornax_trace(&self.run_id, effect_key)
            .await?
            .ok_or(FornaxDeliveryError::CorrelationNotFound)?;
        self.store
            .record_fornax_delivery_proof(WorkflowFornaxDeliveryProofPlan {
                run_id: self.run_id.clone(),
                effect_key: effect_key.to_string(),
                operation_id: correlation.operation_id,
                request_hash: correlation.request_hash,
                remote_trace_id: remote_trace_id.to_string(),
                remote_span_id: remote_span_id.to_string(),
                proof_kind: proof_kind.to_string(),
                proof_digest: proof_digest.to_string(),
                reconciled_at_ms,
            })
            .await
            .map_err(Into::into)
    }
}

impl From<&WorkflowFornaxTraceRecord> for FornaxDeliveryReport {
    fn from(record: &WorkflowFornaxTraceRecord) -> Self {
        Self::new(record, None)
    }
}

impl FornaxDeliveryReport {
    fn new(
        record: &WorkflowFornaxTraceRecord,
        proof: Option<&WorkflowFornaxDeliveryProofRecord>,
    ) -> Self {
        let state = match record.state {
            WorkflowFornaxTraceState::Planned
            | WorkflowFornaxTraceState::Live
            | WorkflowFornaxTraceState::Applied
            | WorkflowFornaxTraceState::Finished => FornaxDeliveryState::AcceptedLocal,
            WorkflowFornaxTraceState::Ambiguous => FornaxDeliveryState::DeliveryAmbiguous,
            WorkflowFornaxTraceState::Orphaned => FornaxDeliveryState::DeliveryFailedRetryable,
            WorkflowFornaxTraceState::Failed => {
                if matches!(record.error_code.as_deref(), Some("sdk_authentication")) {
                    FornaxDeliveryState::DeliveryFailedPermanent
                } else {
                    FornaxDeliveryState::DeliveryFailedRetryable
                }
            }
        };
        let state = if proof.is_some() {
            FornaxDeliveryState::DeliveredRemote
        } else {
            state
        };
        Self {
            effect_key: record.effect_key.clone(),
            operation_id: record.operation_id.clone(),
            state,
            error_code: record.error_code.clone(),
            remote_trace_id: proof.map(|proof| proof.remote_trace_id.clone()),
            remote_span_id: proof.map(|proof| proof.remote_span_id.clone()),
            proof_digest: proof.map(|proof| proof.proof_digest.clone()),
        }
    }
}
