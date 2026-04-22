# codex-core Code Analysis

## Scope

This document analyzes the `codex-core` crate at `/Users/bytedance/project/codex/codex-rs/core` based on:

- crate metadata and dependency declarations in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/core/Cargo.toml)
- the exported module surface in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/lib.rs)
- key runtime modules under `/src`
- representative unit tests and the aggregated integration suite under `/tests`

The crate is the business-logic layer for Codex. It sits between UI/front-end clients and lower-level workspace crates such as `codex-api`, `codex-rollout`, `codex-tools`, `codex-mcp`, `codex-state`, `codex-sandboxing`, and `codex-models-manager`.

## High-Level Responsibilities

- Own the lifecycle of a Codex thread/session: create it, restore it, fork it, mutate it, and shut it down.
- Translate user or client operations into turn execution through a queued submission/event model.
- Build model-visible prompt/context state from config, history, environment, skills, plugins, apps, and sandbox/approval policy.
- Execute model turns, tool calls, reviews, compaction, undo, realtime conversation, and multi-agent collaboration.
- Persist thread state and event history to rollout files and SQLite-backed state, then reconstruct history on resume/fork.
- Centralize approval, sandboxing, policy enforcement, and network mediation for commands and tools.
- Integrate optional subsystems: MCP tools/resources, skills, plugins/marketplaces, memories, guardian review, hooks, analytics, and telemetry.

In practice, `codex-core` is less a single library module and more an orchestration layer over many focused subsystems.

## Crate Shape

The top-level export map in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/lib.rs) shows the architectural split:

