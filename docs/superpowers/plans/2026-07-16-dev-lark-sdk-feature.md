# `dev-lark-sdk-feature` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one production Codex workflow, invokable as `/workflow dev-lark-sdk-feature --developers <open-ids> [--artifacts-dir <path>]` in the TUI or `codex workflow dev-lark-sdk-feature --developers <open-ids> [--artifacts-dir <path>]`, that validates the Lark Rust SDK repository, obtains one explicit external-effect approval, conducts a bot-mentioned Lark requirements/design discussion through real Codex child threads, waits for digest-bound technical-design approval, runs an implementation child, and persists crash-safe progress without real Lark writes in automated tests.

**Architecture:** A focused `codex-dev-lark-sdk-feature` crate owns workflow-specific parsing, validation, persistence, Lark process I/O, prompts, and stage transitions. App-server owns the live manager, root/child adapters, turn tracker, leases, RPC routing, and notification projection. `codex-core` exposes only narrow managed-child lifecycle APIs that delegate to the existing agent-control path. TUI and CLI are protocol clients and share the same invocation parser and workflow RPCs.

**Tech Stack:** Rust 1.95, Tokio, Clap, Serde/serde_json, standard-library advisory file locking through `std::fs::File::try_lock`, UUIDv7, SHA-256, app-server v2 JSON-RPC, `tokio::process::Command`, `CancellationToken`, Insta snapshots, Wiremock/core test support, Cargo Nextest through `just test`, and Bazel/Rust crate metadata.

## Global Constraints

- Work in an isolated worktree created with `superpowers:using-git-worktrees` before Task 1. Preserve the current clean/dirty state of both Codex and the target SDK; never reset unrelated changes.
- Invoke `superpowers:test-driven-development` before Task 1 and retain its RED-GREEN-REFACTOR discipline for every implementation task. On any unexpected failure, invoke `superpowers:systematic-debugging` before changing production code; record the root cause and the focused evidence that proves the fix.
- Read every applicable `AGENTS.md` again in the execution worktree before editing its scope. Follow `codex/AGENTS.md`, TUI-scoped instructions, and SDK instructions. Re-read `/Users/bytedance/project/harness-engine/docs/codex-dev-lesson/openwiki/{codex-cli-conversation-flow,codex-cli-conversation-code-map,codex-cli-conversation-type-map}.md` and `/Users/bytedance/project/harness-engine/docs/codex-dev-lesson/lessons/0004-build-a-custom-workflow.html` before Task 1; record any drift from current code.
- The approved design plus the listener/ownership/shutdown safety amendments at `docs/superpowers/specs/2026-07-16-dev-lark-sdk-feature-design.md` become the normative contract when this plan is approved. If implementation evidence contradicts it, stop and amend/re-approve the design rather than silently changing semantics.
- All production Lark operations use bot identity, argv arrays, JSON, bounded output/time, cancellation-aware process control, and an injected fake in tests. Automated tests must never invoke a real Lark write. The user's example `lark-cli im +chat-create --users ... --as bot` is manual evidence only, not a test fixture or automated command.
- Repository validation completes before artifact creation, thread creation, group creation, or messaging. `workflow/prepare` is read-only. No public resume command is added; explicit retrigger is the only recovery entry.
- The first authorized, structured-bot-mentioned ordinary Lark message is the requirement and the research child's initial turn. `/finish` cannot substitute for it. Implementation waits for a structured `/approve-design` whose stored SHA-256 matches the current design artifact.
- App-server remains the sole consumer of `CodexThread::next_event`; the workflow waits through `WorkflowTurnTracker`. Do not copy `submission_loop`, `user_input_or_turn_inner`, or `run_turn`.
- `RunStateCommitter::commit_transition` is the sole writer for manifest stage/status/sequence/presentation/journal fields. Stage, failure, cancellation, and completion paths may publish only its immutable committed snapshot, except the explicitly marked non-durable terminal-write-failure notification.
- Keep `runId`, preparation ID, app-server request ID, root/child thread IDs, turn/submission IDs, model response IDs, tool-call IDs, and Lark chat/message IDs in distinct types/fields.
- Keep new production modules near or below 500 lines. Put unit tests in sibling `*_tests.rs` modules. Use `apply_patch` for source edits.
- Prefix every execution command with RTK as required by `/Users/bytedance/.codex/RTK.md`: use `rtk git ...`, `rtk cargo ...`, and `rtk proxy just ...` / `rtk proxy bazel ...`. Short `just`, `cargo`, `git`, and `bazel` spellings below name the repository command; the executed form must include that RTK prefix.
- Run focused tests after every task. Use `just test`, never direct `cargo test`. Use `just fmt` after Rust edits and `just fix -p <crate>` only when the repository instructions require it.
- Commit each completed task independently so reviews and rollback remain bounded.

---

## File and Ownership Map

### New workflow crate

- `codex-rs/dev-lark-sdk-feature/Cargo.toml`, `BUILD.bazel`, `src/lib.rs`: crate surface and Bazel/Cargo registration.
- `src/args.rs`, `args_tests.rs`, `protocol_adapter.rs`: shared Clap invocation, normalized developer list, and the single domain/v2 DTO conversion boundary used by TUI, CLI, and app-server.
- `src/validation.rs`, `validation_tests.rs`: repository identity and artifact-path safety.
- `src/model.rs`: IDs, stages, statuses, errors, snapshots, and feature-specific host contracts.
- `src/store.rs`, `store_tests.rs`: manifest, start index, dual locks, atomic durability, recovery checkpoints.
- `src/transition.rs`, `transition_tests.rs`: the sole per-run serialized manifest transition writer and committed-update snapshots.
- `src/lark/runner.rs`, `runner_tests.rs`: injected process runner and safe diagnostics.
- `src/lark/client.rs`, `client_tests.rs`: auth, chat, membership, poll, and send DTO/argv mappings.
- `src/lark/inbox.rs`, `inbox_tests.rs`: ordering, mention/sender filtering, cursor, command recognition, and checkpoints.
- `src/lark/outbox.rs`, `outbox_tests.rs`: chunking, stable references, ambiguous-send reconciliation, and idempotency.
- `src/prompts.rs`, `prompts_tests.rs`, `prompts/*.md`: six embedded optimized templates and delimiter-safe rendering.
- `src/runtime/mod.rs`: small coordinator that composes initialization, stages, recovery, cancellation, and terminal persistence without owning their detailed logic.
- `src/runtime/{initialization,recovery,cancellation,terminal}.rs` and sibling `*_tests.rs`: one bounded responsibility per module; no catch-all `runtime.rs`.
- `src/stages/{research,design,implementation}.rs` and sibling tests: bounded stage logic.
- `tests/workflow_integration.rs`: fake-Lark/fake-host stage integration.

### Existing crates

- `codex-rs/Cargo.toml`, `Cargo.lock`: workspace member/dependency registration.
- `codex-rs/core/src/{thread_manager.rs,codex_thread.rs}` plus sibling tests: public correlated-root and two-phase managed-child create/submit façade, read-only workflow correlation lookup, and conditional interrupt façade.
- `codex-rs/core/src/agent/control/spawn.rs` plus tests: delegate to existing spawn bookkeeping while preserving stable initial client ID and correlation.
- `codex-rs/protocol/src/protocol.rs`: schema-compatible optional root/child workflow correlation in `SessionMeta`, plus existing `ThreadSpawn` lineage.
- `codex-rs/rollout/src/recorder.rs`, `codex-rs/thread-store/src/{types.rs,local/create_thread.rs,in_memory.rs}`, and core session creation: atomically carry correlation into the first durable `SessionMeta`.
- `codex-rs/state/src/{extract.rs,migrations.rs,model/thread_metadata.rs,runtime/threads.rs}`, migration `0041_threads_workflow_correlation.sql`, and sibling tests: project/query durable root and child correlation.
- `codex-rs/app-server-protocol/src/protocol/v2/{mod.rs,workflow.rs,workflow_tests.rs}` and `protocol/common.rs`: v2 DTOs, request/response unions, notifications, serialization scopes.
- `codex-rs/app-server/src/workflow/{mod.rs,manager.rs,preparation_registry.rs,run_registry.rs,progress_sink.rs,shutdown.rs,host.rs,turn_tracker.rs,thread_leases.rs,mutation_policy.rs}` plus sibling tests: live ownership, committed-update projection, shutdown coordination, lease-owned listener readiness, adapters, and centralized workflow-child mutation policy.
- `codex-rs/app-server/src/{message_processor.rs,request_processors.rs,bespoke_event_handling.rs}` and `request_processors/thread_lifecycle.rs`: RPC routing, sole-listener tracker feed, and lease-aware unload.
- `codex-rs/app-server/src/in_process.rs` and `codex-rs/app-server-client/src/lib.rs`: classify completion as required-delivery in both real event pumps.
- `codex-rs/app-server/tests/suite/v2/workflow.rs`, `suite/v2/mod.rs`: protocol and end-to-end app-server tests.
- Generated `codex-rs/app-server-protocol/schema/json/**` and `schema/typescript/**`: refreshed only with `just write-app-server-schema`.
- `codex-rs/tui/src/workflow/{mod.rs,state.rs,controller.rs,render.rs}` plus sibling tests: focused confirmation/RPC state, progress/read repair, cancellation, and presentation logic. `slash_command.rs`, `app_server_session.rs`, `app.rs`, `chatwidget.rs`, and `chatwidget/slash_dispatch.rs` remain thin integration points with existing snapshots.
- `codex-rs/cli/src/{main.rs,workflow_cmd.rs,workflow_cmd_tests.rs}` and `codex-rs/cli/Cargo.toml`: direct CLI client, TTY confirmation, progress, Ctrl+C, and final output.
- `codex-rs/app-server/Cargo.toml`, `codex-rs/tui/Cargo.toml`, `codex-rs/cli/Cargo.toml`: focused dependencies.
- `codex-rs/app-server/README.md`: required app-server v2 API documentation.
- `/Users/bytedance/project/harness-engine/docs/codex-dev-lesson/openwiki/*.md` and `lessons/0004-build-a-custom-workflow.html`: targeted verified-map updates only where the new APIs make current lesson evidence stale.

---

## Task 1: Add the app-server v2 workflow contract

**Files:**

- Create: `codex-rs/app-server-protocol/src/protocol/v2/workflow.rs`
- Create: `codex-rs/app-server-protocol/src/protocol/v2/workflow_tests.rs`
- Modify: `codex-rs/app-server-protocol/Cargo.toml`
- Modify: `codex-rs/app-server-protocol/src/protocol/v2/mod.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/common.rs`
- Modify after green: generated `codex-rs/app-server-protocol/schema/json/**`, `schema/typescript/**`

