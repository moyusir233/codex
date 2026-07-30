use serde::Deserialize;
use serde::Serialize;

use super::LarkCliError;
use super::error::invalid;

/// Identity used for one Lark CLI operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LarkIdentity {
    User,
    Bot,
}

impl LarkIdentity {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Bot => "bot",
        }
    }
}

macro_rules! prefixed_id {
    ($name:ident, $prefix:literal, $label:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, LarkCliError> {
                let value = value.into();
                let suffix = value.strip_prefix($prefix).unwrap_or_default();
                if suffix.is_empty()
                    || !suffix
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
                {
                    return Err(invalid(concat!("invalid ", $label)));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = LarkCliError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }
    };
}

prefixed_id!(OpenId, "ou_", "open_id");
prefixed_id!(ChatId, "oc_", "chat_id");
prefixed_id!(MessageId, "om_", "message_id");

/// Server-issued Lark thread identity (`om_` root or `omt_` thread).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ThreadId(String);

impl ThreadId {
    pub fn parse(value: impl Into<String>) -> Result<Self, LarkCliError> {
        let value = value.into();
        let suffix = value
            .strip_prefix("om_")
            .or_else(|| value.strip_prefix("omt_"))
            .unwrap_or_default();
        if suffix.is_empty()
            || !suffix
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(invalid("invalid thread_id"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<ThreadId> for String {
    fn from(value: ThreadId) -> Self {
        value.0
    }
}

impl TryFrom<String> for ThreadId {
    type Error = LarkCliError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}
