# Codex workflow extension

This crate owns the experimental typed workflow API, durable reducer runtime,
external capability adapters, and reference workflows. External mutations must
be planned in `codex-state` before dispatch and reconciled rather than retried
when their outcome is ambiguous.

## Architecture and API

A `Workflow` is a typed reducer over serializable arguments, a versioned
checkpoint, and a typed terminal output. `WorkflowRegistryBuilder` captures
Clap help and JSON schemas at startup. `WorkflowDriver` leases one run, invokes
one reducer version, and atomically commits a continue, wait, completion, or
failure transition. Workflow code receives only `WorkflowContext`; raw
credentials, databases, process launchers, and thread managers are deliberately
not public capabilities.

Every changing checkpoint representation increments
`WorkflowState::SCHEMA_VERSION`. Registry name/version selection and the state
schema version are separate: the former chooses code, while the latter guards
persisted data. Duplicate versions, duplicate defaults, and definitions without
a default fail registry construction.

Normal Codex nodes remain ordinary persisted threads. `NodeClient::ensure`
materializes a stable node key exactly once, persists its full `NodeSpec`, and
returns a `NodeHandle` whose thread ID can be resumed by the normal
`thread/resume` API or `codex resume`. Nodes can form chains and fan-out/fan-in
graphs through persisted dependencies. A node's explicit `SkillPolicy` is an
allow-list, not a filesystem sandbox.

## Lifecycle and persistence

Run, node, attempt, effect, interaction, external correlation, and event rows
live in `workflows_1.sqlite`. Immutable local artifacts live below
`$CODEX_HOME/workflows/artifacts/<run-id>/` with mode-restricted directories,
atomic installation, SHA-256 manifests, and normalized relative paths.

Reducer steps are replayable. Before an external mutation, its capability must
persist an effect derived from a stable `EffectKey`; after an uncertain
response, it reconciles by provider correlation rather than issuing the
mutation again. Startup recovery repairs known thread mappings, reconciles
terminal attempts, polls durable human waits, and marks ambiguous or orphaned
work as `needs_operator`.

Cancellation is cooperative at reducer/process boundaries and durable at the
run boundary. Dropping a `NodeHandle` does not archive or delete its thread.
Archive and deletion require their dedicated explicit confirmation paths.

## Registration

`default_registry()` contains the experimental `prompt-review@1.0.0` reference
definition. The app server installs that registry once at process composition;
the CLI uses the same in-process app server, so there is no second definition
path.

```rust,ignore
let registry = codex_workflow_extension::default_registry()?;
let prompt_review = registry.resolve(
    &codex_workflow_extension::WorkflowName::new("prompt-review")?,
    None,
)?;
assert_eq!(prompt_review.metadata().version().to_string(), "1.0.0");
```

New definitions should be registered only in `default_registry`, declare every
required capability, use a new semantic version for behavioral incompatibility,
and keep an explicitly selected default.

## `prompt-review`

The reference reducer has four durable phases:

1. preflight exact Fornax/Lark versions, auth, live trace approval, and artifact
   storage; retrieve/render the prompt and begin a correlated trace;
2. run a planner, three bounded reviewers with distinct explicit skill
   allow-lists, and one fan-in synthesizer on resumable Codex threads;
3. reconcile the target Lark resource, send a non-sensitive correlated request,
   and wait on one allowed reply without holding a worker;
4. submit that reply to the existing synthesizer thread, save a complete Fornax
   draft and immutable local artifacts, finish spans, and return every thread,
   trace, document, artifact, and resume correlation.

The reducer depends on a narrow `PromptReviewCapability`. A production host
must install this capability only after the whole bundle passes preflight.
Tests replace it with a deterministic fake and exercise the same reducer,
including a process restart while waiting for the human reply. An absent bundle
causes `workflow/run` to fail before a run, node, or external effect is created.

Example discovery and launch:

```text
codex workflow --list
codex workflow prompt-review \
  --prompt-key example.prompt \
  --prompt-version 7 \
  --reviewers 3 \
  --lark-users ou_reviewer_a,ou_reviewer_b \
  --lark-chat-id oc_disposable_review
```

