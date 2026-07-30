use std::fmt;
use std::str::FromStr;

use semver::Version;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

const MAX_NAME_BYTES: usize = 64;
const MAX_KEY_BYTES: usize = 128;

/// Validation failure for a workflow name or stable run-local key.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IdentifierError {
    /// The value was empty or exceeded its byte limit.
    #[error("{kind} must contain between 1 and {max_bytes} bytes")]
    InvalidLength {
        /// Human-readable identifier kind.
        kind: &'static str,
        /// Maximum accepted UTF-8 byte length.
        max_bytes: usize,
    },
    /// The value did not follow the identifier's ASCII grammar.
    #[error("{kind} has an invalid format")]
    InvalidFormat {
        /// Human-readable identifier kind.
        kind: &'static str,
    },
    /// The supplied UUID was malformed or not version 7.
    #[error("{kind} must be a UUIDv7")]
    InvalidUuid {
        /// Human-readable identifier kind.
        kind: &'static str,
    },
    /// The workflow version was not valid semantic version syntax.
    #[error("workflow version is not valid semantic version syntax")]
    InvalidVersion,
    /// Workflow event sequences start at one.
    #[error("workflow sequence must be greater than zero")]
    ZeroSequence,
}

macro_rules! string_id {
    ($name:ident, $kind:literal, $validator:ident, $max:expr, $docs:literal) => {
        #[doc = $docs]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Validates and constructs this identifier.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                validate_length(&value, $kind, $max)?;
                if !$validator(&value) {
                    return Err(IdentifierError::InvalidFormat { kind: $kind });
                }
                Ok(Self(value))
            }

            /// Returns the validated string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdentifierError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

string_id!(
    WorkflowName,
    "workflow name",
    valid_workflow_name,
    MAX_NAME_BYTES,
    "Validated lowercase kebab-case workflow definition name."
);
string_id!(
    NodeKey,
    "node key",
    valid_stable_key,
    MAX_KEY_BYTES,
    "Stable run-local key used to identify a logical workflow node."
);
string_id!(
    EffectKey,
    "effect key",
    valid_stable_key,
    MAX_KEY_BYTES,
    "Stable run-local idempotency key for one journaled workflow effect."
);

/// Semantic version pinned to a workflow definition and every durable run.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowVersion(Version);

impl WorkflowVersion {
    /// Parses a semantic workflow version.
    pub fn parse(value: &str) -> Result<Self, IdentifierError> {
        Version::parse(value)
            .map(Self)
            .map_err(|_| IdentifierError::InvalidVersion)
    }

    /// Returns the parsed semantic version.
    pub fn as_version(&self) -> &Version {
        &self.0
    }
}

impl fmt::Display for WorkflowVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for WorkflowVersion {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

macro_rules! uuid_id {
    ($name:ident, $kind:literal, $docs:literal) => {
        #[doc = $docs]
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(try_from = "Uuid", into = "Uuid")]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a time-ordered UUIDv7 identifier.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Parses and validates a UUIDv7 identifier.
            pub fn parse(value: &str) -> Result<Self, IdentifierError> {
                let value = Uuid::parse_str(value)
                    .map_err(|_| IdentifierError::InvalidUuid { kind: $kind })?;
                if value.get_version_num() != 7 {
                    return Err(IdentifierError::InvalidUuid { kind: $kind });
                }
                Ok(Self(value))
            }

            /// Returns the UUID value.
            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl TryFrom<Uuid> for $name {
            type Error = IdentifierError;

            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                if value.get_version_num() != 7 {
                    return Err(IdentifierError::InvalidUuid { kind: $kind });
                }
                Ok(Self(value))
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

uuid_id!(
    WorkflowRunId,
    "workflow run ID",
    "UUIDv7 identity of one durable workflow run."
);
uuid_id!(
    NodeId,
    "node ID",
    "UUIDv7 identity of one materialized node."
);
uuid_id!(
    NodeAttemptId,
    "node attempt ID",
    "UUIDv7 identity of one node execution attempt."
);
uuid_id!(
    InteractionId,
    "interaction ID",
    "UUIDv7 identity of one durable human interaction."
);

/// Monotonic, one-based sequence number for durable workflow events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct WorkflowSequence(u64);

impl WorkflowSequence {
    /// Validates a one-based event sequence.
    pub fn new(value: u64) -> Result<Self, IdentifierError> {
        if value == 0 {
            return Err(IdentifierError::ZeroSequence);
        }
        Ok(Self(value))
    }

    /// Returns the underlying one-based value.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for WorkflowSequence {
    type Error = IdentifierError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<WorkflowSequence> for u64 {
    fn from(value: WorkflowSequence) -> Self {
        value.0
    }
}

fn validate_length(
    value: &str,
    kind: &'static str,
    max_bytes: usize,
) -> Result<(), IdentifierError> {
    if value.is_empty() || value.len() > max_bytes {
        return Err(IdentifierError::InvalidLength { kind, max_bytes });
    }
    Ok(())
}

fn valid_workflow_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-'))
        && !value.contains("--")
}

fn valid_stable_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
        && !value.contains("//")
}