- Session/thread runtime: [session/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/mod.rs), [codex_thread.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/codex_thread.rs), [thread_manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/thread_manager.rs)
- Configuration and policy: [config/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/config/mod.rs), [config_loader/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/config_loader/mod.rs), [exec_policy.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/exec_policy.rs)
- Model/provider integration: [client.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/client.rs), [client_common.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/client_common.rs)
- Tools/runtime execution: [tools/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/mod.rs), [tools/router.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/router.rs), [tools/registry.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/registry.rs), [unified_exec/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/unified_exec/mod.rs)
- Conversation state/context: [context_manager/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/context_manager/mod.rs), [context](file:///Users/bytedance/project/codex/codex-rs/core/src/context), [state/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/state/mod.rs)
- Extensions and integrations: [skills.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/skills.rs), [plugins/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/mod.rs), [mcp.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/mcp.rs), [connectors.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors.rs)
- Specialized workflows: [guardian/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/guardian/mod.rs), [memories/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/memories/mod.rs), [realtime_conversation.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/realtime_conversation.rs), [tasks/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tasks/mod.rs)
- Persistence and rollout APIs: [rollout.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/rollout.rs), [message_history.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/message_history.rs), [state_db_bridge.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/state_db_bridge.rs)

## Public API Surface

The crate exports a relatively small public surface compared with its internal complexity.

### Primary entry points

- [ThreadManager](file:///Users/bytedance/project/codex/codex-rs/core/src/thread_manager.rs) is the top-level lifecycle manager for threads. It creates, resumes, forks, lists, and shuts down threads.
- [CodexThread](file:///Users/bytedance/project/codex/codex-rs/core/src/codex_thread.rs) is the per-thread conduit for `submit`, `next_event`, `shutdown_and_wait`, `steer_input`, MCP calls, and token usage inspection.
- [ModelClient](file:///Users/bytedance/project/codex/codex-rs/core/src/client.rs) is the session-scoped provider client used by turns for streaming, compaction, memory summarization, and realtime call setup.
- [Config](file:///Users/bytedance/project/codex/codex-rs/core/src/config/mod.rs) and `ConfigBuilder` load and normalize effective runtime configuration.

### Other notable exports

- Rollout/persistence helpers from [rollout.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/rollout.rs)
- MCP manager access via `McpManager` from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/lib.rs)
- skills and plugin managers/utilities via [skills.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/skills.rs) and [plugins/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/mod.rs)
- sandbox/network helper exports such as Linux sandbox spawn and Windows read-grant helpers

### API style

- The external API is asynchronous and event-driven.
- Mutation usually happens through `Op` submission rather than direct method calls.
- Thread-local persistence and reconstruction are first-class concerns, not optional add-ons.
- Many internal types are `pub(crate)` to keep the crate boundary smaller than the actual subsystem graph.

## Core Runtime Flow

### 1. Config loading

Config flows through [ConfigBuilder::build](file:///Users/bytedance/project/codex/codex-rs/core/src/config/mod.rs), which:

- locates `codex_home`
- loads layered config via `config_loader`
- merges CLI/harness overrides
- applies managed requirements and constraints
- resolves model providers, permissions, MCP config, feature flags, trust/project state, telemetry config, sandbox settings, and plugin/skill defaults

Important detail: configuration is not just parsed; it is normalized into an enforceable runtime form. The `Permissions` struct contains both:

- legacy `SandboxPolicy`
- split `FileSystemSandboxPolicy`
- split `NetworkSandboxPolicy`
- optional managed `NetworkProxySpec`

That dual representation exists to bridge older policy APIs with richer per-path enforcement.

### 2. Thread creation

The user-facing lifecycle begins in [ThreadManager](file:///Users/bytedance/project/codex/codex-rs/core/src/thread_manager.rs):

- `start_thread` and variants create a new thread
- `resume_thread_*` restore from rollout history
- `fork_thread` snapshots and spawns a derived thread

`ThreadManagerState::spawn_thread_with_source` eventually calls [Codex::spawn](file:///Users/bytedance/project/codex/codex-rs/core/src/session/mod.rs), passing:

- effective config
- auth/model/environment managers
- skills/plugins/MCP managers
- initial history
- dynamic tools
- session source and inherited shell/exec-policy state where needed

### 3. Session initialization

[Session::new](file:///Users/bytedance/project/codex/codex-rs/core/src/session/session.rs) is the central startup procedure. It does several things in parallel:

- initialize rollout recorder and state DB
- read message history metadata
- resolve auth and effective MCP server configuration

It then builds long-lived session services:

- `ModelClient`
- `UnifiedExecProcessManager`
- hooks
- analytics and telemetry
- MCP connection manager
- skills/plugins managers
- network proxy and network approval service
- shell and shell snapshotting
- agent identity manager
- rollout persistence

Finally it emits `SessionConfigured`, starts background listeners, initializes MCP connectivity, records initial history, and launches startup tasks such as memories.

### 4. Submission loop

The live control loop is [submission_loop](file:///Users/bytedance/project/codex/codex-rs/core/src/session/handlers.rs). This is the operational hub of the crate.

It consumes `Submission { id, op, trace }` values and dispatches `Op` variants such as:

- `UserInput` / `UserTurn`
- approvals and user-input answers
- compact / undo / review
- realtime start/text/audio/close
- MCP refresh and list operations
- thread metadata updates
- user shell command execution
- shutdown

This means the core runtime model is a queue pair:

- inbound queue of `Op`
- outbound queue of `Event`

That abstraction is explicit in [Codex](file:///Users/bytedance/project/codex/codex-rs/core/src/session/mod.rs).

### 5. Turn creation

Each user turn becomes a [TurnContext](file:///Users/bytedance/project/codex/codex-rs/core/src/session/turn_context.rs), which packages:

- resolved model info and per-turn reasoning settings
- current cwd/date/timezone/environment
- approval and sandbox state
- tools config and feature flags
- network proxy context
- dynamic tools and loaded skills
- telemetry and turn metadata state

The turn context is produced from session configuration plus transient runtime state. That separation is a strong design choice: session-scoped invariants live in `SessionConfiguration`, while turn-scoped mutations are reified into `TurnContext`.

### 6. Task execution

Tasks are modeled by [SessionTask](file:///Users/bytedance/project/codex/codex-rs/core/src/tasks/mod.rs). Key task kinds include:

- regular chat turn
- review
- compaction
- undo
- ghost snapshot
- user shell command

`Session::spawn_task` aborts incompatible active tasks, creates an `ActiveTurn`, seeds pending input, and runs the task on Tokio with cancellation, timing, telemetry, and unified completion/abort behavior.

This keeps turn kinds extensible while sharing:

- cancellation handling
- final `TurnComplete` / `TurnAborted` emission
- rollout flushing
- pending-input scheduling
- token/tool/memory metrics

## Model Interaction Flow

[client.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/client.rs) encapsulates all provider-facing model traffic.

### Session-scoped `ModelClient`

Responsibilities:

- resolve auth/provider state
- keep session identity headers stable
- manage websocket fallback state across turns
- cache websocket sessions where allowed
- expose unary helpers for compaction, realtime call setup, and memory summarization

### Turn-scoped `ModelClientSession`

Responsibilities:

- stream one or more Responses API requests for a single turn
- maintain per-turn sticky routing (`x-codex-turn-state`)
- reuse websocket connection inside a turn
- opportunistically prewarm websocket transport
- perform incremental request optimization using previous input/response state

### Transport behavior

- Prefer Responses WebSocket when provider capability allows.
- Fallback permanently to HTTP for the rest of the session when websocket health is lost or unsupported.
- Retry once through auth-recovery paths on 401 where supported.
- Attach Codex-specific headers such as installation id, parent thread id, window id, turn metadata, and subagent tags.

This subsystem is sophisticated because it combines transport optimization, routing stickiness, telemetry, and auth recovery without exposing that complexity to turn runners.

## Tooling and Execution Flow

The tool stack is split across:

- [tools/router.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/router.rs)
- [tools/registry.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/registry.rs)
- [tools/orchestrator.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/orchestrator.rs)
- [unified_exec/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/unified_exec/mod.rs)

### Tool routing

`ToolRouter`:

- builds tool specs visible to the model
- distinguishes function/MCP/custom/local-shell/tool-search payloads
- dispatches parsed tool calls to the registry
- knows whether a tool supports parallel execution

### Tool registry

`ToolRegistry`:

- maps `ToolName` to a handler
- tracks tool-call counts in active turn state
- runs pre/post tool-use hooks
- serializes mutating tools through readiness gates
- converts handler outputs into response items

### Orchestrator

`ToolOrchestrator` centralizes the policy-heavy path:

- decide whether approval is needed
- optionally route approval through guardian review
- choose sandbox mode
- run the tool
- detect sandbox denial
- optionally retry under relaxed sandbox rules without reprompting when allowed
- integrate network approval bookkeeping

This is one of the cleanest design decisions in the crate: policy logic is not duplicated across tool implementations.

### Unified exec

`unified_exec` is the interactive process subsystem for terminal-like tools:

- owns process lifecycle and bounded buffering
- manages process reuse by process id
- couples process execution to sandboxing/approval orchestration
- supports background terminals and bounded polling/yield windows

This subsystem is effectively a miniature terminal/process manager embedded into the crate.

## Context and History Design

Context is managed by [context_manager/history.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/context_manager/history.rs) and session methods in [session/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/mod.rs).

Key ideas:

- Conversation history is maintained in memory for prompt construction.
- Rollout items are persisted separately for durability and reconstruction.
- A `reference_context_item` acts as the baseline for later context diffs.
- Initial turns inject full model-visible context; later turns inject diff-style updates where possible.
- Resume/fork reconstruct history from rollout files and recover token usage snapshots.

The model-visible context can include:

- developer instructions
- permissions instructions
- apps/skills/plugins sections
- personality info
- environment context
- user instructions from AGENTS.md
- realtime updates
- memory tool instructions

This layered context building is one of the crate’s defining behaviors.

## Persistence and State

Persistence is built around [rollout.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/rollout.rs), which re-exports `codex-rollout` APIs and adapts crate `Config` into a rollout config view.

The crate persists:

- `ResponseItem`s
- protocol events
- turn-context snapshots
- compaction markers
- session metadata
- thread names and memory-mode changes

Operational state also lives in SQLite-backed services exposed through `state_db`. This is used for:

- dynamic tool restoration
- thread metadata/title lookup
- memory jobs and outputs
- spawn-tree relationships

The overall persistence design is hybrid:

- append-only rollout files for reconstructable conversation/event history
- SQLite for indexed metadata and background-job coordination

## Plugins, Skills, MCP, and Agents

### Skills

[skills.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/skills.rs) wraps `codex-core-skills` and adds session/turn behavior:

- derive skill inputs from config and plugin-provided skill roots
- resolve missing environment-variable dependencies by prompting the user
- emit analytics for implicit skill invocation

### Plugins

[plugins/manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/manager.rs) manages:

- configured and curated marketplaces
- local/remote plugin installation and sync
- plugin-provided skills, MCP servers, and app connectors
- plugin capability summaries for prompt injection

Plugins are not a cosmetic extension point; they materially affect tool surfaces, skill roots, and MCP topology.

### MCP

MCP is woven through session startup and tool dispatch:

- effective server config is built during session init
- required servers can fail session startup
- session operations can list tools/resources and call/read MCP endpoints
- MCP servers participate in auth, approvals, and tool routing

### Agents / multi-agent

The crate supports hierarchical thread spawning. Relevant pieces include:

- [thread_manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/thread_manager.rs)
- [agent/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/agent/mod.rs)
- [tools/handlers/multi_agents_v2/spawn.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs)

Important behaviors:

- spawn depth is enforced
- child threads inherit/fork selected context
- terminal child completion is forwarded back to parent agents
- inter-agent communication uses mailbox delivery and can trigger turns in idle sessions

## Guardian and Safety Model

[guardian/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/guardian/mod.rs) implements a fail-closed review path for approval decisions.

Flow:

- reconstruct a compact transcript focused on planned action
- ask a dedicated guardian review session for strict structured output
- apply allow/deny outcome or fail closed on timeout/malformed output/error

This is separate from sandbox enforcement. The design intentionally keeps:

- policy reasoning in guardian
- execution containment in sandbox/policy/tool orchestrator

That separation makes the system safer and more auditable.

## Memories

[memories/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/memories/mod.rs) describes a two-phase startup memory pipeline:

- Phase 1 extracts raw memories and rollout summaries from prior rollouts
- Phase 2 consolidates those outputs into durable memory artifacts

The implementation in [memories/phase1.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/memories/phase1.rs) and [memories/storage.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/memories/storage.rs) shows that memories are:

- driven by background startup jobs
- coordinated through the state DB
- materialized both as database rows and file artifacts under `codex_home/memories`
- later fed back into developer instructions and citation flows

This subsystem is operationally meaningful, not just prompt decoration.

## Realtime

[realtime_conversation.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/realtime_conversation.rs) manages a separate realtime conversation channel with:

- websocket-based transport
- audio input queueing
- text mirroring from normal turns
- backend handoff output streaming
- response-create coordination to avoid overlapping active responses

This subsystem is intentionally isolated behind `RealtimeConversationManager`, which keeps the rest of the session model mostly transport-agnostic.

## Key External Dependencies

Based on [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/core/Cargo.toml), the most important dependencies are workspace crates rather than third-party crates.

### Internal workspace dependencies

- `codex-api`: provider-facing Responses/realtime/compact/memory clients
- `codex-rollout`: thread persistence and history materialization
- `codex-tools`: tool specs and tool config model
- `codex-mcp` and `codex-rmcp-client`: MCP transport and auth/runtime integration
- `codex-models-manager` and `codex-model-provider-info`: model catalog/provider metadata
- `codex-sandboxing`, `codex-exec-server`, `codex-execpolicy`: sandbox and exec policy plumbing
- `codex-state` and `codex-thread-store`: SQLite-backed metadata and thread index support
- `codex-core-skills` and `codex-core-plugins`: skill/plugin loading and packaging
- `codex-analytics`, `codex-otel`, `codex-feedback`: observability

### Third-party infrastructure

- `tokio`, `futures`, `async-channel`, `tokio-util`: async runtime and coordination
- `reqwest`, `tokio-tungstenite`, `eventsource-stream`, `http`: transport stack
- `serde`, `serde_json`, `toml`, `toml_edit`: config and payload serialization
- `notify`: file watching for skill changes
- `uuid`, `chrono`: ids and time handling

### Platform-specific dependencies

- macOS: `core-foundation`
- Windows: `windows-sys`
- Unix: `codex-shell-escalation`
- musl targets: vendored `openssl-sys`

The dependency profile confirms that `codex-core` is orchestration-heavy and platform-aware.

## Testing Strategy

The crate has both extensive unit coverage and a large integration suite.

### Unit-style tests in `src`

- There are many colocated `*_tests.rs` files under `src`, covering config, tools, routing, sandboxing, memories, plugins, turn metadata, unified exec, thread management, and more.
- Snapshot-based tests exist in:
  - [src/guardian/snapshots](file:///Users/bytedance/project/codex/codex-rs/core/src/guardian/snapshots)
  - [src/session/snapshots](file:///Users/bytedance/project/codex/codex-rs/core/src/session/snapshots)

Representative modules:

- [config/config_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/config/config_tests.rs)
- [tools/router_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tools/router_tests.rs)
- [unified_exec/mod_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/unified_exec/mod_tests.rs)
- [thread_manager_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/thread_manager_tests.rs)
- [memories/phase1_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/memories/phase1_tests.rs)

### Aggregated integration suite

[tests/all.rs](file:///Users/bytedance/project/codex/codex-rs/core/tests/all.rs) builds a single integration binary, and [tests/suite/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/tests/suite/mod.rs) aggregates a broad suite of scenario tests:

- approvals and permissions
- compact/resume/fork
- exec and unified exec
- plugins and skills
- realtime conversation
- shell behavior and snapshots
- model switching/overrides
- memories
- rollback/undo
- websocket fallback and request compression

There is also substantial snapshot coverage under [tests/suite/snapshots](file:///Users/bytedance/project/codex/codex-rs/core/tests/suite/snapshots) for context layout and compaction behavior.

### Testing observations

- The suite is integration-heavy and behavior-oriented, which fits the crate’s orchestration role.
- Snapshot tests are used where exact model-visible layout matters.
- There is dedicated support for test binary aliasing to simulate `apply_patch` and Linux sandbox execution.
- The test surface strongly suggests the crate has accumulated many edge-case regressions and now protects them with scenario tests.

## Design Strengths

- Clear session/turn split: session-scoped state and turn-scoped state are distinct and intentionally reified.
- Queue-based control model: the `Submission -> Event` interface cleanly decouples UI/client layers from business logic.
- Strong orchestration boundaries: tools, approvals, sandboxing, model transport, persistence, and plugins are separated into recognizable subsystems.
- Fail-closed bias: guardian, approvals, sandbox denial handling, and managed constraints tend to reject unsafe or ambiguous paths rather than silently weaken enforcement.
- Good extensibility points: task trait, tool registry, skill/plugin roots, MCP integration, hooks, and feature flags all support incremental growth.
- Persistence-aware design: resume, fork, rollback, and compaction are first-class runtime behaviors rather than afterthoughts.

## Design Tradeoffs and Risks

- The crate is very large and subsystem-dense; understanding the full control flow requires crossing many modules.
- `Session::new` and `session/mod.rs` are high-complexity hotspots that concentrate many responsibilities.
- The runtime still carries legacy policy bridges between old `SandboxPolicy` and newer split filesystem/network policies.
- Many behaviors depend on feature flags and managed requirements, increasing state-space complexity.
- Rollout history, in-memory context, and SQLite metadata are all authoritative in different ways, which makes correctness subtle during resume/fork/rollback.
- Plugins, skills, MCP, and agent spawning all modify the effective tool/context surface dynamically, which increases reasoning burden.

## Open Questions and Follow-Up Ideas

- The TODO in [session/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/mod.rs) notes that `config.model` and `config.model_reasoning_effort` should probably be consolidated into `collaboration_mode`. That would simplify session configuration and reduce duplicate model-setting logic.
- Another TODO in [session/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/mod.rs) says context updates should become a pure diff against persisted `TurnContextItem` state. Today, some context derivation still depends on runtime-only inputs.
- [session/session.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/session.rs) and [turn_context.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/turn_context.rs) both carry comments about removing `original_config_do_not_use` / reducing mutable config cloning. The current per-turn config construction works, but it is a signal that config ownership is not fully settled.
- [tasks/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/tasks/mod.rs) contains a TODO to merge `idle_pending_input` with mailbox handling. The current split likely reflects historical layering rather than ideal design.
- [thread_manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/thread_manager.rs) notes missing explicit live-turn snapshot modes for forking. The current interrupt-or-truncate behavior may be sufficient now, but the API likely wants a more explicit sampling-boundary model.
- Given the centrality of [session/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/session/mod.rs), a future architectural extraction into smaller orchestration services may reduce maintenance risk.

## Bottom Line

`codex-core` is the orchestration heart of Codex.

It is responsible for turning a heterogeneous set of concerns:

- model access
- tool execution
- approvals and sandboxing
- persistence and replay
- plugins, skills, and MCP
- realtime and multi-agent workflows

into a single session/thread abstraction with durable history and event-driven execution.

The codebase is broad, but the architecture is coherent: `ThreadManager` owns threads, `Session` owns runtime orchestration, `TurnContext` packages per-turn state, `ModelClient` talks to providers, `ToolRouter` and `ToolRegistry` dispatch tools, `RolloutRecorder` preserves history, and optional subsystems plug into that central flow.
