use std::collections::HashMap;

use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::RolloutItem;
use uuid::Uuid;

const MARKER_PREFIX: &str = "codex-workflow-prepared/v1/";
const INPUT_HASH_BYTES: usize = 32;

/// Caller-generated identity for one crash-reconcilable workflow user turn.
///
/// The submission ID must be UUIDv7. The input hash is a canonical digest
/// computed and persisted by the workflow control plane before it asks core to
/// queue the normal `Op::UserInput`.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedUserTurn {
    submission_id: Uuid,
    input_hash: [u8; INPUT_HASH_BYTES],
}

impl PreparedUserTurn {
    /// Validates and constructs a prepared turn identity.
    pub fn new(
        submission_id: impl AsRef<str>,
        input_hash: [u8; INPUT_HASH_BYTES],
    ) -> Result<Self, PreparedUserTurnError> {
        let submission_id = Uuid::parse_str(submission_id.as_ref())
            .map_err(|_| PreparedUserTurnError::InvalidSubmissionId)?;
        if submission_id.get_version_num() != 7 {
            return Err(PreparedUserTurnError::SubmissionIdMustBeV7);
        }
        Ok(Self {
            submission_id,
            input_hash,
        })
    }

    /// Returns the UUIDv7 submission ID used as the normal Codex turn ID.
    pub fn submission_id(&self) -> String {
        self.submission_id.to_string()
    }

    pub(crate) fn marker(&self) -> String {
        format!(
            "{MARKER_PREFIX}{}/{}",
            self.submission_id,
            encode_hash(&self.input_hash)
        )
    }
}

/// Validation or reconciliation failure for a prepared user turn.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreparedUserTurnError {
    /// The supplied submission ID was not a UUID.
    InvalidSubmissionId,
    /// The supplied UUID was not version 7.
    SubmissionIdMustBeV7,
    /// The same submission ID was reused with different canonical input.
    InputHashConflict,
}

impl std::fmt::Display for PreparedUserTurnError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSubmissionId => formatter.write_str("prepared submission ID is invalid"),
            Self::SubmissionIdMustBeV7 => {
                formatter.write_str("prepared submission ID must be UUIDv7")
            }
            Self::InputHashConflict => {
                formatter.write_str("prepared submission ID has a different input hash")
            }
        }
    }
}

impl std::error::Error for PreparedUserTurnError {}

/// Result of reconciling a prepared turn before normal queue submission.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparedUserTurnSubmission {
    /// This caller owns the first queue attempt.
    Queue,
    /// The same process already accepted this exact prepared turn.
    AlreadyQueued,
    /// A matching user boundary already exists in persisted rollout history.
    BoundaryAlreadyPersisted,
}

/// Persisted boundary state for one prepared workflow turn.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparedUserTurnHistory {
    Missing,
    BoundaryPersisted,
    Conflict,
}

