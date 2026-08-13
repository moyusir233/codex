# Lark feature workflow rollout

Status: local implementation accepted; live workflow disabled.

The `lark-rust-sdk-feature-development@1.0.0` definition is discoverable, but
the app server does not install its live feature capability. This is the
reviewed release decision for the 2026-08-06 implementation pass. Registration
does not authorize execution: launch fails before a run or external effect is
created when the capability bundle is absent.

## Local release evidence

`eval/lark_feature/release-manifest.json` binds the reviewed source base,
implementation files, bundled prompts, output schemas, dataset, rubrics,
thresholds, scenario fixtures and local results by SHA-256. The deterministic
bundle has 16 scenarios and 19 crash boundaries. It reports 100% contract,
gate, trace, back-edge, normal-holdout and adverse safe-disposition results,
with no critical hallucination, leak, false completion or hard-veto failure.

The final full workflow-extension suite passed 80/80 tests, including 9/9 Lark
contract tests. The app-server registry smoke test passed once, and the named
CLI integration binary passed all 12 tests. All tests used fakes, temporary
repositories and temporary workflow state. No Lark, Fornax or SDK mutation was
performed.

## Why live remains disabled

The selected `lark-cli 1.0.0` fixture profile supports the approved
single-writer revision-aware contract. Workflow-owned documents have one
writer: the current workflow agent. Every update journals its expected revision
and content digest, fetches before dispatch, performs a deterministic
full-content overwrite, and verifies the resulting revision and digest.
Timeouts reconcile by fetch before content-idempotent retry. Any unexpected
drift or mismatch requires an operator. The feature advertises
`lark.documents.single-writer-revision-aware.v1`; this does not claim
multi-writer server CAS.

The following operational inputs are also unresolved: approved test tenant and
identities, bot scopes, owned-resource marker, requester/approver allowlists,
document parent, retention and cleanup owner, SDK worktree authority and CI
matrix, Fornax test workspace/target/model and retention, and authorization for
a disposable canary. Remote prompt/evaluation IDs are intentionally `null` in
the manifest; no local result is represented as remotely delivered.

## Enablement review

Before a host may install the capability, reviewers must:

1. review the exact sanitized Lark compatibility profile and its
   single-writer revision/update ambiguity and reconciliation fixtures;
2. configure a disposable tenant, explicit requester and approver allowlists,
   bounded retention, an owner and a non-destructive cleanup procedure;
3. configure an isolated SDK repository/worktree and verify its complete
   governing instructions and required test matrix;
4. register authorized Fornax test resources, run evaluator-only experiments
   with the documented `--skip-target` restriction, and record returned IDs in
   a new immutable manifest;
5. rerun the complete local M6 command set and review the manifest hashes;
6. obtain separate authorization for one nonproduction canary and follow the
   sanitized procedure in `tests/fixtures/lark_feature/canary/README.md`.

Any version drift, missing capability, identity ambiguity, truncated member
pagination, revision conflict, stale approval, incomplete goal, trace leak or
unreconciled external effect keeps new runs disabled. Rollback disables new
runs while preserving workflow SQLite, artifacts, node threads, goal records,
approval history, Lark correlations and the Fornax bridge journal for repair.
It never auto-deletes external resources.