The launch is intentionally unavailable until the host supplies all advertised
capabilities. `--non-interactive` does not grant live Fornax or Lark approval.
Use `--json` for NDJSON automation and `--detach` only with a persistent remote
app server. Resume an operator-owned run with
`codex workflow --resume-run <run-id>` and resume any returned node with
`codex resume <thread-id>`.

## Fornax

The Fornax integration has two separately versioned boundaries:

- `fornax-cli v0.0.51` provides typed prompt reads/full-draft saves, skill
  reads/staging, and trace reads through a controlled process runner.
- The separately installed `codex-fornax-trace-bridge 0.1.0` wheel provides
  authenticated loopback trace writes through pinned `bytedance.fornax
  1.0.46`. Codex never installs the private wheel or SDK.

Rust invokes only the lifecycle CLI with structured arguments. Trace
start/record/finish/status calls use the protocol-v1 typed HTTP client, with
proxies and redirects disabled and an exact `127.0.0.1` endpoint. Credentials,
opaque SDK header maps, and input/output content are excluded from normal
responses and workflow state. Workflow persistence keeps operation, span
handle, trace context, trace, span, and bridge-instance IDs.

Fornax trace writing remains experimental. Preflight requires the exact bridge,
protocol, and SDK versions, authenticated health, secure descriptor files, and
an explicit live-delivery approval. That approval remains false until a
finished/flushed disposable trace is retrieved by ID with `fornax-cli`.
Prompt/skill/trace-read support is unaffected when the companion is absent.

## Lark and human waits

The adapter accepts exactly `lark-cli 1.0.0`. Document/chat/message operations
use bot identity and require configured scopes and explicit authorization for
live writes. Message text is passed through argv by the upstream CLI, so
sensitive content is rejected before spawn and allowed content is registered
for child-diagnostic redaction.

Human requests persist an effect, an interaction, a Lark correlation token,
message/event dedupe keys, and polling cursors before sending. One process-wide
subscriber runs without `--force`; independent waiters cannot cancel the shared
stream. Reconnect gaps are covered by bounded chat-history polling. Only the
configured chat/thread, correlation token, sender allow-list, and deadline can
resolve a request, and event plus message IDs deduplicate stream/poll overlap.
The reply is stored as an internal artifact before the exact waiting run wakes.

## Errors, recovery, and security

- `capability_unavailable`: install nothing automatically. Verify exact
  binaries/SDK/protocol, descriptor permissions, live approval, and auth.
- `workflow_step_failed`: inspect sequenced run events and persisted effects.
  An ambiguous external effect must be reconciled or escalated, never replayed.
- `needs_operator`: preserve the database, bridge journal, artifacts, and node
  rollouts. Repair the specific missing/ambiguous correlation and explicitly
  resume the run.
- timeout or cancellation: node turns and child processes are interrupted
  cooperatively; created external resources and normal threads are retained and
  reported.
- version drift or malformed/oversized output: the typed process adapter fails
  closed. Do not broaden output limits or accept unknown versions at runtime.

Secrets, tokens, prompt bodies, Lark messages, bridge headers, and raw model
outputs must not enter checkpoints, effect metadata, normal logs, or trace
tags. Store content in classified artifacts and put only digests/opaque refs in
telemetry. External IDs are correlations, not proof of provider-native artifact
upload. The bridge is loopback-only, refuses proxies/redirects, authenticates
every request, and requires private descriptor/journal files.

A binary predating persisted `SkillVisibilityPolicy` is not a safe rollback
for constrained nodes. Disable the experimental Workflows feature to stop new
runs and recovery effects, but retain `workflows_1.sqlite`, artifacts, bridge
journal, and rollouts for repair/resume.

## Local testing

From the repository root:

```text
just test -p codex-workflow-extension prompt_review_e2e
just test -p codex-app-server workflow_example
just test -p codex-cli workflow_cli
just test -p codex-workflow-extension compile_api
just fmt-check
```

The E2E uses a temporary Codex home and fake capability/model boundary. It
asserts chain/fan-out correlations, waits durably, restarts, accepts one reply,
follows up on the existing synthesizer thread, and returns resume commands.

Live testing is opt-in and non-CI. Use only a disposable approved Fornax
workspace, configured Lark test app/chat, consenting recipients, and test model
credentials. First run the documented adapter dry-runs and reads; then authorize
the single mutation explicitly. Never auto-delete live resources. On failure,
report run/thread/trace/span/document/message IDs and preserve all journals.