/// Inspects persisted rollout boundaries without registering or queueing a turn.
#[doc(hidden)]
pub fn inspect_prepared_user_turn_history(
    items: &[RolloutItem],
    prepared: &PreparedUserTurn,
) -> PreparedUserTurnHistory {
    let mut found = false;
    for item in items {
        let RolloutItem::EventMsg(EventMsg::ItemCompleted(completed)) = item else {
            continue;
        };
        let TurnItem::UserMessage(user_message) = &completed.item else {
            continue;
        };
        let Some(marker) = user_message.client_id.as_deref() else {
            continue;
        };
        let Some(observed) = parse_marker(marker) else {
            continue;
        };
        if observed.submission_id != prepared.submission_id {
            continue;
        }
        if observed.input_hash != prepared.input_hash {
            return PreparedUserTurnHistory::Conflict;
        }
        found = true;
    }
    if found {
        PreparedUserTurnHistory::BoundaryPersisted
    } else {
        PreparedUserTurnHistory::Missing
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreparedUserTurnState {
    Queued,
    BoundaryPersisted,
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreparedUserTurnEntry {
    input_hash: [u8; INPUT_HASH_BYTES],
    state: PreparedUserTurnState,
}

#[derive(Default)]
pub(crate) struct PreparedUserTurnRegistry {
    entries: HashMap<Uuid, PreparedUserTurnEntry>,
}

impl PreparedUserTurnRegistry {
    pub(crate) fn from_history(history: &InitialHistory) -> Self {
        let mut registry = Self::default();
        for item in history.get_rollout_items() {
            let RolloutItem::EventMsg(event) = item else {
                continue;
            };
            let EventMsg::ItemCompleted(completed) = event else {
                continue;
            };
            let TurnItem::UserMessage(user_message) = &completed.item else {
                continue;
            };
            let Some(marker) = user_message.client_id.as_deref() else {
                continue;
            };
            let Some(prepared) = parse_marker(marker) else {
                continue;
            };
            match registry.entries.get_mut(&prepared.submission_id) {
                Some(entry) if entry.input_hash != prepared.input_hash => {
                    entry.state = PreparedUserTurnState::Conflict;
                }
                Some(entry) => entry.state = PreparedUserTurnState::BoundaryPersisted,
                None => {
                    registry.entries.insert(
                        prepared.submission_id,
                        PreparedUserTurnEntry {
                            input_hash: prepared.input_hash,
                            state: PreparedUserTurnState::BoundaryPersisted,
                        },
                    );
                }
            }
        }
        registry
    }

    pub(crate) fn register(
        &mut self,
        prepared: &PreparedUserTurn,
    ) -> Result<PreparedUserTurnSubmission, PreparedUserTurnError> {
        match self.entries.get(&prepared.submission_id) {
            Some(entry) if entry.input_hash != prepared.input_hash => {
                Err(PreparedUserTurnError::InputHashConflict)
            }
            Some(PreparedUserTurnEntry {
                state: PreparedUserTurnState::Conflict,
                ..
            }) => Err(PreparedUserTurnError::InputHashConflict),
            Some(PreparedUserTurnEntry {
                state: PreparedUserTurnState::Queued,
                ..
            }) => Ok(PreparedUserTurnSubmission::AlreadyQueued),
            Some(PreparedUserTurnEntry {
                state: PreparedUserTurnState::BoundaryPersisted,
                ..
            }) => Ok(PreparedUserTurnSubmission::BoundaryAlreadyPersisted),
            None => {
                self.entries.insert(
                    prepared.submission_id,
                    PreparedUserTurnEntry {
                        input_hash: prepared.input_hash,
                        state: PreparedUserTurnState::Queued,
                    },
                );
                Ok(PreparedUserTurnSubmission::Queue)
            }
        }
    }

    pub(crate) fn remove_queued(&mut self, prepared: &PreparedUserTurn) {
        if self.entries.get(&prepared.submission_id)
            == Some(&PreparedUserTurnEntry {
                input_hash: prepared.input_hash,
                state: PreparedUserTurnState::Queued,
            })
        {
            self.entries.remove(&prepared.submission_id);
        }
    }
}

fn parse_marker(marker: &str) -> Option<PreparedUserTurn> {
    let suffix = marker.strip_prefix(MARKER_PREFIX)?;
    let (submission_id, input_hash) = suffix.split_once('/')?;
    PreparedUserTurn::new(submission_id, decode_hash(input_hash)?).ok()
}

fn encode_hash(hash: &[u8; INPUT_HASH_BYTES]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hash(hash: &str) -> Option<[u8; INPUT_HASH_BYTES]> {
    if hash.len() != INPUT_HASH_BYTES * 2 {
        return None;
    }
    let mut decoded = [0; INPUT_HASH_BYTES];
    for (index, byte) in decoded.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&hash[offset..offset + 2], 16).ok()?;
    }
    Some(decoded)
}

#[cfg(test)]
#[path = "prepared_user_turn_tests.rs"]
mod tests;
