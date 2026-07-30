use crate::integrations::process::ProcessError;

/// Typed failures at the `lark-cli` boundary.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LarkCliError {
    #[error("unsupported lark-cli version `{actual}`; expected `1.0.0`")]
    UnsupportedVersion { actual: String },
    #[error("lark-cli returned a malformed version response")]
    MalformedVersion,
    #[error("lark-cli authentication is missing or expired")]
    MissingAuthentication,
    #[error("lark-cli is missing a required scope")]
    MissingScope,
    #[error("lark-cli returned an unsupported JSON response: {message}")]
    InvalidResponse { message: String },
    #[error("invalid Lark request: {message}")]
    InvalidRequest { message: String },
    #[error("sensitive content cannot be passed through lark-cli argv")]
    SensitiveArgv,
    #[error("Lark mutation outcome is ambiguous and requires reconciliation")]
    AmbiguousMutation,
    #[error(transparent)]
    Process(#[from] ProcessError),
}

pub(super) fn classify_process(error: ProcessError) -> LarkCliError {
    let ProcessError::Exit { diagnostic, .. } = &error else {
        return LarkCliError::Process(error);
    };
    let diagnostic = diagnostic.to_ascii_lowercase();
    if diagnostic.contains("permission")
        || diagnostic.contains("scope")
        || diagnostic.contains("99991672")
        || diagnostic.contains("99991679")
    {
        LarkCliError::MissingScope
    } else if diagnostic.contains("auth")
        || diagnostic.contains("login")
        || diagnostic.contains("unauthorized")
        || diagnostic.contains("credential")
    {
        LarkCliError::MissingAuthentication
    } else {
        LarkCliError::Process(error)
    }
}

pub(super) fn invalid(message: impl Into<String>) -> LarkCliError {
    LarkCliError::InvalidRequest {
        message: message.into(),
    }
}
