use semver::Version;

use super::FornaxCliError;

const SUPPORTED_VERSION: &str = "0.0.51";

/// Parsed, pinned `fornax-cli` version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FornaxCliVersion(Version);

impl FornaxCliVersion {
    /// Parses and accepts only the supported `fornax-cli v0.0.51` response.
    pub fn parse(stdout: &[u8]) -> Result<Self, FornaxCliError> {
        let stdout = std::str::from_utf8(stdout).map_err(|_| FornaxCliError::MalformedVersion)?;
        let version = stdout
            .trim()
            .strip_prefix("fornax-cli v")
            .ok_or(FornaxCliError::MalformedVersion)?;
        let parsed = Version::parse(version).map_err(|_| FornaxCliError::MalformedVersion)?;
        if parsed != Version::new(0, 0, 51) {
            return Err(FornaxCliError::UnsupportedVersion {
                actual: parsed.to_string(),
                expected: SUPPORTED_VERSION,
            });
        }
        Ok(Self(parsed))
    }

    /// Returns the normalized semantic version.
    pub fn as_version(&self) -> &Version {
        &self.0
    }
}

/// Capabilities verified for the pinned CLI build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FornaxCliCapabilities {
    /// Prompt reads are supported.
    pub prompt_read: bool,
    /// Full prompt draft save through a private file is supported.
    pub prompt_draft_save: bool,
    /// Skill reads and explicit-directory installation are supported.
    pub skill_read_and_stage: bool,
    /// Trace, span, and trajectory reads are supported.
    pub trace_read: bool,
    /// Trace writes are not present in `fornax-cli v0.0.51`.
    pub trace_write: bool,
}

impl FornaxCliCapabilities {
    /// Returns the fixed capability set for a verified supported version.
    pub fn for_version(_version: &FornaxCliVersion) -> Self {
        Self {
            prompt_read: true,
            prompt_draft_save: true,
            skill_read_and_stage: true,
            trace_read: true,
            trace_write: false,
        }
    }
}
