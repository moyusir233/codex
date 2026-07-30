use codex_skills_extension::ResolvedSkillIdentity;
use codex_skills_extension::SkillIdentity;
use codex_skills_extension::SkillIdentityMatcher;
use codex_skills_extension::SkillIdentityResolutionError;
use codex_skills_extension::SkillVisibilityPolicy;
use codex_skills_extension::catalog::SkillAuthority;
use codex_skills_extension::catalog::SkillCatalog;
use codex_skills_extension::catalog::SkillCatalogEntry;
use codex_skills_extension::catalog::SkillPackageId;
use codex_skills_extension::catalog::SkillResourceId;
use codex_skills_extension::catalog::SkillSourceKind;
use codex_skills_extension::resolve_skill_identities;
use pretty_assertions::assert_eq;

#[test]
fn workflow_visibility_resolves_exact_identities_across_every_authority() {
    let host = authority(SkillSourceKind::Host, "host");
    let executor = authority(SkillSourceKind::Executor, "env-1");
    let orchestrator = authority(SkillSourceKind::Orchestrator, "codex_apps");
    let catalog = SkillCatalog {
        entries: vec![
            entry(&host, "/skills/host/SKILL.md", "shared"),
            entry(&executor, "executor/pkg", "executor-only"),
            entry(&orchestrator, "orchestrator/pkg", "orchestrator-only"),
        ],
        warnings: Vec::new(),
    };
    let matchers = catalog
        .entries
        .iter()
        .map(|entry| SkillIdentityMatcher::Exact {
            identity: SkillIdentity::from(entry),
        })
        .collect::<Vec<_>>();

    let resolved = resolve_skill_identities(&catalog, &matchers).expect("resolve exact identities");

    assert_eq!(
        resolved,
        catalog
            .entries
            .iter()
            .map(|entry| ResolvedSkillIdentity {
                identity: SkillIdentity::from(entry),
                name: entry.name.clone(),
                invocation_path: entry.invocation_path().to_string(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn workflow_visibility_rejects_unknown_ambiguous_and_disabled_matchers() {
    let host = authority(SkillSourceKind::Host, "host");
    let executor = authority(SkillSourceKind::Executor, "env-1");
    let mut disabled = entry(&host, "/skills/disabled/SKILL.md", "disabled");
    disabled.enabled = false;
    let catalog = SkillCatalog {
        entries: vec![
            entry(&host, "/skills/shared/SKILL.md", "shared"),
            entry(&executor, "executor/shared", "shared"),
            disabled,
        ],
        warnings: Vec::new(),
    };

    let ambiguous = resolve_skill_identities(
        &catalog,
        &[SkillIdentityMatcher::DisplayName {
            name: "shared".to_string(),
            authority: None,
        }],
    )
    .expect_err("unscoped duplicate display name must be ambiguous");
    assert!(matches!(
        ambiguous,
        SkillIdentityResolutionError::Ambiguous { matches, .. } if matches.len() == 2
    ));

    let unknown = resolve_skill_identities(
        &catalog,
        &[
            SkillIdentityMatcher::DisplayName {
                name: "disabled".to_string(),
                authority: Some(host.clone()),
            },
            SkillIdentityMatcher::Exact {
                identity: identity(&host, "/skills/missing/SKILL.md"),
            },
        ],
    )
    .expect_err("disabled and missing skills must fail closed");
    assert!(matches!(
        unknown,
        SkillIdentityResolutionError::Unknown { .. }
    ));
}

#[test]
fn workflow_visibility_policy_serializes_stable_opaque_identities() {
    let host = authority(SkillSourceKind::Host, "host");
    let executor = authority(SkillSourceKind::Executor, "env-1");
    let policy = SkillVisibilityPolicy::allow_only([
        identity(&executor, "executor/pkg"),
        identity(&host, "/skills/host/SKILL.md"),
        identity(&executor, "executor/pkg"),
    ]);

    let encoded = serde_json::to_value(&policy).expect("serialize policy");
    assert_eq!(
        encoded,
        serde_json::json!({
            "kind": "allow_only",
            "identities": [
                {
                    "authority": {
                        "kind": {"kind": "host"},
                        "id": "host"
                    },
                    "package": "/skills/host/SKILL.md"
                },
                {
                    "authority": {
                        "kind": {"kind": "executor"},
                        "id": "env-1"
                    },
                    "package": "executor/pkg"
                }
            ]
        })
    );
    assert_eq!(
        serde_json::from_value::<SkillVisibilityPolicy>(encoded)
            .expect("deserialize stable policy"),
        policy
    );
}

fn authority(kind: SkillSourceKind, id: &str) -> SkillAuthority {
    SkillAuthority::new(kind, id)
}

fn identity(authority: &SkillAuthority, package: &str) -> SkillIdentity {
    SkillIdentity::new(authority.clone(), SkillPackageId(package.to_string()))
}

fn entry(authority: &SkillAuthority, package: &str, name: &str) -> SkillCatalogEntry {
    SkillCatalogEntry::new(
        SkillPackageId(package.to_string()),
        authority.clone(),
        name,
        "test skill",
        SkillResourceId::new(package),
    )
}
