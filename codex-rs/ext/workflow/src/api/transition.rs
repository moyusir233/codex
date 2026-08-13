use std::time::SystemTime;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::api::InteractionId;
use crate::api::NodeId;

/// Durable result of one reducer step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowTransition<S, O> {
    /// Persist the new state and schedule another reducer step immediately.
    Continue {
        /// Next durable checkpoint.
        state: S,
    },
    /// Persist the new state and sleep until the declared durable wake condition.
    Wait {
        /// Next durable checkpoint.
        state: S,
        /// Durable condition that can make the run runnable again.
        wake: WakeCondition,
    },
    /// Persist an actionable checkpoint and transfer ownership to an operator.
    NeedsOperator {
        /// Checkpoint from which an explicit operator resume can continue.
        state: S,
        /// Stable machine-readable reason code, without sensitive details.
        error_code: String,
        /// Redacted structured evidence explaining the required action.
        metadata: Value,
    },
    /// Persist the successful terminal output.
    Complete {
        /// Typed successful result.
        output: O,
    },
}

/// Durable condition that can wake a waiting workflow run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WakeCondition {
    /// Wake when one node reaches a terminal state.
    NodeTerminal(NodeId),
    /// Wake when every listed node reaches a terminal state.
    NodesTerminal(Vec<NodeId>),
    /// Wake when one correlated durable human interaction resolves.
    HumanInteraction(InteractionId),
    /// Wake at or after the supplied wall-clock instant.
    RetryAt(SystemTime),
    /// Wake when cancellation is requested.
    Cancellation,
}
