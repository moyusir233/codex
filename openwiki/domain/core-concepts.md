# Core domain concepts

## Thread, session, task, and turn

`codex-rs/docs/protocol_v1.md` supplies the shared vocabulary, with the caveat that its spec may lag source:

- **Thread**: durable conversation/work unit; current code prefers “thread,” while `core/src/lib.rs` retains deprecated conversation aliases.
- **Session**: active configuration and runtime state for a thread.
- **Task**: work started by user input; at most one runs per session.
- **Turn**: one model request/stream/tool cycle. Tool output can feed another turn until completion, interruption, or failure.

`core/src/thread_manager.rs`, `codex_thread.rs`, and `session/` are the implementation anchors.

## Context and user input

`Op::UserTurn` carries content and per-turn context. User input can contain text, images, explicit skills, and app/connector mentions (`protocol/src/user_input.rs`, `docs/protocol_v1.md`). Core incrementally assembles model-visible history plus bounded contextual fragments.

`AGENTS.md` treats context as a performance and safety contract: do not rewrite history, avoid cache-breaking churn, enforce hard bounds, and define injected fragments as types under `core/context`. Tool descriptions, skills, plugin instructions, environment context, and collaboration/multi-agent state all consume context budget.

## Models and providers

Three layers are distinct:

- `model-provider-info`: serializable provider registry/configuration, including built-ins and user-defined providers.
- `model-provider`: runtime provider abstraction for auth, capabilities, API adaptation, account state, attestation, model managers, and preferred models for reviews/memory.
- `models-manager`: bundled, static, and remotely refreshed model catalogs plus cache/filtering behavior.

The supported wire API is Responses; legacy chat-style provider settings produce migration errors. Model/reasoning changes frequently cross core client construction, TUI catalogs/settings, app-server model APIs, and persisted thread metadata.

## Tools

`core/src/tools/spec_plan.rs` assembles the model-visible tool set; `tools/handlers/` dispatches implementations. Categories include shell/exec, patching, user input/permissions, MCP and dynamic tools, plugin requests, web/image functions, code mode, and multi-agent control.

Treat tool name, schema, description, ordering, and output truncation as model-facing APIs. A “small” spec change can alter model behavior and requires behavioral tests.

## Approval, policy, and sandboxing

These mechanisms are related but not interchangeable:

- **Approval policy** decides whether an action may proceed automatically, must be reviewed, or is forbidden.
- **Permission-request hooks** may allow or deny first.
- **Reviewer routing** sends unresolved requests to Guardian or the user (`core/src/tools/approvals.rs`).
- **Exec policy** independently classifies commands and can produce policy amendments (`execpolicy/`, `core/src/exec_policy.rs`).
- **Sandbox policy** constrains filesystem/network/process access; reusable implementations live in `sandboxing/`, `linux-sandbox/`, and Windows sandbox crates, while core orchestrates them.

Session approvals can be cached by approval key; apply-patch may require all affected file keys. Denied-read restrictions cannot be bypassed by ordinary unsandboxed escalation, because that would silently grant forbidden access. Changes in this area require integration and platform-aware tests.

## Persistence and compatibility

- `rollout`: durable JSONL items, compression, session indexes, archives, ordinals.
- `thread-store`: local/in-memory thread state and metadata synchronization.
- `state`: SQLite projections/extraction used by listing, diagnostics, app-server, and memory.

Stored history is an external compatibility surface because users and clients resume, fork, list, and archive older threads. Verify write, replay, projection, and UI restoration paths together.

## Multi-agent

Multi-agent behavior spans `core/src/tools/handlers/multi_agents*`, `thread_manager.rs`, `agent-graph-store`, protocol model metadata, thread-store parent/fork metadata, app-server, and TUI controls. A spawned agent is a related thread with constrained inherited configuration, not parallel work inside one session task.

Recent model-override restrictions (`92938d880`) demonstrate that spawn configuration must remain compatible with the active backend and client-visible model choices.

## Skills

Skills are prompt resources that can be explicitly or implicitly invoked:

- `core-skills` owns host discovery/loading, policy, namespacing, rendering budgets, and service APIs.
- `ext/skills` provides typed Host/Executor/Orchestrator integration and read/selection behavior.

The weighted lexical selector added in `c10010928` is used for a shadow metrics experiment. `2b0b37abb` aligned candidates with invocation-observable sources. Do not describe this experiment as the production selection path without checking current call sites.

## Plugins, connectors, and extensions

- A typed **extension** contributes runtime behavior through `ext/extension-api`.
- A **plugin** is an installable manifest/package containing skills, MCP servers, apps, hooks, and presentation metadata.
- A **connector** supplies app/tool metadata and account/workspace-scoped live MCP tool snapshots.

See [Integration points](integrations.md) for ownership boundaries.

## Memories

Memory reads and writes are separate crates. `memories/write/src/start.rs` starts the asynchronous pipeline only for non-ephemeral root sessions with `MemoryTool` enabled and an available state DB:

1. Phase 1 claims bounded eligible rollouts and extracts/redacts per-rollout memory into the DB.
2. Phase 2 serializes global consolidation into a git-backed memory workspace and can run a restricted no-network consolidation agent.

Current orchestration lives in `memories/write/`; the statement in `memories/README.md` that it remains under `core/src/memories/` is stale. Parent permission profiles are deliberately preserved or narrowed during consolidation (`54b8f112a`).

## Configuration and features

Core config types live under `core/src/config/`, generated schema at `core/config.schema.json`, and feature definitions in `features/`. Config changes need loader/strict-config tests and `just write-config-schema`; feature changes need tests at both the registry and consuming product surface.
