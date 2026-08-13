# Lark feature workflow implementation contract

Status: local fake-only implementation profile, reconfirmed 2026-08-06.

## Source and runtime baseline

- Harness commit: `fa43925012e611e649268f00ca412556ff2ab21f`.
- Codex commit: `d1c082e008aa13486672171c2286ca9559b44f47`.
- Workflow definition target: `lark-rust-sdk-feature-development@1.0.0`.
- Supported fixture profiles: `lark-cli 1.0.0`, `fornax-cli v0.0.51`, Fornax
  bridge protocol 1, and `bytedance.fornax 1.0.46`.
- Prompt sources are copied into the workflow crate and bound by content hash;
  runtime code must not depend on the planning checkout.

## Nonsecret prerequisites

The launcher must provide an explicit requirement ID/title/description,
immutable or uniquely resolvable participants and approvers, per-gate quorum,
an absolute SDK repository and isolated worktree, branch and immutable base
commit, source/template references, tenant and execution identity, document
parent/group policy, Fornax workspace and delivery policy, a Stage 4 goal
objective with completion evidence, and bounded security/retry/timeout policy.
Credentials are host-owned references and never workflow argument bytes.

Preflight fails before mutation unless the exact adapter profiles, required
capabilities, repository identity, worktree policy, approvers, and credential
references are present. Ambiguous identity, incomplete membership pagination,
stale document revision, unsupported CLI output, missing goal capability, or
missing prompt hash yields an operator disposition rather than a guess.

## Local acceptance policy

Tests use controlled fake processes, temporary SQLite/artifact stores, and a
temporary Git repository. They must not authenticate to Lark or Fornax, change
the SDK target, install dependencies, merge, release, deploy, or delete an
external resource. Accepted business state and remotely delivered telemetry
are separate fields; queued trace delivery is never reported as remote
delivery.

## Deployment inputs intentionally unresolved

- approved Lark tenant, bot/user identity, scopes, parent folder/wiki node,
  owned-group marker, requester allowlist, retention, and cleanup owner;
- sanitized authenticated golden envelopes for every live Lark operation and
  explicit revision/CAS semantics;
- Fornax workspace/target/model, retention/sampling, delivery SLA, and
  evaluator resource IDs;
- whether telemetry delivery is a mandatory progression gate;
- SDK repository/worktree authority, complete governing instruction set, and
  required CI matrix; and
- authorization for one disposable nonproduction canary.

Until these are reviewed, the workflow may be registered only behind the
experimental workflow feature plus an absent-by-default live capability
allowlist.
