use std::time::Duration;

use sha2::Digest;
use sha2::Sha256;

use crate::BackoffPolicy;
use crate::RetryClassification;
use crate::RetryPolicy;
use crate::RetrySession;

/// Deterministic result of applying a persisted retry policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryDecision {
    Fail,
    RetryAt {
        retry_at_ms: i64,
        session: RetrySession,
    },
}

/// Pure retry classifier/backoff calculator safe to repeat after restart.
pub struct RetryPlanner;

impl RetryPlanner {
    pub fn decide(
        policy: &RetryPolicy,
        completed_attempts: u32,
        classification: RetryClassification,
        attempt_id: &str,
        now_ms: i64,
        workflow_deadline_ms: Option<i64>,
    ) -> RetryDecision {
        if completed_attempts >= policy.maximum_attempts.get()
            || !policy.retry_on.contains(&classification)
        {
            return RetryDecision::Fail;
        }
        let delay = retry_delay(
            &policy.backoff,
            completed_attempts.max(1),
            deterministic_unit(attempt_id),
        );
        let delay_ms = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
        let retry_at_ms = now_ms.saturating_add(delay_ms);
        if workflow_deadline_ms.is_some_and(|deadline| retry_at_ms > deadline) {
            return RetryDecision::Fail;
        }
        RetryDecision::RetryAt {
            retry_at_ms,
            session: policy.session,
        }
    }
}

fn retry_delay(policy: &BackoffPolicy, attempt: u32, jitter_unit: u64) -> Duration {
    match policy {
        BackoffPolicy::None => Duration::ZERO,
        BackoffPolicy::Fixed { delay_ms } => Duration::from_millis(*delay_ms),
        BackoffPolicy::Exponential {
            initial_delay_ms,
            maximum_delay_ms,
            jitter_percent,
        } => {
            let shift = attempt.saturating_sub(1).min(63);
            let uncapped = initial_delay_ms.saturating_mul(1_u64 << shift);
            let capped = uncapped.min(*maximum_delay_ms);
            let spread = capped.saturating_mul(u64::from(*jitter_percent)) / 100;
            let centered = if spread == 0 {
                capped
            } else {
                let width = spread.saturating_mul(2).saturating_add(1);
                capped
                    .saturating_sub(spread)
                    .saturating_add(jitter_unit % width)
            };
            Duration::from_millis(centered.min(*maximum_delay_ms))
        }
    }
}

fn deterministic_unit(value: &str) -> u64 {
    let digest = Sha256::digest(value.as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("SHA-256 prefix is 8 bytes"))
}
