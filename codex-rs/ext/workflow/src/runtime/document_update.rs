use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use codex_state::WorkflowEffectPlan;
use codex_state::WorkflowEffectPlanOutcome;
use codex_state::WorkflowEffectRecord;
use codex_state::WorkflowEffectState;
use codex_state::WorkflowEffectUpdate;
use codex_state::WorkflowStore;
use codex_state::WorkflowStoreError;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::EffectKey;
use crate::WorkflowRunId;
use crate::integrations::lark::ContentSensitivity;
use crate::integrations::lark::DocumentSelection;
use crate::integrations::lark::DocumentSnapshot;
use crate::integrations::lark::DocumentUpdateMode;
use crate::integrations::lark::DocumentUpdateRequest;
use crate::integrations::lark::LarkCli;
use crate::integrations::lark::LarkIdentity;

const EFFECT_KIND: &str = "lark.document.update.single-writer.v1";
const RETRYABLE_ERROR: &str = "lark_document_update_retryable";
const MAX_DISPATCH_ATTEMPTS: u64 = 3;

/// One deterministic full-content update to a workflow-owned Lark document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedDocumentUpdateRequest {
    pub effect_key: EffectKey,
    pub identity: LarkIdentity,
    pub document: String,
    pub expected_revision_id: String,
    pub expected_sha256: String,
    pub desired_markdown: String,
    pub sensitivity: ContentSensitivity,
}

/// Redacted proof that the requested document content is current.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedDocumentUpdateEvidence {
    pub document: String,
    pub revision_id: String,
    pub sha256: String,
    pub reconciled: bool,
}

