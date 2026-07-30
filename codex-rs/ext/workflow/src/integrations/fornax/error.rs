use crate::integrations::process::ProcessError;

/// Typed failures at the `fornax-cli` boundary.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FornaxCliError {
    /// The configured CLI version is not the pinned supported build.
    #[error("unsupported fornax-cli version `{actual}`; expected `{expected}`")]
    UnsupportedVersion {
        /// Observed normalized version.
        actual: String,
        /// Required normalized version.
        expected: &'static str,
    },
    /// Version stdout did not match the documented shape.
    #[error("fornax-cli returned a malformed version response")]
    MalformedVersion,
    /// Required authentication was unavailable.
    #[error("fornax-cli authentication is missing or expired")]
    MissingAuthentication,
    /// Required workspace selection was unavailable.
    #[error("fornax-cli workspace is not configured")]
    MissingWorkspace,
    /// The CLI returned non-JSON or a changed response schema.
    #[error("fornax-cli returned an unsupported JSON response: {message}")]
    InvalidResponse {
        /// Bounded parser/schema message without response payload.
        message: String,
    },
    /// A request violated a typed adapter invariant.
    #[error("invalid Fornax request: {message}")]
    InvalidRequest {
        /// Stable validation message.
        message: String,
    },
    /// A local draft temporary file could not be created or secured.
    #[error("failed to prepare a private prompt draft file: {message}")]
    DraftFile {
        /// Bounded local I/O message.
        message: String,
    },
    /// The installed CLI process failed.
    #[error(transparent)]
    Process(#[from] ProcessError),
}

pub(super) fn classify_process(error: ProcessError) -> FornaxCliError {
    let ProcessError::Exit { diagnostic, .. } = &error else {
        return FornaxCliError::Process(error);
    };
    let diagnostic = diagnostic.to_ascii_lowercase();
    if diagnostic.contains("auth")
        || diagnostic.contains("login")
        || diagnostic.contains("credential")
        || diagnostic.contains("unauthorized")
    {
        FornaxCliError::MissingAuthentication
    } else if diagnostic.contains("workspace") || diagnostic.contains("space id") {
        FornaxCliError::MissingWorkspace
    } else {
        FornaxCliError::Process(error)
    }
}
