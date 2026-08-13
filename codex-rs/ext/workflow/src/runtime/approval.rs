use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use codex_state::WorkflowApprovalDecisionAppend;
use codex_state::WorkflowApprovalDecisionRecord;
use codex_state::WorkflowApprovalPlan;
use codex_state::WorkflowApprovalPlanOutcome;
use codex_state::WorkflowStore;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;

use crate::ApprovalDecision;
use crate::ApprovalDecisionEvidence;
use crate::ApprovalId;
use crate::ApprovalOutcome;
use crate::ApprovalRequest;
use crate::ArtifactId;
use crate::EffectKey;
use crate::HumanInteractionOutcome;
use crate::HumanInteractionRequest;
use crate::WorkflowRunId;
use crate::integrations::lark::ContentSensitivity;
use crate::integrations::lark::OpenId;

use super::LarkInteractionError;
use super::LarkInteractionService;
use super::WorkflowArtifactStore;
use super::WorkflowArtifactStoreError;

const MAX_INVALID_RESPONSES_PER_APPROVER: usize = 3;

/// Generic revision-bound approval service over durable Lark interactions.
#[derive(Clone)]
pub struct WorkflowApprovalService {
    store: WorkflowStore,
    lark: Arc<LarkInteractionService>,
    artifacts: WorkflowArtifactStore,
}

impl WorkflowApprovalService {
    pub fn new(
        store: WorkflowStore,
        lark: Arc<LarkInteractionService>,
        artifacts: WorkflowArtifactStore,
    ) -> Self {
        Self {
            store,
            lark,
            artifacts,
        }
    }

    /// Reconciles one request, polls attributable decisions, and derives quorum.
    pub async fn poll_or_request(
        &self,
        run_id: WorkflowRunId,
        request: ApprovalRequest,
        now_ms: i64,
        cancelled: &AtomicBool,
    ) -> Result<ApprovalOutcome, WorkflowApprovalError> {
        validate_request(&request)?;
        let subject_bytes = self
            .artifacts
            .read(run_id, request.subject.artifact_id)
            .await?;
        let actual_sha256 = format!("{:x}", Sha256::digest(subject_bytes));
        if !actual_sha256.eq_ignore_ascii_case(&request.subject.sha256) {
            return Err(WorkflowApprovalError::SubjectDigestMismatch);
        }
        let allowed_approvers = request
            .allowed_approvers
            .iter()
            .map(|approver| approver.as_str().to_string())
            .collect::<Vec<_>>();
        let planned = self
            .store
            .plan_approval(WorkflowApprovalPlan {
                approval_id: ApprovalId::new().to_string(),
                run_id: run_id.to_string(),
                effect_key: request.effect_key.to_string(),
                gate: request.gate.clone(),
                subject: serde_json::to_value(&request.subject)?,
                allowed_approvers,
                quorum: u32::try_from(request.quorum.get())
                    .map_err(|_| WorkflowApprovalError::InvalidRequest)?,
                deadline_ms: request.deadline_ms,
                created_at_ms: now_ms,
            })
            .await?;
        let approval = match planned {
            WorkflowApprovalPlanOutcome::Planned(approval)
            | WorkflowApprovalPlanOutcome::Existing(approval) => approval,
        };
        let approval_id = ApprovalId::parse(&approval.approval_id)?;
        let nonce = &approval.request_hash[..24];

        let decisions = self
            .store
            .list_approval_decisions(&approval.approval_id)
            .await?;
        if let Some(outcome) = derive_terminal(
            approval_id,
            approval.quorum,
            approval.deadline_ms,
            now_ms,
            &decisions,
        )? {
            return Ok(outcome);
        }

        for approver in &request.allowed_approvers {
            if decisions
                .iter()
                .any(|decision| decision.sender_id == approver.as_str())
            {
                continue;
            }
            match self
                .poll_approver(
                    run_id,
                    approval_id,
                    &request,
                    approver,
                    nonce,
                    now_ms,
                    cancelled,
                )
                .await?
            {
                ApproverPoll::Waiting(interaction_id) => {
                    return Ok(ApprovalOutcome::Waiting {
                        approval_id,
                        interaction_id,
                    });
                }
                ApproverPoll::Cancelled => {
                    return Ok(ApprovalOutcome::Cancelled { approval_id });
                }
                ApproverPoll::NeedsOperator(reason) => {
                    return Ok(ApprovalOutcome::NeedsOperator {
                        approval_id,
                        reason,
                    });
                }
                ApproverPoll::Decision(decision) => {
                    self.store.append_approval_decision(decision).await?;
                    let decisions = self
                        .store
                        .list_approval_decisions(&approval.approval_id)
                        .await?;
                    if let Some(outcome) = derive_terminal(
                        approval_id,
                        approval.quorum,
                        approval.deadline_ms,
                        now_ms,
                        &decisions,
                    )? {
                        return Ok(outcome);
                    }
                }
            }
        }

        let decisions = self
            .store
            .list_approval_decisions(&approval.approval_id)
            .await?;
        derive_terminal(
            approval_id,
            approval.quorum,
            approval.deadline_ms,
            now_ms,
            &decisions,
        )?
        .ok_or(WorkflowApprovalError::InvalidState)
    }