/// Durable disposition for one revision-aware single-writer update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedDocumentUpdateOutcome {
    Applied(OwnedDocumentUpdateEvidence),
    Retryable {
        reason: String,
    },
    NeedsOperator {
        reason: String,
        observed_revision_id: Option<String>,
        observed_sha256: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum LarkDocumentUpdateError {
    #[error("invalid owned document update request")]
    InvalidRequest,
    #[error("owned document update journal changed concurrently")]
    ConcurrentMutation,
    #[error("owned document update journal response is malformed")]
    InvalidJournalResponse,
    #[error(transparent)]
    Store(#[from] WorkflowStoreError),
}

/// Journals and reconciles deterministic overwrites for single-writer documents.
#[derive(Clone)]
pub struct LarkDocumentService {
    cli: Arc<LarkCli>,
    store: WorkflowStore,
}

impl LarkDocumentService {
    pub fn new(cli: Arc<LarkCli>, store: WorkflowStore) -> Self {
        Self { cli, store }
    }

    pub async fn update_owned_document(
        &self,
        run_id: WorkflowRunId,
        request: &OwnedDocumentUpdateRequest,
        now_ms: i64,
        cancelled: &AtomicBool,
    ) -> Result<OwnedDocumentUpdateOutcome, LarkDocumentUpdateError> {
        validate_request(request)?;
        let desired_sha256 = content_sha256(&request.desired_markdown);
        let run_id = run_id.to_string();
        let effect_key = request.effect_key.to_string();
        let planned = self
            .store
            .plan_effect(WorkflowEffectPlan {
                run_id: run_id.clone(),
                effect_key: effect_key.clone(),
                kind: EFFECT_KIND.to_string(),
                request: json!({
                    "document": request.document,
                    "identity": identity_name(request.identity),
                    "expected_revision_id": request.expected_revision_id,
                    "expected_sha256": request.expected_sha256,
                    "desired_sha256": desired_sha256,
                    "mode": "overwrite",
                    "consistency": "single_writer_revision_aware",
                }),
                created_at_ms: now_ms,
            })
            .await?;
        let mut effect = match planned {
            WorkflowEffectPlanOutcome::Planned(effect)
            | WorkflowEffectPlanOutcome::Existing(effect) => effect,
        };

        let observed = match self
            .cli
            .fetch_document(request.identity, &request.document, cancelled)
        {
            Ok(observed) => observed,
            Err(_) => {
                return self
                    .observation_unavailable(&run_id, &effect_key, &effect, now_ms)
                    .await;
            }
        };
        if observed.doc_id != request.document || observed.revision_id.trim().is_empty() {
            return self
                .needs_operator(
                    &run_id,
                    &effect_key,
                    &effect,
                    now_ms,
                    "unexpected_document_identity",
                    Some(&observed),
                )
                .await;
        }
        let observed_sha256 = content_sha256(&observed.markdown);

        if effect.state == WorkflowEffectState::Applied {
            let evidence = applied_evidence(&effect)?;
            if evidence.revision_id == observed.revision_id
                && evidence.sha256 == observed_sha256
                && observed_sha256 == desired_sha256
            {
                return Ok(OwnedDocumentUpdateOutcome::Applied(evidence));
            }
            return self
                .needs_operator(
                    &run_id,
                    &effect_key,
                    &effect,
                    now_ms,
                    "unexpected_post_apply_drift",
                    Some(&observed),
                )
                .await;
        }

        if observed_sha256 == desired_sha256 {
            let evidence = OwnedDocumentUpdateEvidence {
                document: request.document.clone(),
                revision_id: observed.revision_id,
                sha256: desired_sha256,
                reconciled: true,
            };
            self.apply_evidence(&run_id, &effect_key, &effect, &evidence, now_ms)
                .await?;
            return Ok(OwnedDocumentUpdateOutcome::Applied(evidence));
        }

        if observed.revision_id != request.expected_revision_id
            || observed_sha256 != request.expected_sha256
        {
            return self
                .needs_operator(
                    &run_id,
                    &effect_key,
                    &effect,
                    now_ms,
                    "unexpected_revision_drift",
                    Some(&observed),
                )
                .await;
        }

        if effect.state == WorkflowEffectState::Cancelled {
            return Ok(needs_operator_outcome(
                "document_update_cancelled",
                Some(&observed),
            ));
        }
        if effect.state == WorkflowEffectState::Ambiguous
            && effect.error_code.as_deref() != Some(RETRYABLE_ERROR)
        {
            return Ok(needs_operator_outcome(
                "document_update_requires_operator",
                Some(&observed),
            ));
        }

        let prior_attempts = dispatch_attempts(&effect);
        if prior_attempts >= MAX_DISPATCH_ATTEMPTS {
            return self
                .needs_operator(
                    &run_id,
                    &effect_key,
                    &effect,
                    now_ms,
                    "document_update_retry_exhausted",
                    Some(&observed),
                )
                .await;
        }
        let attempts = prior_attempts + 1;
        self.transition(WorkflowEffectUpdate {
            run_id: run_id.clone(),
            effect_key: effect_key.clone(),
            expected_state: effect.state,
            state: WorkflowEffectState::Dispatched,
            response: Some(json!({"dispatch_attempts": attempts})),
            error_code: None,
            updated_at_ms: now_ms,
        })
        .await?;
        effect.state = WorkflowEffectState::Dispatched;
        effect.response = Some(json!({"dispatch_attempts": attempts}));
        effect.error_code = None;

        let update_result = self.cli.update_document(
            &DocumentUpdateRequest {
                identity: request.identity,
                document: request.document.clone(),
                mode: DocumentUpdateMode::Overwrite,
                markdown: Some(request.desired_markdown.clone()),
                selection: None::<DocumentSelection>,
                new_title: None,
                sensitivity: request.sensitivity,
            },
            cancelled,
        );
        let after = match self
            .cli
            .fetch_document(request.identity, &request.document, cancelled)
        {
            Ok(after) => after,
            Err(_) => {
                return self
                    .mark_retryable(&run_id, &effect_key, &effect, now_ms)
                    .await;
            }
        };
        let after_sha256 = content_sha256(&after.markdown);
        if after.doc_id == request.document
            && after.revision_id != request.expected_revision_id
            && after_sha256 == desired_sha256
        {
            let evidence = OwnedDocumentUpdateEvidence {
                document: request.document.clone(),
                revision_id: after.revision_id,
                sha256: desired_sha256,
                reconciled: update_result.is_err() || prior_attempts > 0,
            };
            self.apply_evidence(&run_id, &effect_key, &effect, &evidence, now_ms)
                .await?;
            return Ok(OwnedDocumentUpdateOutcome::Applied(evidence));
        }
        if after.doc_id == request.document
            && after.revision_id == request.expected_revision_id
            && after_sha256 == request.expected_sha256
        {
            return self
                .mark_retryable(&run_id, &effect_key, &effect, now_ms)
                .await;
        }
        self.needs_operator(
            &run_id,
            &effect_key,
            &effect,
            now_ms,
            "unexpected_post_update_content",
            Some(&after),
        )
        .await
    }

    async fn observation_unavailable(
        &self,
        run_id: &str,
        effect_key: &str,
        effect: &WorkflowEffectRecord,
        now_ms: i64,
    ) -> Result<OwnedDocumentUpdateOutcome, LarkDocumentUpdateError> {
        if matches!(
            effect.state,
            WorkflowEffectState::Dispatched | WorkflowEffectState::Ambiguous
        ) {
            return self
                .mark_retryable(run_id, effect_key, effect, now_ms)
                .await;
        }
        Ok(OwnedDocumentUpdateOutcome::Retryable {
            reason: "document_observation_unavailable".to_string(),
        })
    }

    async fn mark_retryable(
        &self,
        run_id: &str,
        effect_key: &str,
        effect: &WorkflowEffectRecord,
        now_ms: i64,
    ) -> Result<OwnedDocumentUpdateOutcome, LarkDocumentUpdateError> {
        let attempts = dispatch_attempts(effect);
        if attempts >= MAX_DISPATCH_ATTEMPTS {
            return self
                .needs_operator(
                    run_id,
                    effect_key,
                    effect,
                    now_ms,
                    "document_update_retry_exhausted",
                    None,
                )
                .await;
        }
        if effect.state != WorkflowEffectState::Ambiguous {
            self.transition(WorkflowEffectUpdate {
                run_id: run_id.to_string(),
                effect_key: effect_key.to_string(),
                expected_state: effect.state,
                state: WorkflowEffectState::Ambiguous,
                response: Some(json!({"dispatch_attempts": attempts})),
                error_code: Some(RETRYABLE_ERROR.to_string()),
                updated_at_ms: now_ms,
            })
            .await?;
        }
        Ok(OwnedDocumentUpdateOutcome::Retryable {
            reason: "document_update_not_observed".to_string(),
        })
    }

    async fn needs_operator(
        &self,
        run_id: &str,
        effect_key: &str,
        effect: &WorkflowEffectRecord,
        now_ms: i64,
        reason: &str,
        observed: Option<&DocumentSnapshot>,
    ) -> Result<OwnedDocumentUpdateOutcome, LarkDocumentUpdateError> {
        if effect.state != WorkflowEffectState::Ambiguous {
            self.transition(WorkflowEffectUpdate {
                run_id: run_id.to_string(),
                effect_key: effect_key.to_string(),
                expected_state: effect.state,
                state: WorkflowEffectState::Ambiguous,
                response: Some(observation_json(observed, dispatch_attempts(effect))),
                error_code: Some(reason.to_string()),
                updated_at_ms: now_ms,
            })
            .await?;
        }
        Ok(needs_operator_outcome(reason, observed))
    }

    async fn apply_evidence(
        &self,
        run_id: &str,
        effect_key: &str,
        effect: &WorkflowEffectRecord,
        evidence: &OwnedDocumentUpdateEvidence,
        now_ms: i64,
    ) -> Result<(), LarkDocumentUpdateError> {
        self.transition(WorkflowEffectUpdate {
            run_id: run_id.to_string(),
            effect_key: effect_key.to_string(),
            expected_state: effect.state,
            state: WorkflowEffectState::Applied,
            response: Some(
                serde_json::to_value(evidence)
                    .map_err(|_| LarkDocumentUpdateError::InvalidJournalResponse)?,
            ),
            error_code: None,
            updated_at_ms: now_ms,
        })
        .await
    }

    async fn transition(
        &self,
        update: WorkflowEffectUpdate,
    ) -> Result<(), LarkDocumentUpdateError> {
        if !self.store.update_effect(update).await? {
            return Err(LarkDocumentUpdateError::ConcurrentMutation);
        }
        Ok(())
    }
}

fn validate_request(request: &OwnedDocumentUpdateRequest) -> Result<(), LarkDocumentUpdateError> {
    if request.document.trim().is_empty()
        || request.expected_revision_id.trim().is_empty()
        || !valid_sha256(&request.expected_sha256)
    {
        return Err(LarkDocumentUpdateError::InvalidRequest);
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn content_sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn identity_name(identity: LarkIdentity) -> &'static str {
    match identity {
        LarkIdentity::Bot => "bot",
        LarkIdentity::User => "user",
    }
}

fn dispatch_attempts(effect: &WorkflowEffectRecord) -> u64 {
    effect
        .response
        .as_ref()
        .and_then(|response| response.get("dispatch_attempts"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

fn applied_evidence(
    effect: &WorkflowEffectRecord,
) -> Result<OwnedDocumentUpdateEvidence, LarkDocumentUpdateError> {
    serde_json::from_value(
        effect
            .response
            .clone()
            .ok_or(LarkDocumentUpdateError::InvalidJournalResponse)?,
    )
    .map_err(|_| LarkDocumentUpdateError::InvalidJournalResponse)
}

fn observation_json(
    observed: Option<&DocumentSnapshot>,
    dispatch_attempts: u64,
) -> serde_json::Value {
    json!({
        "dispatch_attempts": dispatch_attempts,
        "observed_revision_id": observed.map(|snapshot| snapshot.revision_id.as_str()),
        "observed_sha256": observed.map(|snapshot| content_sha256(&snapshot.markdown)),
    })
}

fn needs_operator_outcome(
    reason: &str,
    observed: Option<&DocumentSnapshot>,
) -> OwnedDocumentUpdateOutcome {
    OwnedDocumentUpdateOutcome::NeedsOperator {
        reason: reason.to_string(),
        observed_revision_id: observed.map(|snapshot| snapshot.revision_id.clone()),
        observed_sha256: observed.map(|snapshot| content_sha256(&snapshot.markdown)),
    }
}
