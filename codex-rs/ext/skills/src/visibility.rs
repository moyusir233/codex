use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::catalog::SkillAuthority;
use crate::catalog::SkillCatalog;
use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillPackageId;

/// Stable identity used to match one skill across catalog providers.
///
/// A name or resource path is not sufficient because different authorities can
/// publish skills with the same display name. Matching therefore uses both the
/// opaque authority and package identifiers returned by the owning provider.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct SkillIdentity {
    /// Authority that owns and reads the skill package.
    pub authority: SkillAuthority,
    /// Opaque package identifier within the authority.
    pub package: SkillPackageId,
}

impl SkillIdentity {
    /// Creates an identity from the exact authority and package handles exposed
    /// by a skill catalog entry.
    pub fn new(authority: SkillAuthority, package: SkillPackageId) -> Self {
        Self { authority, package }
    }
}

impl From<&SkillCatalogEntry> for SkillIdentity {
    fn from(entry: &SkillCatalogEntry) -> Self {
        Self::new(entry.authority.clone(), entry.id.clone())
    }
}

/// Author-facing matcher resolved against one merged catalog.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SkillIdentityMatcher {
    /// Match one exact provider-owned opaque identity.
    Exact { identity: SkillIdentity },
    /// Match one display name, optionally scoped to an exact authority.
    DisplayName {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authority: Option<SkillAuthority>,
    },
}

/// Exact identity plus presentation fields persisted after resolution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedSkillIdentity {
    pub identity: SkillIdentity,
    pub name: String,
    pub invocation_path: String,
}

/// Per-thread policy that constrains which catalog skills can be used.
///
/// Hosts attach this value to thread-scoped [`codex_extension_api::ExtensionData`]
/// before the skills extension's thread-start contributor runs. The policy is
/// then fixed for that runtime and applies to prompt rendering, explicit
/// invocation, `skills.list`, and `skills.read` across every source authority.
///
/// This is a capability-selection policy, not a filesystem or process sandbox.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind", content = "identities")]
pub enum SkillVisibilityPolicy {
    /// Preserve the existing catalog behavior.
    #[default]
    AllowAll,
    /// Disable every skill while retaining entries for diagnostics.
    DisableAll,
    /// Enable only the exact authority/package identities in this list.
    AllowOnly(Vec<SkillIdentity>),
}

impl SkillVisibilityPolicy {
    /// Creates an exact allow-list from provider-owned skill identities.
    pub fn allow_only(identities: impl IntoIterator<Item = SkillIdentity>) -> Self {
        let mut identities = identities.into_iter().collect::<Vec<_>>();
        identities.sort();
        identities.dedup();
        Self::AllowOnly(identities)
    }

    /// Returns whether this policy permits the exact skill identity.
    pub fn allows(&self, authority: &SkillAuthority, package: &SkillPackageId) -> bool {
        match self {
            Self::AllowAll => true,
            Self::DisableAll => false,
            Self::AllowOnly(identities) => identities
                .iter()
                .any(|identity| identity.authority == *authority && identity.package == *package),
        }
    }

    pub(crate) fn apply(&self, catalog: &mut SkillCatalog) {
        for entry in &mut catalog.entries {
            entry.enabled &= self.allows(&entry.authority, &entry.id);
        }
    }
}

/// Resolves matchers against enabled entries in one already-merged catalog.
pub fn resolve_skill_identities(
    catalog: &SkillCatalog,
    matchers: &[SkillIdentityMatcher],
) -> Result<Vec<ResolvedSkillIdentity>, SkillIdentityResolutionError> {
    let mut resolved = Vec::with_capacity(matchers.len());
    for matcher in matchers {
        let matches = catalog
            .entries
            .iter()
            .filter(|entry| entry.enabled && matcher.matches(entry))
            .collect::<Vec<_>>();
        let entry = match matches.as_slice() {
            [] => {
                return Err(SkillIdentityResolutionError::Unknown {
                    matcher: matcher.clone(),
                });
            }
            [entry] => *entry,
            _ => {
                return Err(SkillIdentityResolutionError::Ambiguous {
                    matcher: matcher.clone(),
                    matches: matches.into_iter().map(SkillIdentity::from).collect(),
                });
            }
        };
        let candidate = ResolvedSkillIdentity {
            identity: SkillIdentity::from(entry),
            name: entry.name.clone(),
            invocation_path: entry.invocation_path().to_string(),
        };
        if !resolved
            .iter()
            .any(|existing: &ResolvedSkillIdentity| existing.identity == candidate.identity)
        {
            resolved.push(candidate);
        }
    }
    Ok(resolved)
}

impl SkillIdentityMatcher {
    fn matches(&self, entry: &SkillCatalogEntry) -> bool {
        match self {
            Self::Exact { identity } => {
                identity.authority == entry.authority && identity.package == entry.id
            }
            Self::DisplayName { name, authority } => {
                entry.name == *name
                    && authority
                        .as_ref()
                        .is_none_or(|authority| authority == &entry.authority)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillIdentityResolutionError {
    Unknown {
        matcher: SkillIdentityMatcher,
    },
    Ambiguous {
        matcher: SkillIdentityMatcher,
        matches: Vec<SkillIdentity>,
    },
}

impl std::fmt::Display for SkillIdentityResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown { .. } => {
                formatter.write_str("skill matcher did not resolve to an enabled catalog entry")
            }
            Self::Ambiguous { .. } => {
                formatter.write_str("skill matcher resolved to multiple enabled catalog entries")
            }
        }
    }
}

impl std::error::Error for SkillIdentityResolutionError {}
