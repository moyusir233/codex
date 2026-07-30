use crate::catalog::SkillAuthority;
use crate::catalog::SkillCatalog;
use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillPackageId;

/// Stable identity used to match one skill across catalog providers.
///
/// A name or resource path is not sufficient because different authorities can
/// publish skills with the same display name. Matching therefore uses both the
/// opaque authority and package identifiers returned by the owning provider.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

/// Per-thread policy that constrains which catalog skills can be used.
///
/// Hosts attach this value to thread-scoped [`codex_extension_api::ExtensionData`]
/// before the skills extension's thread-start contributor runs. The policy is
/// then fixed for that runtime and applies to prompt rendering, explicit
/// invocation, `skills.list`, and `skills.read` across every source authority.
///
/// This is a capability-selection policy, not a filesystem or process sandbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
        Self::AllowOnly(identities.into_iter().collect())
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