- [ ] **Write failing DTO and wire-format tests.** Cover camelCase JSON, nullable fields, tagged `WorkflowInvocation`, tagged cancel target, preparation/start/read/cancel request/response round trips, progress/completion notifications, integer timestamps, distinct ID fields, and serialization scopes. Define a bounded `WorkflowStagePresentation { assistantOutput, artifactPaths }`. `WorkflowReadResponse` exposes both the retained transition journal and a fixed three-stage `stagePresentations` snapshot so research/design/implementation results remain repairable after the 256-transition journal truncates; a completion transition may repeat the same presentation for live rendering. Neither copy enters the parent rollout. Give prepare a `deny_unknown_fields` tagged target enum (`ExistingParent { parentThreadId }` or `NewRoot { cwd, threadStartOptions }`) so both/neither cannot be represented by typed clients; also test malicious JSON with both/neither is rejected. The public shape starts with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowInvocation {
    DevLarkSdkFeature {
        #[serde(flatten)]
        args: DevLarkSdkFeatureWorkflowArgs,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowCancelTarget {
    Preparation { preparation_id: String },
    Run { run_id: String },
}
```

- [ ] **Run the RED test.** `just test -p codex-app-server-protocol workflow_tests` must fail because the module/variants do not exist.
- [ ] **Implement DTOs and macro registrations.** Add `WorkflowPrepare`, `WorkflowStart`, `WorkflowRead`, and `WorkflowCancel` to the client-request macro; add `WorkflowProgress` and `WorkflowCompleted` to the notification macro. Add `WorkflowPreparation { preparation_hash: String }` and `WorkflowRun { run_id: String }` to `ClientRequestSerializationScope`; use the workspace `sha2` dependency to avoid retaining the bearer token in scope keys. Start/preparation cancel use the preparation hash, while read/run cancel use the run ID:

```rust
WorkflowPrepare => "workflow/prepare" {
    params: v2::WorkflowPrepareParams,
    serialization: None,
    response: v2::WorkflowPrepareResponse,
},
WorkflowStart => "workflow/start" {
    params: v2::WorkflowStartParams,
    serialization: workflow_preparation(params.preparation_id),
    response: v2::WorkflowStartResponse,
},
WorkflowRead => "workflow/read" {
    params: v2::WorkflowReadParams,
    serialization: workflow_run_or_thread(params.run_id, params.parent_thread_id),
    response: v2::WorkflowReadResponse,
},
WorkflowCancel => "workflow/cancel" {
    params: v2::WorkflowCancelParams,
    serialization: workflow_cancel_target(params.target),
    response: v2::WorkflowCancelResponse,
},
```

  Add matching `serialization_scope_expr!` arms: SHA-256 preparation scope; run scope when read has `runId`; thread scope when read omits it; and target-dependent preparation/run scope for cancel. Register notifications explicitly as `WorkflowProgress => "workflow/progress" (...)` and `WorkflowCompleted => "workflow/completed" (...)`; do not rely on macro-derived names.

- [ ] **Run the GREEN test.** `just test -p codex-app-server-protocol workflow_tests` must pass.
- [ ] **Generate and verify schemas.** Run `just write-app-server-schema`; inspect generated JSON and TypeScript for all four methods and both notifications; run `just test -p codex-app-server-protocol schema_fixtures`.
- [ ] **Update dependency locks.** Run `just bazel-lock-update`, inspect `Cargo.lock`/`MODULE.bazel.lock`, and retain only changes caused by the protocol's `sha2` dependency.
- [ ] **Commit.** `git add codex-rs/app-server-protocol codex-rs/Cargo.lock MODULE.bazel.lock && git commit -m 'feat(app-server): define workflow v2 protocol'`.

## Task 2: Create the focused crate and shared typed invocation parser

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/{Cargo.toml,BUILD.bazel}`
- Create: `codex-rs/dev-lark-sdk-feature/src/{lib.rs,args.rs,args_tests.rs,model.rs,protocol_adapter.rs}`
- Modify: `codex-rs/Cargo.toml`, `codex-rs/Cargo.lock`

- [ ] **Write parser tests first.** Cover the exact success syntax, default `dev-lark-sdk-feature` artifact directory, ASCII-only surrounding whitespace trimming with order preservation, rejection of non-ASCII whitespace inside an identifier, required `--developers`, empty elements, malformed IDs, duplicates, more than 50 IDs, unknown flags, and TUI/CLI argv parity.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature args_tests` must fail because the crate is not registered.
- [ ] **Register the crate.** Add workspace member/dependency entries and create these exact crate declarations; remove dependencies during implementation if the compiler proves they are unused:

```toml
[package]
name = "codex-dev-lark-sdk-feature"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "codex_dev_lark_sdk_feature"
path = "src/lib.rs"

[lints]
workspace = true

[dependencies]
clap = { workspace = true, features = ["derive"] }
codex-app-server-protocol = { workspace = true }
codex-protocol = { workspace = true }
futures = { workspace = true }
rand = { workspace = true }
regex = { workspace = true }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
sha2 = { workspace = true }
thiserror = { workspace = true }
time = { workspace = true, features = ["formatting", "parsing", "serde"] }
tokio = { workspace = true, features = ["io-util", "macros", "process", "rt", "sync", "time"] }
tokio-util = { workspace = true, features = ["rt"] }
toml = { workspace = true }
uuid = { workspace = true, features = ["serde", "v7"] }

[dev-dependencies]
assert_matches = { workspace = true }
ctor = { workspace = true }
insta = { workspace = true, features = ["json"] }
pretty_assertions = { workspace = true }
tempfile = { workspace = true }
```

```starlark
load("//:defs.bzl", "codex_rust_crate")

codex_rust_crate(
    name = "dev-lark-sdk-feature",
    crate_name = "codex_dev_lark_sdk_feature",
)
```

- [ ] **Implement the shared parser.** Do not accept an untyped trailing string below this boundary:

```rust
#[derive(Debug, Clone, clap::Args, PartialEq, Eq)]
pub struct WorkflowInvocationArgs {
    #[command(subcommand)]
    pub workflow: WorkflowCommand,
}

#[derive(Debug, Clone, clap::Subcommand, PartialEq, Eq)]
pub enum WorkflowCommand {
    DevLarkSdkFeature(DevLarkSdkFeatureArgs),
}

#[derive(Debug, Clone, clap::Args, PartialEq, Eq)]
pub struct DevLarkSdkFeatureArgs {
    #[arg(long, value_parser = parse_developers)]
    pub developers: DeveloperOpenIds,
    #[arg(long, default_value = "dev-lark-sdk-feature")]
    pub artifacts_dir: PathBuf,
}

#[derive(Debug, clap::Parser)]
#[command(name = "workflow", disable_help_subcommand = true)]
struct StandaloneWorkflowParser {
    #[command(flatten)]
    invocation: WorkflowInvocationArgs,
}

impl WorkflowInvocationArgs {
    /// Parses tokens after `/workflow` or `codex workflow`; no program name is accepted.
    pub fn parse_tokens<I, T>(tokens: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString>,
    {
        let argv = std::iter::once(OsString::from("workflow"))
            .chain(tokens.into_iter().map(Into::into));
        StandaloneWorkflowParser::try_parse_from(argv).map(|parsed| parsed.invocation)
    }
}
```

  `DeveloperOpenIds` trims with `trim_matches(|c: char| c.is_ascii_whitespace())`, validates `^ou_[A-Za-z0-9]{1,128}$`, rejects duplicates using a set, retains input order in a vector, and caps at 50. Tests pass the exact TUI remainder beginning with `dev-lark-sdk-feature` and prove the synthetic program name prevents Clap from consuming that subcommand as argv[0].
- [ ] **Add one explicit protocol conversion boundary.** `protocol_adapter.rs` exposes `build_prepare_params(target, parsed, client_kind) -> WorkflowPrepareParams` and `TryFrom<&protocol::WorkflowInvocation> for DevLarkSdkFeatureArgs`. TUI and CLI call the same builder; app-server uses the reverse conversion before repeating semantic validation. Unit tests construct TUI-style shlex argv and direct-CLI argv, then assert equality of the complete `WorkflowPrepareParams`, not only equality of parser structs. Protocol DTOs remain owned by `codex-app-server-protocol`; runtime locks/threads/stores never enter them.
- [ ] **Run GREEN.** `just test -p codex-dev-lark-sdk-feature args_tests` must pass.
- [ ] **Check the workspace.** `cargo check -p codex-dev-lark-sdk-feature` and `just fmt-check` must pass.
- [ ] **Update Bazel dependency lock.** Run `just bazel-lock-update`, inspect `MODULE.bazel.lock`, and include only dependency-lock changes caused by the new crate/dependencies.
- [ ] **Commit.** `git add codex-rs/Cargo.toml codex-rs/Cargo.lock codex-rs/dev-lark-sdk-feature MODULE.bazel.lock && git commit -m 'feat(workflow): add typed dev lark invocation'`.

## Task 3: Enforce repository identity and artifact-path safety before side effects

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/validation.rs`
- Create: `codex-rs/dev-lark-sdk-feature/src/validation_tests.rs`
- Modify: `codex-rs/dev-lark-sdk-feature/src/lib.rs`
- Modify: `codex-rs/dev-lark-sdk-feature/src/model.rs`

- [ ] **Write RED tests with temporary repositories.** Assert canonicalization and stable markers (`Cargo.toml` packages/members `lark-integrator`, `lark-api-chat`, `lark-biz-chat`, root `AGENTS.md`, `.ai_knowledge/architecture.md`). Assert wrong basename-only repo, missing/inconsistent marker, absolute/empty/traversal paths, existing escaping symlink, missing descendants below an escaping symlink, and a valid not-yet-created descendant. Assert an injected side-effect recorder remains empty on every validation failure.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature validation_tests` must fail on missing validator/types.
- [ ] **Implement typed validation.** The only constructor for runtime config performs repository validation first and path validation without creating anything:

```rust
pub fn validate_invocation(
    cwd: &Path,
    args: DevLarkSdkFeatureArgs,
) -> Result<DevLarkSdkFeatureConfig, WorkflowValidationError> {
    let repository = validate_sdk_repository(cwd)?;
    let artifact_dir = validate_artifact_path(&repository, &args.artifacts_dir)?;
    Ok(DevLarkSdkFeatureConfig {
        repository,
        developers: args.developers,
        artifact_dir,
    })
}
```

  Walk to the deepest existing ancestor with `symlink_metadata`, canonicalize it, require it to remain below the canonical root, then validate the non-existing suffix component-by-component. Revalidate after eventual directory creation.
- [ ] **Run GREEN.** `just test -p codex-dev-lark-sdk-feature validation_tests` must pass, including Unix symlink cases behind `#[cfg(unix)]`.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): validate sdk repository and artifacts'`.

## Task 4: Add versioned, atomic workflow persistence and dual locking

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/store.rs`
- Create: `codex-rs/dev-lark-sdk-feature/src/store_tests.rs`
- Create: `codex-rs/dev-lark-sdk-feature/src/transition.rs`
- Create: `codex-rs/dev-lark-sdk-feature/src/transition_tests.rs`
- Modify: `codex-rs/dev-lark-sdk-feature/src/{lib.rs,model.rs}`

- [ ] **Write RED persistence and transition tests.** Cover the exact artifact layout, schema version 1, UUIDv7 run IDs, non-secret approval record, start-index hash (never raw token), `current.json`, `chat_id.txt`, root/child intent-before-effect checkpoints, cursor/inbox/outbound/design-digest fields, opportunistic removal of terminal start indices after 24 hours, atomic replace failpoints, parent-directory sync behavior where supported, artifact lock, parent-thread lock, duplicate active start, stale-file acquisition, completed/cancelled retrigger, and failed/interrupted recovery. Serialize sentinel access/refresh tokens, raw stderr, and authorization URLs with query/fragment through every failure path and assert none appears in the manifest, start index, failure record, prompt audit, or other artifact; a separately classified query-free non-secret developer-console location may appear only in a bounded safe diagnostic. For `RunStateCommitter`, race stage completion against cancellation/failure and require one monotonic order; each commit atomically updates current state, sequence, fixed three-stage presentation map, capped 256-entry journal, and manifest before returning `CommittedWorkflowUpdate`. Inject a write failure and prove no in-memory state swap or durable notification snapshot escapes.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature store_tests` must fail.
- [ ] **Implement the durable model and store.** Use typed IDs/wrappers and exhaustive stage/status enums:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowInvocationOwner {
    ExistingParent { parent_thread_id: ThreadId },
    WorkflowCreatedRoot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StagePresentation {
    pub assistant_output: String,
    pub artifact_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowManifest {
    pub schema_version: u32,
    pub workflow_name: String,
    pub run_id: WorkflowRunId,
    pub canonical_repository: PathBuf,
    pub artifact_dir: PathBuf,
    pub invocation_digest: Sha256Digest,
    pub invocation_owner: WorkflowInvocationOwner,
    pub stage: WorkflowStage,
    pub status: WorkflowStatus,
    pub sequence: u64,
    pub parent_thread_id: Option<ThreadId>,
    pub child_threads: BTreeMap<WorkflowStageKey, ChildCheckpoint>,
    pub stage_presentations: BTreeMap<WorkflowStageKey, StagePresentation>,
    pub lark: LarkCheckpoint,
    pub design_approval: Option<DesignApproval>,
    pub failure: Option<WorkflowFailure>,
}
```

  `WorkflowInvocationOwner` distinguishes `ExistingParent { parent_thread_id }` from the stable `WorkflowCreatedRoot` mode. `ManifestStore::write_atomic` must create a same-directory temporary file, serialize, `sync_all`, rename, then sync the parent directory when supported. `RunStateCommitter` owns `Arc<tokio::sync::Mutex<WorkflowManifest>>`; its single `commit_transition` operation clones/validates the next state, increments sequence, updates current status/presentation/journal, writes atomically, then swaps the in-memory value and returns an immutable committed snapshot. Stage, cancellation, failure, and completion code never mutate or publish those fields separately. `RunGuards` owns both open locked files for the entire live run. Start-index writes precede root/Lark effects and map SHA-256(preparation ID) to the sole run ID and normalized invocation digest. The digest covers canonical repository, normalized artifact path, ordered normalized developers, workflow name/version, and ownership mode; the eventual created root ID is recorded in `parent_thread_id`, not folded into the pre-root digest.
- [ ] **Run GREEN.** `just test -p codex-dev-lark-sdk-feature store_tests transition_tests` must pass; inspect tests to ensure no secret-like fields are serialized.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): persist crash-safe workflow state'`.

## Task 5: Implement the injectable, cancellation-aware Lark process runner

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/lark/{mod.rs,runner.rs,runner_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/{lib.rs,model.rs}`

- [ ] **Write RED runner tests.** A scripted fake must capture executable and argv as separate values. The sibling `runner_tests.rs` uses a `ctor` to dispatch its current Rust unit-test executable into deterministic helper modes and passes that path through a private test constructor—never `PATH` mutation, a shell fixture, or real `lark-cli`. Cover JSON success, nonzero exit, missing executable, auth/scope/update/console-location classification, stdout >4 MiB, stderr >256 KiB, 15/30/60-second operation timeouts with paused time, cancellation, kill request, five-second reap timeout, and sanitization. Feed access/refresh tokens and an auth URL containing sensitive query/fragment data; returned/loggable errors must omit them while optionally retaining only an allow-listed HTTPS developer-console origin/path with query and fragment stripped.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature runner_tests` must fail.
- [ ] **Implement a stable injected boundary without `async_trait`.**

```rust
pub trait LarkCommandRunner: Send + Sync + 'static {
    fn run(
        &self,
        request: LarkCommand,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<LarkOutput, LarkCommandError>> + Send + '_>>;
}

pub struct LarkCommand {
    pub operation: LarkOperation,
    pub argv: Vec<OsString>,
    pub timeout: Duration,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
}
```

  `TokioLarkCommandRunner::production()` fixes the executable to `lark-cli`; a private `with_executable_for_test(PathBuf, FixtureMode)` is visible only to tests and sets one non-secret test-mode environment variable consumed by the Rust helper's constructor. Execution uses `Command::new(&self.executable).args(&argv)`, concurrently drained piped stdout/stderr with independent bounds, `kill_on_drop(true)`, a `tokio::select!` over completion/cancellation/timeout, explicit `start_kill`, and a bounded reap. Diagnostic parsing returns closed error codes plus optional `SafeConsoleLocation { origin, path }`; it never returns/persists raw stderr or URL query/fragment material. Never use a shell, raw command string, test-time `PATH` mutation, or log unredacted output.
- [ ] **Run GREEN.** `just test -p codex-dev-lark-sdk-feature runner_tests` must pass.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): add safe lark command runner'`.

## Task 6: Implement typed bot auth, chat creation, membership verification, and sends

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/lark/client.rs`
- Create: `codex-rs/dev-lark-sdk-feature/src/lark/client_tests.rs`
- Modify: `codex-rs/dev-lark-sdk-feature/src/lark/mod.rs`

- [ ] **Re-read the Lark command contracts before writing tests.** Read `/Users/bytedance/.agents/skills/lark-shared/SKILL.md` and `/Users/bytedance/.agents/skills/lark-im/SKILL.md` completely, then inspect the installed `lark-cli` shortcut help/schema for chat creation, member add/list, message list, and message send. Record any drift from the approved design and stop for re-approval if command identity or payload semantics materially differ.
- [ ] **Write RED client tests using only the fake runner.** Cover read-only `auth status`, exact normal-group argv and description, persist-before-member-add callback order, bot-only identity on every operation, typed JSON parsing, partial/invalid/pending membership, paginated member verification, exactly one bot identity, chat search/list by name plus exact unique `codex:dev-lark-sdk-feature:<runId>` description marker (zero/one/multiple matches), app visibility/scope errors, welcome/status send, stable idempotency key, and update notices. Assert no command includes `sh`, `-c`, user identity, or raw credentials.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature client_tests` must fail.
- [ ] **Implement the typed client.** Production chat creation must be equivalent to:

```rust
vec![
    "im", "+chat-create", "--as", "bot", "--chat-mode", "group",
    "--name", "lark-sdk需求开发", "--description", &description,
    "--format", "json",
]
```

  Do not pass developers through the convenience `--users` path in production. Persist `chat_id` immediately, then invoke the schema-inspected bot member-add operation using `member_id_type=open_id`, `succeed_type=1`, and typed JSON data; paginate a bot member read to verify all requested developers and exactly one creator bot. Add `find_chat_by_run_marker`: list/search by the exact group name, then require exactly one exact description match; zero means safe-to-create and multiple means typed ambiguity failure. `send_markdown` always supplies `--as bot`, `--chat-id`, one `--markdown` argv value, `--idempotency-key`, and `--format json`.
- [ ] **Run GREEN.** `just test -p codex-dev-lark-sdk-feature client_tests` must pass and its recorded argv snapshots must be reviewed.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): add typed lark bot client'`.

## Task 7: Implement ordered inbound processing and idempotent outbound delivery

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/lark/{inbox.rs,inbox_tests.rs,outbox.rs,outbox_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/lark/{mod.rs,client.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/model.rs`

- [ ] **Write RED inbox tests.** Cover 50-item pages and page tokens, at most 100 pages per pass, ascending normalization by `(create_time,message_id)`, inclusive cursor IDs, duplicate/out-of-order pages, deleted messages, bot-self messages, unauthorized senders, missing/wrong structured mention, display-name-only spoofing, text extraction, 8 KiB input rejection, first eligible ordinary message classification as `Requirement`, first `/finish` rejection, `/finish` after research artifacts, early `/approve-design`, design-stage feedback, digest-gated approval, and contiguous terminal-prefix cursor advancement.
- [ ] **Write RED outbox tests.** Cover code-fence-aware 8 KiB chunks, 32-part and 256 KiB limits, deterministic stable reference/key from `(run,stage,turn,part)`, persistence before send, ambiguous-send reconciliation by bot-message scan, replay without duplicate send, and one idempotent explanatory response for rejected commands/input.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature inbox_tests outbox_tests` must fail.
- [ ] **Implement eligibility and commands from structured metadata.**

```rust
pub fn classify_message(
    message: &LarkMessage,
    policy: &InboundPolicy,
    manifest: &WorkflowManifest,
) -> InboundClassification {
    if message.deleted || message.sender_id == policy.bot_id {
        return InboundClassification::Ignored(IgnoreReason::BotOrDeleted);
    }
    if !policy.developer_ids.contains(&message.sender_id) {
        return InboundClassification::Ignored(IgnoreReason::UnauthorizedSender);
    }
    if !message.mentions.iter().any(|mention| mention.id == policy.bot_id) {
        return InboundClassification::Ignored(IgnoreReason::BotNotMentioned);
    }
    classify_mentioned_text(remove_verified_mention(message, &policy.bot_id), manifest)
}
```

  Poll with cancellation-aware exponential backoff from 2 to 30 seconds with jitter. Every observed message is durably classified. Eligible ordinary entries transition `Received -> TurnSubmitted -> AssistantPersisted -> ForwardPending -> Forwarded -> CursorCommitted`; only the contiguous terminal prefix advances the cursor.
- [ ] **Implement outbound transactions.** Persist each assistant output before sending. Split without breaking an open code fence, add a stable human-visible reference, derive idempotency keys from stable IDs, reconcile ambiguous responses, then mark forwarded. Never silently truncate.
- [ ] **Run GREEN.** `just test -p codex-dev-lark-sdk-feature inbox_tests outbox_tests` must pass with paused Tokio time, not sleeps.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): process lark messages idempotently'`.

## Task 8: Add optimized, delimiter-safe stage prompts and artifact contracts

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/prompts/{research-developer.md,research-turn.md,technical-design-developer.md,technical-design-turn.md,implementation-developer.md,implementation-turn.md}`
- Create: `codex-rs/dev-lark-sdk-feature/src/{prompts.rs,prompts_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/{BUILD.bazel,src/lib.rs}`

- [ ] **Read prompt-design evidence at execution time.** Re-read `/Users/bytedance/.codex/skills/prompt-optimizer/SKILL.md` and only its required references. Record the techniques used in the code review, but do not create a runtime dependency on that personal path.
- [ ] **Write RED prompt tests.** Snapshot all six templates and rendered prompts. Cover collision-free boundaries, canonical JSON escaping, malicious requirement content containing Markdown fences/instruction text, exact artifact paths, research no-implementation rule, design private-template access through supported Lark document mechanisms, implementation TDD/worktree rules, secret-prohibition text, and atomic audit-copy paths/names under `runs/<runId>/prompts/`. For the 9,000-token safety budget, test ASCII and multibyte payloads immediately below/at/above 9,000 UTF-8 bytes and require rejection before any audit write or submission.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature prompts_tests` must fail.
- [ ] **Implement prompt rendering and a conservative token upper bound.** Embed sources through `include_str!`, generate a nonce boundary absent from the serialized payload, and put trusted context and untrusted runtime JSON in separate sections. Enforce `rendered.as_bytes().len() <= 9_000` on every fully rendered model-visible user item before persistence/submission. Codex's existing `approx_token_count` is a coarse lower estimate and must not enforce this invariant; for the byte-level token encodings used by the Responses transport, one UTF-8 byte per token is the conservative worst case. Return typed `PromptTooLarge { utf8_bytes, limit: 9_000 }`. The prompt API is stage-specific:

```rust
pub enum RenderedStagePrompt {
    Research { developer: String, initial_user: String },
    TechnicalDesign { developer: String, initial_user: String },
    Implementation { developer: String, initial_user: String },
}
```

  Research requires the four research files and prohibits source edits. Design requires `design/technical-design.md` and no implementation. Implementation verifies the approval digest and requires summary/tests/risk files. Before every initial or follow-up submission, atomically persist the exact rendered developer/user prompt copies under the run-local `prompts/` directory and checkpoint their digests so recovery can prove which prompt was submitted.
- [ ] **Run GREEN and Bazel compile-data check.** Run `just test -p codex-dev-lark-sdk-feature prompts_tests` and `bazel build //codex-rs/dev-lark-sdk-feature:dev-lark-sdk-feature`. Both must pass.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): add stage prompt templates'`.

## Task 9A: Persist and project immutable workflow thread correlation

**Files:**

- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/rollout/src/recorder.rs`
- Modify: `codex-rs/thread-store/src/{types.rs,local/create_thread.rs,in_memory.rs}`
- Modify: `codex-rs/core/src/session/{mod.rs,session.rs}`
- Modify: `codex-rs/core/src/codex_thread.rs`
- Create: `codex-rs/core/src/codex_thread_tests.rs`
- Create: `codex-rs/state/migrations/0041_threads_workflow_correlation.sql`
- Modify: `codex-rs/state/src/{extract.rs,migrations.rs,migrations_tests.rs}`
- Modify: `codex-rs/state/src/model/thread_metadata.rs`
- Modify: `codex-rs/state/src/runtime/threads.rs`
- Modify: focused protocol/rollout/store/state tests beside those modules

- [ ] **Write RED durable-correlation tests.** Prove `(owner,runId,kind,attemptId)` is present in the first durable `SessionMeta`, survives rollout resume, projects to SQLite, is exposed on a live/resumed `CodexThread`, and is uniquely queryable. Cover old metadata without the optional field, migration/backfill, and zero/one/multiple lookup results. Prove a stage correlation is immutable for the thread lifetime so app-server can recognize ownership before any asynchronous thread-created notification or in-memory guard registration.
- [ ] **Run RED.** Run `just test` filters named `workflow_thread_correlation` in `codex-protocol`, `codex-rollout`, `codex-thread-store`, `codex-state`, and `codex-core`; they must fail because the field/projection/query/accessor do not exist.
- [ ] **Add schema-compatible correlation to canonical session metadata.** Use an optional field with serde default/skip so old rollouts remain readable. Carry it through `CreateThreadParams` and `RolloutRecorderParams::Create` so thread creation and the first `SessionMeta` write share one value:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowThreadCorrelation {
    pub owner: String,
    pub run_id: String,
    pub kind: WorkflowThreadKind,
    pub attempt_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowThreadKind {
    Root,
    Stage { stage: String },
}
```

- [ ] **Project and query correlation.** Migration 0041 adds nullable owner/run/kind/stage/attempt columns plus a selective lookup index. State extraction copies the optional `SessionMeta` value; `StateRuntime::find_thread_by_workflow_correlation` returns zero/one/multiple as distinct results so recovery never guesses. Add a narrow read-only live-thread accessor for the immutable correlation; it does not expose `Session`.
- [ ] **Run GREEN and inspect migration/build data.** Run the five focused test filters plus migration tests, and verify the migration is included by the state crate's Cargo/Bazel data mechanism.
- [ ] **Commit.** `git add codex-rs/protocol codex-rs/rollout codex-rs/thread-store codex-rs/state codex-rs/core/src/session codex-rs/core/src/codex_thread.rs codex-rs/core/src/codex_thread_tests.rs && git commit -m 'feat(core): persist workflow thread identity'`.

## Task 9B: Expose a two-phase managed-child lifecycle through core

**Files:**

- Modify: `codex-rs/core/src/thread_manager.rs`
- Modify: `codex-rs/core/src/thread_manager_tests.rs`
- Modify: `codex-rs/core/src/codex_thread.rs`
- Modify: `codex-rs/core/src/codex_thread_tests.rs`
- Modify: `codex-rs/core/src/agent/control/spawn.rs`
- Modify: `codex-rs/core/src/agent/control_tests.rs`
- Create: `codex-rs/core/tests/suite/managed_workflow_child.rs`
- Modify: `codex-rs/core/tests/suite/mod.rs`

- [ ] **Write RED lifecycle tests.** Cover correlated root durability, root failure before/after thread identity, two-phase child creation with no submission/model activity before the explicit submit call, parent `ThreadSpawn` lineage/path/depth, capacity accounting, cleanup after partial creation, cwd override, inherited current model/provider/reasoning/approval/reviewer/permission profile/base instructions, appended bounded developer instructions, stable initial `client_user_message_id`, returned child/submission IDs, subsequent stable submissions, conditional interrupt outcomes, and subtree shutdown. Unload and resume a correlated child through its parent, then prove it and newly created siblings still share the same registry/session/capacity budget and root subtree shutdown reaches all of them; ordinary `resume_thread_from_rollout` must not be used for stage children.
- [ ] **Run RED.** `just test -p codex-core managed_child` must fail because the façade and deferred-input agent path do not exist.
- [ ] **Build child config from current runtime state.** Add a private core helper that starts from the parent's effective/base config and overlays `CodexThread::config_snapshot()` values—current model/provider/reasoning/service tier, approval/reviewer, permission profile/sandbox, environments/workspace roots, collaboration/personality, cwd, and current base/developer instructions. Then apply the explicit stage cwd and append bounded workflow instructions. Do not use `Session::get_config()` or `original_config_do_not_use` as an unqualified clone; mutation-after-start tests prove the child sees current settings except for the intentional stage overlay.
- [ ] **Implement the correlated-root façade.** Add `WorkflowRootStartError::{BeforeThread,AfterThread { thread_id, source }}` and `ThreadManager::start_workflow_root(options, correlation)` over the existing start path. After allocation call result-returning `CodexThread::flush_rollout`, which materializes deferred persistence, and return only when correlation is durable; never rely on warning-only `ensure_rollout_materialized`. A flush failure is `AfterThread` and retains the ID for recovery.
- [ ] **Implement two-phase child create/submit.** Extend the existing internal agent-control spawn primitive with a deferred-input variant; it performs the existing graph/depth/capacity/lineage registration and emits creation but starts no task. `ManagedChildSpec` contains the parent and correlation; expose `ThreadManager::create_managed_child(spec) -> ManagedChildHandle`, flush its rollout, and expose `submit_managed_child_initial_input(child_thread_id, Op::UserInput, stable_client_id) -> submission_id`. This separation is mandatory: app-server must acquire the child lease, establish the sole listener, and activate its immutable-correlation mutation policy between create and submit. On recovery, inspect the stable client ID/rollout before deciding whether to call submit.
- [ ] **Add managed-child resume.** Expose `ThreadManager::resume_managed_child(parent_thread_id, child_thread_id, expected_correlation) -> ManagedChildHandle`. It validates persisted `ThreadSpawn` parent plus immutable correlation, then delegates to the existing crate-private agent resume path with the parent's `AgentControl`; it must not call the ordinary top-level `resume_thread_from_rollout`, which constructs a fresh control. It resumes without submitting input and returns only after correlation is live/durable.
- [ ] **Use the root tree's existing `AgentControl`.** Create, managed resume, and `shutdown_agent_subtree` must load the parent/target thread and clone `thread.codex.session.services.agent_control` internally. They must never call `ThreadManager::agent_control()` or `agent_control_for_config()` for a stage child, because those construct a fresh registry. Keep this access inside core; do not expose `Session` or `AgentControl` publicly. Test sibling/depth/capacity accounting and subtree shutdown against the same registry.
- [ ] **Add the required core integration test.** In `core/tests/suite/managed_workflow_child.rs`, use `TestCodexBuilder` and mocked Responses events to start a correlated root, create a correlated child, assert no request occurs before explicit submit, submit with a stable client ID, observe a tool call and paired tool output, observe the follow-up sample, and assert the authoritative final item plus rollout metadata. Register it in `suite/mod.rs`.
- [ ] **Run GREEN and regression tests.** Run `just test -p codex-core managed_child` plus focused existing agent-control/subagent filters. The integration test must prove the request and tool-output follow-up traverse ordinary `Op::UserInput` -> `RegularTask` -> `run_turn` behavior.
- [ ] **Enforce the change-size gate.** Before commit, inspect `git diff --stat` and split again if any review unit exceeds the repository's 500-line target or 800-line hard review limit; do not combine Task 9A and 9B into one commit.
- [ ] **Commit.** `git add codex-rs/core && git commit -m 'feat(core): add two-phase managed workflow children'`.

## Task 10: Track authoritative child-turn output and lease workflow threads in app-server

**Files:**

- Create: `codex-rs/app-server/src/workflow/{mod.rs,turn_tracker.rs,turn_tracker_tests.rs,thread_leases.rs,thread_leases_tests.rs}`
- Modify: `codex-rs/app-server/src/lib.rs`
- Modify: `codex-rs/app-server/src/in_process.rs`
- Modify: `codex-rs/app-server/src/bespoke_event_handling.rs`
- Modify: `codex-rs/app-server/src/message_processor.rs`
- Modify: `codex-rs/app-server/src/request_processors/{thread_processor.rs,turn_processor.rs}`
- Modify: `codex-rs/app-server/src/request_processors/thread_lifecycle.rs`
- Modify: `codex-rs/app-server/src/thread_state.rs`
- Modify: `codex-rs/app-server-client/src/lib.rs`

- [ ] **Write RED tracker tests.** Feed ordered `ItemCompleted`, successful `TurnCompleted`, failed terminal, and interrupted/aborted terminal events through their current distinct event branches. Assert selection of the last `MessagePhase::FinalAnswer`, legacy `None` fallback only, exclusion of Commentary, typed missing-output failure, explicit failed/interrupted results, child/turn key isolation, waiter-before-completion, completion-before-waiter, cancellation, and 256-terminal-entry eviction. Assert normal client notification projection is unchanged.
- [ ] **Write RED lease/listener/delivery tests.** Assert reference-counted root/child leases prevent only subscriberless timeout unload, do not block explicit shutdown/archive/delete, transfer/release at normal stage finalization, are released at terminal cleanup, restore on recovery, and do not hot-loop once the original 30-minute deadline has passed. Releasing the last lease starts a fresh subscriberless grace period. Prove an internal lease-owned attach starts the same sole listener task with zero client connections, later client subscription reuses it, and listener teardown still follows the ordinary registry. Add a listener-command observer barrier returning `NewlySubscribed`, `AlreadySubscribed`, or `ConnectionClosed`: if approval creation happens just before the command it is replayed once; if it happens just after, live delivery sends it once; repeated attach on one connection sends nothing; a new connection after close receives one replay. Assert `WorkflowCompleted` is required-delivery while `WorkflowProgress` remains best-effort/coalescible in both delivery pumps.
- [ ] **Run RED.** `just test -p codex-app-server workflow::turn_tracker workflow::thread_leases` and `just test -p codex-app-server-client workflow` must fail.
- [ ] **Implement the tracker on the sole event-consumer path.** Update tracker state synchronously/in event order from both completion and abort/failure branches in `bespoke_event_handling` before waking waiters, then continue existing projection. Store only completed agent items and terminal status; never call `CodexThread::next_event` from workflow code.

```rust
pub async fn wait_for_turn(
    &self,
    child: ThreadId,
    turn_id: &str,
    cancellation: CancellationToken,
) -> Result<AuthoritativeTurnResult, TurnTrackingError>;
```

- [ ] **Wire shared tracker/lease ownership and the internal listener entry.** `MessageProcessor` owns/clones the shared registries into both `ThreadRequestProcessor` and `TurnRequestProcessor`, and both listener-construction sites put them into `ListenerTaskContext`. Factor an idempotent `ensure_listener_task_running` entry that can use a lease-created `ThreadState` without a `ConnectionId`; ordinary `ensure_conversation_listener` still adds a subscriber first. Add `ThreadListenerCommand::AttachWorkflowObserver { connection_id, completion_tx }`. In the sole listener task's biased command branch, atomically determine whether the connection is new; only when new, add it and replay pending requests before acknowledging. Because the same task cannot process a core event concurrently, a request is either pending-before-barrier or live-after-barrier, never both. The workflow entry starts this same registered listener task and remains alive while leased even when every local-daemon client disconnects. Avoid a parallel listener or per-processor registry.
- [ ] **Implement leases and delivery classification.** `WorkflowThreadLeaseRegistry` owns counts plus a watched generation/state keyed by `ThreadId`. When the subscriberless deadline fires while leased, `wait_for_unloading_trigger` awaits lease/subscriber/shutdown change instead of `continue`-polling an expired timer. When the last lease releases, reset a fresh 30-minute subscriberless deadline. Extend both independent required-delivery classifiers only for `WorkflowCompleted`, not progress.
- [ ] **Run GREEN.** Run the focused tests above plus existing `thread_lifecycle`, `ItemCompleted`, `TurnCompleted`, and app-server-client backpressure tests.
- [ ] **Enforce the Task 10 size gate.** Keep tracker, lease, and listener wiring in separate review units; inspect per-file/per-commit diff size and split further before commit if any new production module approaches 500 lines or the review unit reaches 800 lines.
- [ ] **Commit.** `git add codex-rs/app-server codex-rs/app-server-client && git commit -m 'feat(app-server): track workflow turns and leases'`.

## Task 11: Implement the feature-specific Codex host adapter

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/host.rs`
- Create: `codex-rs/dev-lark-sdk-feature/src/host_tests.rs`
- Create: `codex-rs/app-server/src/workflow/{host.rs,host_tests.rs}`
- Create: `codex-rs/app-server/src/workflow/{mutation_policy.rs,mutation_policy_tests.rs}`
- Modify: `codex-rs/app-server/src/message_processor.rs`
- Modify: `codex-rs/app-server/src/request_processors/thread_processor.rs`
- Modify: `codex-rs/app-server/src/request_processors/turn_processor.rs`
- Create: `codex-rs/app-server/tests/suite/v2/workflow.rs`
- Modify: `codex-rs/app-server/tests/suite/v2/mod.rs`
- Modify: `codex-rs/dev-lark-sdk-feature/src/{lib.rs,model.rs}`
- Modify: `codex-rs/app-server/src/workflow/mod.rs`

- [ ] **Write RED workflow-port contract tests.** A fake host records root selection/creation, correlation lookup, child spawn, stable submission, turn wait, normal stage finalization, interruption, cancellation subtree shutdown, flush, and root termination. Assert orchestration can be unit-tested without `codex-core` ownership types leaking into persisted DTOs.
- [ ] **Write RED app-server adapter tests.** Cover attach existing parent, create correlated new root, and the exact child order `core create -> durable correlation -> lease -> sole internal listener ready -> root observers attached through listener-command barriers -> mutation policy active -> initial submit`. Prove immediate model completion cannot outrun the listener, a child created after every local-daemon client disconnects still drains to `WorkflowTurnTracker`, later subscription reuses the same listener, and dropped `thread_created` broadcast does not matter. Race approval creation on both sides of the first observer barrier and assert exactly one request; repeated attach/read on that connection sends none, while a newly connected observer receives one pending replay without a second listener. Also cover zero/one/multiple durable correlation lookup, state-projection lookup when available and bounded rollout/ThreadStore fallback when disabled/lagging, managed-child resume through the parent's shared `AgentControl`, initial stable client-ID inspection before resubmission, lease transfer/release, normal stage finalization, parent usability, and parent termination.
- [ ] **Write RED mutation-policy tests.** Through public JSON-RPC, exercise a table of every current thread-scoped state/task/execution mutator against a correlated stage child: `turn/start`, `thread/inject_items` (the current underscore wire name), steer/interrupt, realtime start/append/stop, `review/start`, thread name/metadata/settings/memory-mode changes, goal set/clear, unarchive, compact/rollback/fork, background-terminal clean/terminate, and `thread/shellCommand`. Every request fails with one typed workflow-ownership conflict before state mutation, even between core creation and host registration. Read/subscription methods and approval/guardian/elicitation responses required by the ordinary tool path remain allowed. Archive/delete of a workflow root or child requests whole-run cancellation and bounded cleanup before ordinary destructive handling; cleanup failure prevents silent archive/delete and remains durable/visible. Add a fixture that enumerates all current `ClientRequest` thread/turn variants so a newly added mutator forces an explicit allow/deny classification.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature host_tests` and `just test -p codex-app-server workflow::host_tests` must fail.
- [ ] **Define the narrow port.** Keep futures object-safe without exposing `Session`:

```rust
pub trait WorkflowCodexHost: Send + Sync + 'static {
    fn ensure_root(&self, request: RootRequest, cancel: CancellationToken)
        -> BoxFuture<'_, Result<RootHandle, WorkflowHostError>>;
    fn spawn_child(&self, request: StageChildRequest, cancel: CancellationToken)
        -> BoxFuture<'_, Result<StageChildStart, WorkflowHostError>>;
    fn submit(&self, request: StageSubmission, cancel: CancellationToken)
        -> BoxFuture<'_, Result<TurnId, WorkflowHostError>>;
    fn wait_for_turn(&self, child: ThreadId, turn: TurnId, cancel: CancellationToken)
        -> BoxFuture<'_, Result<AuthoritativeAssistantItem, WorkflowHostError>>;
    fn finalize_stage_child(&self, child: ThreadId)
        -> BoxFuture<'_, Result<(), WorkflowHostError>>;
    fn flush_thread(&self, thread: ThreadId)
        -> BoxFuture<'_, Result<(), WorkflowHostError>>;
    fn cancel_and_shutdown(&self, request: CleanupRequest)
        -> BoxFuture<'_, Result<(), CleanupFailure>>;
}
```

- [ ] **Implement the two-phase app-server adapter.** Delegate to the Task 9B core APIs, `CodexThread::flush_rollout` / `shutdown_and_wait`, the Task 10 internal form of the ordinary listener-registration helper, `WorkflowTurnTracker`, `WorkflowThreadLeaseRegistry`, and state projection. `spawn_child` calls core create first, then acquires a lease and awaits sole-listener readiness, attaches each current root/run observer by sending and awaiting `AttachWorkflowObserver` on that listener, activates the policy, and only then submits stable initial input; on any pre-submit failure it cleans up the correlated child and never starts a task. The listener command owns new-subscription replay; callers never enumerate pending requests themselves. Recovery resumes a stage only through `resume_managed_child(parent, child, correlation)`, then repeats lease/listener/observer barriers before any resubmission. Created/resumed roots and children use exactly one ordinary app-server listener; workflow code never receives core events directly. `finalize_stage_child` flushes, shuts down that stage subtree, and releases its lease without cancelling the root; `flush_thread` durably flushes the parent without shutting it down.
- [ ] **Implement one centralized pre-dispatch mutation policy.** Before `MessageProcessor` routes a `ClientRequest` to its owning processor, classify every current thread/turn request as read/subscription, required response, guarded mutation, or archive/delete lifecycle. For a guarded target, ask the live thread for immutable `WorkflowThreadCorrelation`; `Stage` is sufficient to reject it, so separate processors and async registration cannot create gaps. The workflow host bypasses only this public-RPC check and still uses public `CodexThread`/`ThreadManager` submission. Define a small injected `WorkflowCancellationControl` port for root/child archive-delete handling and test it with a fake here; Task 16 supplies the real manager implementation. Do not block ordinary root turns or responses to core-originated approvals/elicitations. Do not make private `Session` APIs public or reach around `CodexThread` to `LiveThread`.
- [ ] **Run GREEN.** Both focused test sets pass.
- [ ] **Enforce the Task 11 size gate.** Review host adapter, mutation policy, and processor wiring as separate units; split the commit if the aggregate non-mechanical change reaches 800 lines, and keep each new production module near or below 500 lines.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature codex-rs/app-server && git commit -m 'feat(workflow): bridge managed codex children'`.

## Task 12: Implement preparation, accepted-start idempotency, and runtime initialization

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/runtime/{mod.rs,initialization.rs,initialization_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/{lib.rs,model.rs,store.rs}`

- [ ] **Write RED initialization tests.** Cover the pure read-only preparation builder and accepted-start runtime: token-bound prepared data, exact confirmation text, rejection with zero effects, accepted retry after expiry once a start index exists, conflicting token reuse, lost start response, artifact-lock-first ordering, known-parent lock before start-index/manifest writes, new-root post-creation parent locking, new-root intent/discovery, persisted `WorkflowInvocationOwner`, existing TUI parent, direct-CLI root, root-start failure before/after identity, auth revalidation, artifact revalidation after creation, and run registration before background orchestration. Inject a recording `WorkflowProgressSink` and prove it receives only snapshots returned by `RunStateCommitter`, never an uncommitted state. A duplicate known-parent start must lose the lock without leaving a start index or run artifacts beyond the locked directory itself. The live ten-minute token registry and start/cancel serialization belong to app-server Task 16.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature runtime::initialization_tests` must fail.
- [ ] **Implement preparation as a read-only value.** The workflow crate validates inputs and may call `auth status --json`, then returns immutable normalized confirmation data to app-server. It may not create directories/locks/threads/groups/messages and does not own the live preparation registry. Persist only SHA-256(token) after app-server accepts a start.
- [ ] **Implement start order exactly.** Under preparation-key serialization, validate again, acquire the artifact lock, and inspect `current.json`. For an existing/recovered parent, acquire its parent lock before writing the accepted-start index, approval, root result, or first manifest. For a new root only, write the start index plus root intent, call correlated create/discovery, then acquire the newly known parent lock before the first manifest/orchestration effect. Persist owner and normalized invocation digest before foreign effects. Construct `WorkflowRuntime` with the sole `RunStateCommitter` and a projection-only `WorkflowProgressSink`, register cancellation/runtime, spawn orchestration, and return `WorkflowStartSnapshot`. Byte-equivalent retries return the stored run/root and never spawn twice.
- [ ] **Run GREEN.** Focused initialization tests pass with deterministic clocks and failpoints.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): initialize idempotent workflow runs'`.

## Task 13: Implement the research discussion loop on one real child thread

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/stages/{mod.rs,research.rs,research_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/runtime/mod.rs`
- Modify: `codex-rs/dev-lark-sdk-feature/src/{model.rs,store.rs}`

- [ ] **Write RED research-stage tests.** Cover chat creation/persistence before membership, welcome send, `WaitingForRequirement`, first eligible message creating the research child with the requirement as initial input, child intent before spawn, partial-spawn recovery, stable client ID `lark:<message-id>`, authoritative final-item persistence/Lark forwarding/parent presentation, no selection of terminal summary, later messages submitted sequentially to the same child, no second submission while a turn is active, ordinary modeled tool call and follow-up sampling through fake-host/core integration, required four artifacts before `/finish`, early `/finish` response, process/turn/send failure checkpoints, and parent progress ordering.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature research_tests` must fail.
- [ ] **Implement research stage transitions.** `CreatingChat -> WaitingForRequirement`; the first eligible ordinary message renders and persists the exact prompt audit copies before calling `spawn_child`; later eligible messages persist their rendered prompt before `submit`. For each turn, wait for the tracker-supplied final item, propagate failed/interrupted terminal status as failure, enforce the 256 KiB limit, persist `assistant-outputs`, forward through the outbox, then commit the cursor before polling the next message. After valid `/finish`, flush and normally finalize the research child before acquiring the design-child lease.
- [ ] **Verify core reuse with a mocked model/tool fixture.** Add a focused integration fixture that observes the child rollout containing user input, model tool call, tool output, follow-up request, final assistant item, and terminal event. The workflow asserts only through host APIs; it does not own sampling.
- [ ] **Run GREEN.** `just test -p codex-dev-lark-sdk-feature research_tests` and the focused core-backed integration test pass.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): run lark research discussion'`.

## Task 14: Add technical-design revision/approval and implementation stages

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/stages/{design.rs,design_tests.rs,implementation.rs,implementation_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/stages/mod.rs`
- Modify: `codex-rs/dev-lark-sdk-feature/src/runtime/{mod.rs,terminal.rs,terminal_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/{model.rs,store.rs}`

- [ ] **Write RED design tests.** Cover design child only after valid `/finish`, research-child normal finalization and finalization failure, research artifact reads, template URL in prompt, supported Lark-doc instruction, persisted prompt audit copies, no source editing, final item and path forwarding, failed/interrupted terminal propagation, `WaitingForDesignApproval`, feedback turns on the same design child, digest invalidation after every revision, unauthorized/unmentioned/early approval rejection, `approval.json` containing approver/message/time/exact SHA-256, and design-child finalization failure preventing implementation.
- [ ] **Write RED implementation tests.** Cover exact-digest recomputation before spawn, refusal on mismatch, design-child normal finalization and lease transfer, implementation child lineage, required SDK instruction/worktree/TDD text, persisted prompt audit copies, preservation record, real ordinary tools/approvals/sandbox path through host, authoritative output forwarding, failed/interrupted terminal propagation, required summary/tests/risk artifacts, atomic run-local `verification-report.md`, flush/finalize before completion, and failure when child/artifact/forward/flush/report persistence fails.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature design_tests implementation_tests` must fail.
- [ ] **Implement design stage.** `/finish` is a command, never user input. Spawn one design child, then use it for sequential feedback turns. Persist each rendered prompt before submission. Hash the exact persisted `technical-design.md` after each successful turn. Accept `/approve-design` only from an authorized structured mention in the correct state; atomically write `approval.json` and manifest approval, then normally finalize the design child before implementation.
- [ ] **Implement implementation stage.** Recompute the digest immediately before spawn, persist the rendered prompt, wait for terminal final item, persist/forward it, verify required artifacts, write `verification-report.md` atomically from durable stage/turn/artifact/test records, flush and normally finalize the implementation child/root persistence, then atomically commit `Completed`. Do not expose or call private `Session` methods.
- [ ] **Run GREEN.** The focused stage tests pass.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): gate design and implementation stages'`.

## Task 15: Complete explicit-retrigger recovery, cancellation, and failure semantics

**Files:**

- Create: `codex-rs/dev-lark-sdk-feature/src/runtime/{recovery.rs,recovery_tests.rs,cancellation.rs,cancellation_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/runtime/{mod.rs,terminal.rs,terminal_tests.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/{store.rs,model.rs}`
- Modify: `codex-rs/dev-lark-sdk-feature/src/lark/{runner.rs,inbox.rs,outbox.rs}`

- [ ] **Write RED recovery tests for every durable boundary.** Inject crashes after start-index write, artifact creation, root intent, root creation, chat intent, chat creation before `chat_id` persistence, member verification, child intent, child creation, turn submission, assistant persistence, each outbound part, design artifact update, approval write, implementation completion, and final manifest write. For the ambiguous chat-create window, cover zero/one/multiple exact run-marker matches. On explicit retrigger, assert discovery of the same recorded root/chat/child/turn/output and no duplicate group, turn, or send. Separately prove: TUI recovery succeeds only from the recorded owning parent; a different TUI parent conflicts before any recovery effect; direct CLI recovers its recorded workflow-created root; a different surface/ownership mode conflicts; and any mismatch in ordered developers, canonical repository, normalized artifact directory, workflow/schema version, or stored invocation digest returns a typed `RecoveryInvocationConflict` before locks are converted into live runtime effects. Completed/Cancelled must create a new run; no `resume` RPC/flag is added.
- [ ] **Write RED cancellation/failure tests.** Cover polling cancellation, in-flight process kill/reap, active child interruption, subtree shutdown, parent shutdown, child cleanup timeout, combined cleanup errors, Lark send failure after a completed turn, final persistence failure, `Cancelling` retention, `CancellationFailed`, exactly one terminal event, and no group deletion.
- [ ] **Run RED.** `just test -p codex-dev-lark-sdk-feature runtime::recovery_tests runtime::cancellation_tests` must fail.
- [ ] **Implement explicit recovery from intent/commit checkpoints.** Recovery is entered only by a new approved invocation. While holding the artifact lock, compare the complete normalized invocation and `WorkflowInvocationOwner` before creating a runtime, thread, group, or message; then reacquire the recorded parent lock, restore the root, resume any stage child only through the parent's managed-resume API, restore lease/listener/current-observer readiness, reconcile foreign effects by stable correlation/reference, and restart from the first incomplete checkpoint. Direct CLI reattaches the recorded workflow-created root rather than creating another. A chat intent without `chat_id` calls `find_chat_by_run_marker`; create only after zero matches, reuse exactly one, and fail without writing on multiple matches.
- [ ] **Implement structured cancellation.** One root `CancellationToken` derives child tokens. Persist `Cancelling`, cancel poll/process, conditionally interrupt the active turn, shut down workflow-owned subtree, flush within 30 seconds, aggregate errors, then persist `Cancelled` only on complete cleanup. A terminal-write failure emits a best-effort completion with `durable_state=false` and leaves durable `Cancelling`; all other terminal notifications reflect the durable manifest.
- [ ] **Run GREEN.** Both focused suites pass with paused time and injected failures.
- [ ] **Enforce module and review size.** Keep coordinator, recovery, cancellation, and terminal persistence in their separate modules; inspect per-file and per-commit diff size and split further before review if a new production file approaches 500 lines.
- [ ] **Commit.** `git add codex-rs/dev-lark-sdk-feature && git commit -m 'feat(workflow): recover and cancel durable runs'`.

## Task 16: Own live workflows in app-server and route all four RPCs

**Files:**

- Create: `codex-rs/app-server/src/workflow/{manager.rs,manager_tests.rs,preparation_registry.rs,preparation_registry_tests.rs,run_registry.rs,run_registry_tests.rs,progress_sink.rs,progress_sink_tests.rs,shutdown.rs,shutdown_tests.rs}`
- Modify: `codex-rs/app-server/src/workflow/mod.rs`
- Modify: `codex-rs/app-server/src/{lib.rs,in_process.rs,message_processor.rs,request_processors.rs,outgoing_message.rs}`
- Modify: `codex-rs/app-server/Cargo.toml`
- Modify: `codex-rs/app-server/tests/suite/v2/workflow.rs`

- [ ] **Write RED manager tests.** Cover the injected-clock in-memory preparation registry, opaque token generation, ten-minute expiry, one-use behavior, token binding to invocation/repository/optional parent/server/bot/root options, explicit rejection/cancel, keyed preparation-hash start/cancel race, accepted-start retries after mapping, run registry, one spawned task, parent/run lookup, `afterSequence`, `historyTruncated`, and read-through to the committer's durable 256-transition journal/fixed three-stage presentation snapshot. Prove the manager publishes only `CommittedWorkflowUpdate` values after persistence, while the explicitly non-durable terminal-write-failure path is separately marked. Also cover automatic root subscription plus current-child listener barrier before initial submit; approval creation racing the first `workflow/read` barrier is delivered once; repeated read performs no replay; after close, read from a new connection receives each pending child request once; zero-subscriber lease-owned listeners; current-subscriber lookup per emission; root-scoped projection; required terminal delivery; local-daemon disconnect not cancelling server-owned work; parent termination; and manager shutdown.
- [ ] **Write RED shutdown-order tests.** Exercise both the in-process and socket teardown paths with cancellation during Lark polling, a running fake Lark child process, and an active child turn. Assert `WorkflowManager::shutdown` runs while outgoing delivery, connection subscriptions, sole listeners, and threads are still alive; it kills/reaps the process, interrupts/flushes the child, persists `Cancelled` or an honest cleanup failure, and delivers the terminal notification when the transport remains writable. Only then may connection/RPC-gate cleanup and listener/thread teardown begin. If transport delivery is impossible, `workflow/read` after restart must expose the durable terminal state. Cover bounded graceful timeout followed by forced task abort without a false `Completed`/`Cancelled` claim.
- [ ] **Write RED v2 integration tests.** Use `AppServerTestClient` to exercise prepare/start/read/cancel JSON-RPC, server-side rejection of malformed raw prepare targets (both/neither parent and cwd), wrong repo typed error, malformed developers, no-effect rejection, direct-root creation, existing parent attachment, child metadata, omitted-`runId` lookup of the parent's current run, `afterSequence` retention/truncation, notifications, lost-progress repair, terminal outcome, and protocol schema fixtures. Fake Lark and fake/model fixtures only.
- [ ] **Run RED.** `just test -p codex-app-server workflow` must fail.
- [ ] **Implement `WorkflowManager` ownership.** The manager owns prepared/running registries, keyed locks, per-run cancellation roots, `WorkflowRuntime`/`RunStateCommitter` handles, turn tracker, thread leases, and host/Lark factories. It does not separately mutate manifest sequence/journal/presentation state. Implement Task 11's `WorkflowCancellationControl` so archive/delete can resolve immutable correlation to the owning run and await bounded cleanup. Runtime DTO conversion occurs at the request boundary; protocol structs never own locks, threads, runners, or stores.
- [ ] **Route RPCs from `message_processor`.** Add dedicated request-processor methods. Pass the requesting connection identity into start so a direct-CLI-created root is ordinarily subscribed and its active child is attached through the listener-command barrier; do not retain a stale sender snapshot in the runtime. Pass the requesting connection into read as well: after loading the authoritative run, ensure root subscription, then send/await `AttachWorkflowObserver` on the current child's sole listener before returning. The command, not the RPC handler, decides new/already/closed and performs ordered replay. Repeated read on an already subscribed connection returns state without resending requests. `start` returns after durable registration and spawned orchestration, not after group creation. `cancel` accepts either tagged preparation or run target and is idempotent.
- [ ] **Project only committed notifications.** Implement `WorkflowProgressSink::publish(CommittedWorkflowUpdate)` in app-server. The runtime first uses the sole `RunStateCommitter`; only its returned snapshot reaches the sink, which resolves current root subscribers and emits the matching progress/completion DTO. `workflow/read` reads the same committer/store snapshot and repairs dropped or truncated progress from the fixed map plus capped journal. The separate terminal-persistence failure callback is the only `durableState:false` path. Do not insert progress or assistant presentation into the parent rollout/model context.
- [ ] **Integrate graceful shutdown before generic app-server teardown.** Add idempotent `WorkflowManager::shutdown(deadline)` that closes preparation admission, cancels every run, awaits process/turn/subtree cleanup and terminal persistence, then aborts still-running orchestration at the deadline while retaining an honest durable failure/cancelling state. In the in-process path call it before `clear_runtime_references`, `connection_closed`, listener clearing, background drain, or thread shutdown. In the socket path call it before per-connection RPC-gate shutdown/connection cleanup, listener teardown, background drain, or thread shutdown. Outgoing delivery must remain usable through the manager call; forced app-server shutdown may shorten the deadline but must never reorder it. Wire archive/delete lifecycle requests through the same per-run cancellation operation before destructive thread handling.
- [ ] **Run GREEN.** `just test -p codex-app-server workflow` and focused message-processor/request-serialization tests pass.
- [ ] **Update dependency locks.** Run `just bazel-lock-update` after adding the workflow-crate dependency and inspect `codex-rs/Cargo.lock` plus `MODULE.bazel.lock`.
- [ ] **Enforce the Task 16 size gate.** Split preparation registry, running-run registry, committed-update projection, and shutdown coordination into focused modules/commits before any production file approaches 500 lines or the review unit reaches 800 lines.
- [ ] **Commit.** `git add codex-rs/app-server codex-rs/Cargo.lock MODULE.bazel.lock && git commit -m 'feat(app-server): run dev lark workflows'`.

## Task 17: Add TUI `/workflow` parsing, confirmation, progress, and cancellation

**Files:**

- Modify: `codex-rs/tui/Cargo.toml`
- Modify: `codex-rs/tui/src/slash_command.rs`
- Modify: `codex-rs/tui/src/chatwidget.rs`
- Modify: `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
- Modify: `codex-rs/tui/src/{app_server_session.rs,app.rs,app_event.rs}`
- Create: `codex-rs/tui/src/workflow/{mod.rs,state.rs,state_tests.rs,controller.rs,controller_tests.rs,render.rs,render_tests.rs}`
- Modify tests: inline tests in `codex-rs/tui/src/slash_command.rs`, `codex-rs/tui/src/app/tests.rs`, and `codex-rs/tui/src/chatwidget/tests.rs`
- Create: `codex-rs/tui/src/app/tests/workflow.rs`
- Create: `codex-rs/tui/src/chatwidget/tests/workflow.rs`
- Modify: `codex-rs/tui/src/snapshots/*.snap` only through reviewed Insta updates

- [ ] **Write RED slash/parser tests.** Assert `/workflow` supports inline args, extracts the untouched remainder, uses `shlex` only to form tokens, calls shared `WorkflowInvocationArgs::parse_tokens` with the leading `dev-lark-sdk-feature` token intact, shows exact diagnostics, and has parity with direct CLI parsing. Assert the command is unavailable while the parent has an active model turn, remains available while the parent is idle with an active workflow, and never becomes ordinary prompt text.
- [ ] **Write RED target/confirmation tests.** Embedded and LocalDaemon call `workflow/prepare`; explicit Remote fails client-side with a clear message before prepare. Confirmation uses cancel-first/start-second and displays group, developers, bot identity, canonical repository/artifact path, and forwarding warning. Rejection sends no start.
- [ ] **Write RED presentation tests/snapshots.** Cover start/run identity, group status, waiting requirement, child/turn IDs, authoritative assistant Markdown plus artifact paths for research/design/implementation, design approval wait, implementation, failure/cancel/completion, parent ordinary-turn status temporarily overriding workflow status, and cancellation action. Force more than 256 progress transitions, then prove lag/reconnect repair via `workflow/read.stagePresentations` still restores all three stage results after their original transitions were evicted. Add an approval/tool-input request from the active child before its first tool: repeated read on the same connection must not duplicate it; after disconnect, reconnect/read on a new connection delivers one replay. Assert prepare/start/read/cancel are dispatched asynchronously through request handles rather than awaited on the render loop. Assert Ctrl+C interrupts an active parent turn first; only an idle parent with an active workflow sends workflow cancel. Progress/presentation is UI-only and never submitted to the parent model.
- [ ] **Run RED.** `just test -p codex-tui workflow` must fail.
- [ ] **Implement the typed session methods.** Add `workflow_prepare/start/read/cancel` to `AppServerSession` using `codex_dev_lark_sdk_feature::protocol_adapter::build_prepare_params` plus typed requests/response decoding. Keep the existing event loop as the only app-server event consumer; route notifications into explicit app events/state.
- [ ] **Implement the focused TUI workflow module.** `workflow::controller` owns confirmation/RPC handles/reconnect actions, `workflow::state` owns the root-scoped view model keyed by `runId`, and `workflow::render` produces focused status/history cells. Bind the current root when present; otherwise let start create it. Present `SelectionViewParams` with cancel first/start second; approval calls only start with `preparationId`. On lag/reconnect, read the run, restore the fixed stage map, and ask app-server to subscribe/replay the current child. Keep `app.rs`, `chatwidget.rs`, and `slash_dispatch.rs` to event forwarding and a small command hook. The root remains active and observable.
- [ ] **Run GREEN and review snapshots.** `just test -p codex-tui workflow` passes; run the repository's narrow Insta review/update command and inspect every changed snapshot.
- [ ] **Update dependency locks.** Run `just bazel-lock-update`, inspect `codex-rs/Cargo.lock` and `MODULE.bazel.lock`, and retain only TUI dependency changes.
- [ ] **Enforce the TUI size gate.** Keep every new workflow module near or below 500 lines and ensure the Task 17 review unit stays below the repository's 800-line hard limit by splitting state/controller/render commits further if necessary; do not add substantial workflow logic to the already-large `slash_dispatch.rs`.
- [ ] **Commit.** `git add codex-rs/tui codex-rs/Cargo.lock MODULE.bazel.lock && git commit -m 'feat(tui): support dev lark workflow command'`.

## Task 18: Add the direct `codex workflow` client through embedded app-server

**Files:**

- Modify: `codex-rs/cli/Cargo.toml`
- Modify: `codex-rs/cli/src/main.rs`
- Create: `codex-rs/cli/src/workflow_cmd.rs`
- Create: `codex-rs/cli/src/workflow_cmd_tests.rs`
- Modify: `codex-rs/app-server-client/src/lib.rs` only if a small reusable typed-request helper is required

- [ ] **Write RED Clap/parity tests.** Cover exact syntax, required/malformed developers, default/custom artifact dir, same shared parser values as TUI, rejection of remote flags for this subcommand, and unchanged no-subcommand TUI dispatch.
- [ ] **Write RED CLI-flow tests.** With an injected in-process client/event stream and fake TTY, cover prepare, exact confirmation, rejection, non-TTY fail-closed, start without a separate `thread/start`, a newly created root and children, progress to stderr, and ordinary approval/tool-input requests originating on an active child before its first tool. Repeated read on one connection must not replay; disconnect with one pending, reconnect/read using a new connection, and assert one replay/response without another listener. Also cover lag repair, Ctrl+C targeting preparation before start resolution and run after it, cancellation result, and process shutdown. On success, stdout must contain the authoritative implementation-child assistant text and then the canonical artifact directory as the final two bounded sections; it must not substitute a generic terminal-event summary.
- [ ] **Run RED.** `just test -p codex-cli workflow_cmd` must fail.
- [ ] **Add `Subcommand::Workflow(WorkflowInvocationArgs)`.** Route it from `cli_main` after inheriting relevant config overrides and rejecting unsupported remote mode. Start an embedded `AppServerClient` with the CLI workflow client identity; call the same `protocol_adapter::build_prepare_params` function and typed RPCs as TUI.
- [ ] **Implement human-only rendering.** Require a TTY for approval, print progress/status to stderr, render the fixed implementation-stage presentation's authoritative assistant output followed by the canonical artifact directory to stdout, resolve ordinary app-server approvals through an injected prompt policy, repair `Lagged` from `workflow/read.stagePresentations`, and shut down the client cleanly. Do not add a machine mode in this feature.
- [ ] **Run GREEN.** `just test -p codex-cli workflow_cmd` and focused existing multitool/no-subcommand dispatch tests pass.
- [ ] **Update dependency locks.** Run `just bazel-lock-update`, inspect `codex-rs/Cargo.lock` and `MODULE.bazel.lock`, and retain only CLI/client dependency changes.
- [ ] **Commit.** `git add codex-rs/cli codex-rs/app-server-client codex-rs/Cargo.lock MODULE.bazel.lock && git commit -m 'feat(cli): run dev lark workflow directly'`.

## Task 19: Prove the complete path, document it, and pass the repository quality gate

**Files:**

- Modify: `codex-rs/dev-lark-sdk-feature/tests/workflow_integration.rs`
- Modify: `codex-rs/app-server/tests/suite/v2/workflow.rs`
- Modify: `codex-rs/app-server/README.md`
- Modify if stale: `/Users/bytedance/project/harness-engine/docs/codex-dev-lesson/openwiki/{codex-cli-conversation-flow,codex-cli-conversation-code-map,codex-cli-conversation-type-map}.md`
- Modify if stale: `/Users/bytedance/project/harness-engine/docs/codex-dev-lesson/lessons/0004-build-a-custom-workflow.html`
- Modify: any generated schema/snapshots/build metadata already owned by preceding tasks

- [ ] **Add a complete fake-boundary integration test.** Drive: prepare -> approve -> start -> root -> bot chat/member verification -> first authorized bot-mentioned requirement -> two-phase research child create/listener-ready/submit -> mocked model tool call/output/follow-up sample -> authoritative final item -> Lark forward -> additional research message -> `/finish` -> design child -> revision -> structured `/approve-design` -> digest verification -> implementation child with mocked tool path -> artifact verification -> flush -> durable completion -> TUI/CLI-equivalent notification projection. Assert immutable correlation, shared root-tree `AgentControl`, parent/child lineage, stable IDs, ordinary rollouts, zero-connection listener operation, no receiver competition, fixed stage-summary repair after transition truncation, and no real process/network writes.
- [ ] **Add a compact failure matrix test.** Explicitly cover wrong repo; malformed developers; path/symlink escape; artifact failure; confirmation rejection; missing CLI/auth/scope; partial membership; duplicate start/messages; unauthorized/unmentioned/bot messages; child create/listener/submit/turn/model/tool/send failures; guarded external mutation attempts; cancellation in poll/process/turn; both app-server shutdown paths; parent archive/delete/shutdown; restart ownership/digest conflicts; restart checkpoints; early/valid finish; approval digest mismatch; and terminal persistence failure. Each must leave an honest durable status and visible safe error.
- [ ] **Run focused tests.** At minimum:

```text
rtk proxy just test -p codex-dev-lark-sdk-feature
rtk proxy just test -p codex-protocol workflow_thread_correlation
rtk proxy just test -p codex-rollout workflow_thread_correlation
rtk proxy just test -p codex-thread-store workflow_thread_correlation
rtk proxy just test -p codex-state workflow_thread_correlation
rtk proxy just test -p codex-core managed_child
rtk proxy just test -p codex-app-server-protocol workflow
rtk proxy just test -p codex-app-server-client workflow
rtk proxy just test -p codex-app-server workflow
rtk proxy just test -p codex-tui workflow
rtk proxy just test -p codex-cli workflow_cmd
```

- [ ] **Write narrow developer/operator documentation.** Update `codex-rs/app-server/README.md` with all four methods, both notifications, delivery/read semantics, and the two exact invocations. Update only stale evidence/API sections of the separate Codex development flow/code/type maps and custom-workflow lesson. Cover explicit confirmation, first-message requirement behavior, `/finish`, `/approve-design`, artifact layout, no automatic resume, explicit retrigger recovery, cancellation, bot identity, test fakes, and manual prerequisites. Record documentation drift; do not add general product documentation under `codex/docs`.
- [ ] **Validate the separate lesson workspace.** Preserve its parent-repository dirty state, re-check every changed relative/source/asset link from `/Users/bytedance/project/harness-engine/docs/codex-dev-lesson/`, and keep its changes separate from the nested Codex repository. If committing is safe, commit only those lesson paths in the parent repository as `docs: update Codex workflow lesson`; otherwise leave them as clearly reported scoped changes without staging unrelated files.
- [ ] **Regenerate and inspect generated artifacts and locks.** Run `just write-app-server-schema`; run the applicable Insta command and review diffs; run `just bazel-lock-update` after the final dependency graph; verify `MODULE.bazel.lock`, Bazel `compile_data` for prompts, and test data. No blind snapshot or lock acceptance.
- [ ] **Perform bounded manual verification without Lark writes.** Build `codex`; inspect `codex workflow --help` and TUI slash completion; run prepare against a fixture/fake runner. Real group creation/message sending remains a separately approved manual test and is not required for automated completion.
- [ ] **Record the optional live capability check separately.** Do not run it from an automated test. If the execution owner elects to use the user's already authorized capability check, run exactly `lark-cli im +chat-create --users 'ou_17f75702d3185ddad1f1c6c54679ae38' --as bot` once through RTK, record the returned chat identity as manual evidence, send no additional message, and do not delete the group. A full live workflow remains outside automated verification.
- [ ] **Self-review against the approved design and user requirements.** Search for `TODO`, `FIXME`, placeholders, `sh -c`, raw token/stderr/auth-URL fields, duplicate receiver consumption, public `Session` exposure, public resume, untyped raw args, and direct real-Lark calls in tests. Run the serialized-artifact sentinel scan for access/refresh tokens and URL query material. Confirm every required failure case and all 20 required test categories have an evidence row.
- [ ] **Request code review.** Invoke `superpowers:requesting-code-review`; assign independent reviewers to (1) spec/protocol/persistence/security, (2) core ownership/async/cancellation, and (3) TUI/CLI/tests. Apply feedback through `superpowers:receiving-code-review` and rerun the affected evidence.
- [ ] **Request full-suite approval.** Because protocol/core/common crates changed, pause and ask the user for the repository-required approval to run the complete `just test` suite. After approval, run it once and record the result; do not substitute focused tests for this gate.
- [ ] **Run focused lint/fix, then formatting, last.** After all tests and review-driven source changes are complete, run `just fix -p codex-dev-lark-sdk-feature`, `just fix -p codex-core`, `just fix -p codex-state`, `just fix -p codex-app-server-protocol`, `just fix -p codex-app-server`, `just fix -p codex-tui`, and `just fix -p codex-cli` as repository guidance permits. Then run `just fmt` and `just fmt-check`, and inspect the resulting diff. Per `AGENTS.md`, do not rerun tests after either fix or formatting.
- [ ] **Run verification-before-completion.** Invoke `superpowers:verification-before-completion`; verify the recorded fresh focused/full-suite outputs predate only the mandated final fmt/fix pass, inspect the post-fix diff and format check, and record manual gaps, documentation drift, and confirmation that unrelated changes remain intact.
- [ ] **Finish the isolated branch deliberately.** Invoke `superpowers:finishing-a-development-branch` after verification and present its integration choices; do not merge, push, or delete the worktree without the user's explicit choice.
- [ ] **Commit final docs/tests/generated evidence.** `git add` only owned files and commit with `git commit -m 'test(workflow): verify dev lark feature flow'`.

---

## Required-Test Traceability

| Required boundary | Primary task/evidence |
|---|---|
| 1. TUI `/workflow` parse/validation | Task 17 slash/parser tests |
| 2. Direct CLI parse | Task 18 Clap/parity tests |
| 3. App-server request/response/schema | Tasks 1 and 16 protocol/integration tests |
| 4. TUI/CLI shared path parity | Tasks 2, 17, and 18 shared-parser/RPC assertions |
| 5. Repository identity | Task 3 validation tests |
| 6. Artifact normalization/traversal | Task 3 path and symlink tests |
| 7. Manifest/atomic/recovery | Tasks 4 and 15 failpoint tests |
| 8. Mocked group/member behavior | Task 6 fake-runner tests |
| 9. Poll order/filter/dedup | Task 7 inbox tests |
| 10. `/finish` detection | Tasks 7 and 13 command tests |
| 11. Lark output idempotency | Task 7 outbox tests |
| 12. Parent/child identity/metadata | Tasks 9A, 9B, 11, and 16 tests |
| 13. Sequential same-child submissions | Task 13 research tests |
| 14. Ordinary model/tool sampling reuse | Tasks 9B, 13, and 19 mocked core path |
| 15. Authoritative assistant item | Task 10 tracker and Task 13 forwarding tests |
| 16. Parent progress projection | Task 16 app-server integration tests |
| 17. TUI state/snapshots | Task 17 presentation tests |
| 18. Cancellation/shutdown | Tasks 15 and 16 failure/shutdown-order tests |
| 19. Design/implementation transitions | Task 14 stage tests |
| 20. Prompt rendering/delimiter safety | Task 8 prompt snapshots/security tests |

---

## Required Review Checkpoints During Execution

1. **After Task 4:** review parser/validation/persistence types before any live runtime code depends on them.
2. **After Tasks 9A/9B:** review immutable correlation, two-phase listener barrier, shared root-tree `AgentControl`, core public API size, lineage compatibility, one-active-task behavior, and ordinary sampling/tool reuse.
3. **After Task 14:** review the complete stage machine, first-message requirement rule, design approval digest, and artifact contracts.
4. **After Task 18:** review TUI/CLI parity and confirm neither surface bypasses app-server.
5. **Before Task 19 completion:** independent code review and Superpowers verification are mandatory.

## Plan Acceptance Criteria

- Every production side effect occurs only after server-side validation and explicit approved preparation.
- Both user surfaces parse the same typed invocation and call the same app-server workflow handler.
- Workflow-owned agents are real child threads with normal core turn, sampling, model, tool, approval, event, rollout, state-projection, and cancellation behavior.
- App-server alone consumes core events and exposes authoritative final assistant items to the workflow.
- Every stage child is durably created, lease/listener/observer-subscription/mutation-policy ready, and only then submitted; disconnected clients and dropped creation broadcasts cannot strand an initial turn or child-scoped approval.
- Explicit recovery resumes stage children through the parent's shared `AgentControl`, preserving graph/capacity/subtree ownership.
- Immutable stage correlation blocks all external model/context/task mutation RPCs while ordinary tool approvals remain available.
- Lark discussion is ordered, sender/mention authenticated, deduplicated, bounded, persistent, and idempotently forwarded.
- Technical design is a revision-capable child stage and implementation cannot start until exact-digest approval.
- Recovery occurs only on explicit retrigger and cannot duplicate groups, turns, or sends; no public resume surface exists.
- Cancellation and failure states are honest, durable where claimed, parent-visible, and never delete the Lark group.
- Stage presentations survive progress-journal truncation, and app-server shutdown cancels workflows before tearing down listeners or threads.
- Automated tests make zero real Lark writes and the final report separates inspected, executed, and manual verification.