    /// Appends an explicit authorized revocation without rewriting history.
    pub async fn revoke(
        &self,
        run_id: WorkflowRunId,
        approval_id: ApprovalId,
        actor: OpenId,
        reason: String,
        now_ms: i64,
    ) -> Result<ApprovalOutcome, WorkflowApprovalError> {
        let approval = self
            .store
            .read_approval(&run_id.to_string(), &approval_id.to_string())
            .await?
            .ok_or(WorkflowApprovalError::NotFound)?;
        if !approval
            .allowed_approvers
            .iter()
            .any(|allowed| allowed == actor.as_str())
        {
            return Err(WorkflowApprovalError::UnauthorizedApprover);
        }
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(format!(
                "{}\0{}\0{}\0revoke",
                approval.approval_id,
                actor.as_str(),
                reason
            ))
        );
        self.store
            .append_approval_decision(WorkflowApprovalDecisionAppend {
                decision_id: fingerprint.clone(),
                approval_id: approval.approval_id.clone(),
                sender_id: actor.as_str().to_string(),
                message_id: Some(format!("revocation:{}", &fingerprint[..32])),
                response_artifact_id: None,
                decision: "revoke".to_string(),
                reason: Some(reason),
                created_at_ms: now_ms,
            })
            .await?;
        let decisions = self
            .store
            .list_approval_decisions(&approval.approval_id)
            .await?;
        derive_terminal(
            approval_id,
            approval.quorum,
            approval.deadline_ms,
            now_ms,
            &decisions,
        )?
        .ok_or(WorkflowApprovalError::InvalidState)
    }

    /// Re-derives one existing approval from append-only evidence without
    /// dispatching a new interaction. Delivery guards use this to detect a
    /// revocation or change recorded after an earlier gate advanced.
    pub async fn current_outcome(
        &self,
        run_id: WorkflowRunId,
        approval_id: ApprovalId,
        now_ms: i64,
    ) -> Result<ApprovalOutcome, WorkflowApprovalError> {
        let approval = self
            .store
            .read_approval(&run_id.to_string(), &approval_id.to_string())
            .await?
            .ok_or(WorkflowApprovalError::NotFound)?;
        let decisions = self
            .store
            .list_approval_decisions(&approval.approval_id)
            .await?;
        derive_terminal(
            approval_id,
            approval.quorum,
            approval.deadline_ms,
            now_ms,
            &decisions,
        )?
        .ok_or(WorkflowApprovalError::InvalidState)
    }

    #[allow(clippy::too_many_arguments)]
    async fn poll_approver(
        &self,
        run_id: WorkflowRunId,
        approval_id: ApprovalId,
        request: &ApprovalRequest,
        approver: &OpenId,
        nonce: &str,
        now_ms: i64,
        cancelled: &AtomicBool,
    ) -> Result<ApproverPoll, WorkflowApprovalError> {
        let mut prior_invalid_artifact = None;
        for attempt in 0..=MAX_INVALID_RESPONSES_PER_APPROVER {
            let effect_key = approval_interaction_effect(
                approval_id,
                approver,
                attempt,
                prior_invalid_artifact,
            )?;
            let prompt = format!(
                "{}\n\nApproval gate: {}\nSubject SHA-256: {}\nRequest nonce: {}\nReply exactly with one of:\nAPPROVE {} {}\nCHANGES_REQUESTED {} {} <reason>\nREVOKE {} {} <reason>",
                request.prompt,
                request.gate,
                request.subject.sha256,
                nonce,
                nonce,
                request.subject.sha256,
                nonce,
                request.subject.sha256,
                nonce,
                request.subject.sha256,
            );
            let outcome = self
                .lark
                .poll_or_request(
                    run_id,
                    HumanInteractionRequest {
                        effect_key,
                        prompt,
                        chat_id: request.chat_id.clone(),
                        thread_id: request.thread_id.clone(),
                        allowed_senders: vec![approver.clone()],
                        deadline_ms: request.deadline_ms,
                        sensitivity: ContentSensitivity::NonSensitive,
                    },
                    now_ms,
                    cancelled,
                )
                .await?;
            match outcome {
                HumanInteractionOutcome::Waiting { interaction_id }
                | HumanInteractionOutcome::TimedOut { interaction_id } => {
                    return Ok(ApproverPoll::Waiting(interaction_id));
                }
                HumanInteractionOutcome::Cancelled { .. } => {
                    return Ok(ApproverPoll::Cancelled);
                }
                HumanInteractionOutcome::NeedsOperator { reason, .. } => {
                    return Ok(ApproverPoll::NeedsOperator(reason));
                }
                HumanInteractionOutcome::Resolved { artifact_id, .. } => {
                    let reply = self.read_reply(run_id, artifact_id).await?;
                    if reply.sender_id != approver.as_str() {
                        return Ok(ApproverPoll::NeedsOperator(
                            "approval_sender_mismatch".to_string(),
                        ));
                    }
                    if let Some((decision, reason)) =
                        parse_decision(&reply.text, nonce, &request.subject.sha256)
                    {
                        let decision_id = format!(
                            "{:x}",
                            Sha256::digest(format!(
                                "{}\0{}\0{}",
                                approval_id, reply.message_id, reply.sender_id
                            ))
                        );
                        return Ok(ApproverPoll::Decision(WorkflowApprovalDecisionAppend {
                            decision_id,
                            approval_id: approval_id.to_string(),
                            sender_id: reply.sender_id,
                            message_id: Some(reply.message_id),
                            response_artifact_id: Some(artifact_id.to_string()),
                            decision: decision_name(decision).to_string(),
                            reason,
                            created_at_ms: reply.created_at_ms,
                        }));
                    }
                    prior_invalid_artifact = Some(artifact_id);
                }
            }
        }
        Ok(ApproverPoll::NeedsOperator(
            "approval_invalid_response_limit".to_string(),
        ))
    }

    async fn read_reply(
        &self,
        run_id: WorkflowRunId,
        artifact_id: ArtifactId,
    ) -> Result<ApprovalReplyArtifact, WorkflowApprovalError> {
        let bytes = self.artifacts.read(run_id, artifact_id).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

enum ApproverPoll {
    Waiting(crate::InteractionId),
    Cancelled,
    NeedsOperator(String),
    Decision(WorkflowApprovalDecisionAppend),
}

#[derive(Deserialize)]
struct ApprovalReplyArtifact {
    message_id: String,
    sender_id: String,
    created_at_ms: i64,
    text: String,
}

fn validate_request(request: &ApprovalRequest) -> Result<(), WorkflowApprovalError> {
    if request.gate.is_empty()
        || request.gate.len() > 128
        || !request
            .gate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || request.allowed_approvers.is_empty()
        || request.quorum.get() > request.allowed_approvers.len()
        || request.deadline_ms <= 0
        || request.prompt.is_empty()
        || request.prompt.len() > 4_000
        || request.subject.sha256.len() != 64
        || !request
            .subject
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || request.subject.document_id.is_some() != request.subject.document_revision.is_some()
    {
        return Err(WorkflowApprovalError::InvalidRequest);
    }
    Ok(())
}

fn approval_interaction_effect(
    approval_id: ApprovalId,
    approver: &OpenId,
    attempt: usize,
    prior_invalid_artifact: Option<ArtifactId>,
) -> Result<EffectKey, WorkflowApprovalError> {
    let digest = Sha256::digest(format!(
        "{}\0{}\0{}\0{}",
        approval_id,
        approver.as_str(),
        attempt,
        prior_invalid_artifact
            .map(|id| id.to_string())
            .unwrap_or_default()
    ));
    Ok(EffectKey::new(format!(
        "approval-response-{}",
        &format!("{digest:x}")[..32]
    ))?)
}

fn parse_decision(
    text: &str,
    expected_nonce: &str,
    expected_sha256: &str,
) -> Option<(ApprovalDecision, Option<String>)> {
    let command = text
        .split_whitespace()
        .filter(|token| !token.starts_with("[wf:"))
        .collect::<Vec<_>>();
    let [verb, nonce, sha256, rest @ ..] = command.as_slice() else {
        return None;
    };
    if *nonce != expected_nonce || !sha256.eq_ignore_ascii_case(expected_sha256) {
        return None;
    }
    match *verb {
        "APPROVE" if rest.is_empty() => Some((ApprovalDecision::Approve, None)),
        "CHANGES_REQUESTED" if !rest.is_empty() => {
            Some((ApprovalDecision::ChangesRequested, Some(rest.join(" "))))
        }
        "REVOKE" if !rest.is_empty() => Some((ApprovalDecision::Revoke, Some(rest.join(" ")))),
        _ => None,
    }
}

fn derive_terminal(
    approval_id: ApprovalId,
    quorum: u32,
    deadline_ms: i64,
    now_ms: i64,
    decisions: &[WorkflowApprovalDecisionRecord],
) -> Result<Option<ApprovalOutcome>, WorkflowApprovalError> {
    let evidence = decisions
        .iter()
        .map(decision_evidence)
        .collect::<Result<Vec<_>, _>>()?;
    if evidence
        .iter()
        .any(|item| item.decision == ApprovalDecision::Revoke)
    {
        return Ok(Some(ApprovalOutcome::Revoked {
            approval_id,
            decisions: evidence,
        }));
    }
    if evidence
        .iter()
        .any(|item| item.decision == ApprovalDecision::ChangesRequested)
    {
        return Ok(Some(ApprovalOutcome::ChangesRequested {
            approval_id,
            decisions: evidence,
        }));
    }
    let approvals = evidence
        .iter()
        .filter(|item| item.decision == ApprovalDecision::Approve)
        .map(|item| item.sender.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if approvals.len()
        >= usize::try_from(quorum).map_err(|_| WorkflowApprovalError::InvalidState)?
    {
        return Ok(Some(ApprovalOutcome::Approved {
            approval_id,
            decisions: evidence,
        }));
    }
    if now_ms > deadline_ms {
        return Ok(Some(ApprovalOutcome::TimedOut { approval_id }));
    }
    Ok(None)
}

fn decision_evidence(
    record: &WorkflowApprovalDecisionRecord,
) -> Result<ApprovalDecisionEvidence, WorkflowApprovalError> {
    Ok(ApprovalDecisionEvidence {
        sender: OpenId::parse(record.sender_id.clone())?,
        decision: match record.decision.as_str() {
            "approve" => ApprovalDecision::Approve,
            "changes_requested" => ApprovalDecision::ChangesRequested,
            "revoke" => ApprovalDecision::Revoke,
            _ => return Err(WorkflowApprovalError::InvalidState),
        },
        message_id: record.message_id.clone(),
        response_artifact_id: record
            .response_artifact_id
            .as_deref()
            .map(ArtifactId::parse)
            .transpose()?,
        reason: record.reason.clone(),
        created_at_ms: record.created_at_ms,
    })
}

fn decision_name(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Approve => "approve",
        ApprovalDecision::ChangesRequested => "changes_requested",
        ApprovalDecision::Revoke => "revoke",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowApprovalError {
    #[error("invalid approval request")]
    InvalidRequest,
    #[error("approval was not found in this workflow run")]
    NotFound,
    #[error("approval actor is not authorized")]
    UnauthorizedApprover,
    #[error("durable approval state is invalid")]
    InvalidState,
    #[error("approval subject bytes do not match the declared SHA-256")]
    SubjectDigestMismatch,
    #[error(transparent)]
    Store(#[from] codex_state::WorkflowStoreError),
    #[error(transparent)]
    Interaction(#[from] LarkInteractionError),
    #[error(transparent)]
    Artifact(#[from] WorkflowArtifactStoreError),
    #[error(transparent)]
    Identifier(#[from] crate::IdentifierError),
    #[error(transparent)]
    Lark(#[from] crate::integrations::lark::LarkCliError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
