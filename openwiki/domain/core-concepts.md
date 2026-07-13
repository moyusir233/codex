# Core domain concepts

This page defines the main product and technical concepts that recur across Codex source files. Use it before modifying behavior so you can search for the right names and understand compatibility risks.

## Thread, session, task, and turn

The clearest vocabulary source is `codex-rs/docs/protocol_v1.md`, backed by types in `codex-rs/protocol/src/protocol.rs` and runtime code in `codex-rs/core/src/session/`.

- **Codex**: the local engine. It communicates with clients through submissions and events.
- **Thread**: the durable conversation/work unit. Current code favors “thread”; older symbols still expose deprecated “conversation” aliases in places such as `codex-rs/core/src/lib.rs`.
- **Session**: current configuration and runtime state for an initialized model agent. `SessionConfiguration` includes model provider, collaboration mode, developer/base instructions, personality, approval policy, permission profile state, environment selections, workspace roots, dynamic tools, session source, history mode, and thread relationships.
- **Task**: work in response to user input. A session has at most one running task at a time.
- **Turn**: one model/tool iteration loop: send request to model, collect streamed response, execute tool calls or patches, pause for approvals if needed, emit events, then feed outputs into a later turn if necessary.

The core runtime enforces these concepts mainly through `codex-rs/core/src/session/session.rs` and `codex-rs/core/src/thread_manager.rs`.

## User input and protocol events

`codex-rs/protocol/src/protocol.rs` defines shared structures and model-visible context tags. `Op::UserTurn` content can include text, images, skills, and mentions, as described in `codex-rs/docs/protocol_v1.md`. `EventMsg` variants report assistant text, streaming deltas, plan deltas, approval requests, turn start/completion, warnings, errors, and raw item events.

Compatibility watchouts:

- Core protocol events are not identical to app-server JSON-RPC types.
- `EventMsg::TurnStarted`/`TurnComplete` have v1 wire compatibility tags (`task_started`/`task_complete`) per protocol docs.
- `AGENTS.md` flags raw response item events and session resume behavior as breaking-change surfaces.

## Permissions, approvals, sandboxing, and guardian review

Permission and sandbox data flows through protocol/config types, `SessionConfiguration`, tool handlers, and execution code.

Key areas:

- `codex-rs/core/src/exec_policy.rs` and tests: execution policy and warnings.
- `codex-rs/core/src/sandboxing/`, `windows_sandbox.rs`, `landlock.rs`: platform sandbox support.
- `codex-rs/core/src/tools/handlers/request_permissions.rs`: permission request tool behavior.
- `codex-rs/core/src/guardian/`: guardian review prompt/session/policy behavior.
- `codex-rs/protocol/src/protocol.rs`: approval and permission event exports.

Recent `ea1545628` aligned Guardian reviews with session configuration, and `bbdf3030d` adjusted lifecycle behavior after guardian interrupts. Changes here should include integration coverage because approval/safety behavior is user-visible.

## Tools

The model-visible tool surface is assembled in `codex-rs/core/src/tools/spec_plan.rs` and executed by handlers in `codex-rs/core/src/tools/handlers/`.

Major tool categories include:

- Shell/exec/unified exec and stdin writing.
- Apply patch.
- Current time, sleep, context remaining, and new context window.
- MCP resources/tools and dynamic extension tools.
- Request user input and request permissions.
- Plugin installation/listing requests.
- Web search and image viewing/generation.
- Code mode tools.
- Multi-agent spawn/send/wait/resume/interrupt/follow-up flows.

`tools/mod.rs` also centralizes truncation/formatting of exec output for model consumption. Treat tool specs as model-visible API: small wording or schema changes can alter model behavior and tests.

## Multi-agent

Multi-agent support appears across core tools, protocol model metadata, TUI session code, thread-store metadata, analytics, and parent/child thread relationships.

Important files:

- `codex-rs/core/src/tools/handlers/multi_agents_spec.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/`
- `codex-rs/core/src/thread_manager.rs`
- `codex-rs/thread-store/src/types.rs`
- `codex-rs/tui/src/multi_agents.rs`
- `codex-rs/core/tests/suite/subagent_notifications.rs`, `spawn_agent_description.rs`, `multi_agent_mode.rs`

Recent `92938d880` restricted spawned-agent models to the active backend. If changing multi-agent model overrides or spawned-agent configuration, inspect protocol metadata, app/TUI UI controls, and core tool tests together.

## Persistence: rollouts, thread store, and state DB

Codex persists durable conversation history and derived metadata through several crates:

- `codex-rs/rollout`: session/rollout JSONL persistence, compression, listing/search, archived sessions, session index, SQLite state DB hooks, and persistence policy.
- `codex-rs/thread-store`: live thread store abstraction and local/in-memory implementations. `CreateThreadParams` and `ResumeThreadParams` encode the durable metadata contract.
- `codex-rs/state`: extraction/projection code used by thread metadata sync and other systems.

Recent history around rollout ordinals and reasoning metadata shows persistence is actively evolving. When changing what is stored, verify both replay and list/projection paths.

## Skills

Skills provide model-visible prompt resources selected explicitly or implicitly.

Source map:

- `codex-rs/ext/skills/src/lib.rs`: extension module exports.
- `catalog`, `provider`, `selection`, `render`, `sources`, `state`, `tools`: skill loading/rendering/read paths.
- `dynamic_skill_selector/weighted_lexical.rs`: cheap lexical selection algorithm added recently.
- `shadow_selection_experiment.rs`: temporary metrics-only shadow experiment.

Recent `c10010928` added weighted lexical shadow metrics and `2b0b37abb` aligned the candidate set with observable sources. The experiment filters enabled, prompt-visible Host/Orchestrator skills to match invocation observation. Do not treat shadow selection as the active selection path without verifying current call sites.

## Plugins, apps, and connectors

Plugins and app connectors are related but distinct layers:

- `codex-rs/plugin`: plugin manifest/capability primitives.
- `codex-rs/core-plugins`: plugin marketplace/install/load/remote/startup-sync manager.
- `codex-rs/connectors`: app directory metadata and connector runtime snapshots for connector-backed MCP tools.
- `codex-rs/app-server/src/request_processors/plugins.rs`: app-server plugin operations.

Recent `076a110eb` changed trust for hooks from materialized workspace plugins, and `2f7d89b14` extracted connector runtime snapshot management. These are integration boundaries; inspect tests before assuming a single crate owns behavior.

## Memories

`codex-rs/memories/README.md` documents the memory pipeline:

- `memories/read`: read path, memory developer-instruction injection, citation parsing, telemetry classification.
- `memories/write`: write path, Phase 1 extraction, Phase 2 consolidation prompts/artifacts, workspace diff helpers.
- Phase 1 extracts per-rollout memories from recent eligible rollouts using state DB claims and retry/backoff.
- Phase 2 serializes global consolidation, updates filesystem artifacts under the memories root, and may spawn an internal no-network consolidation sub-agent.

Recent `54b8f112a` preserved parent sandbox enforcement for memory consolidation, so sandbox inheritance is important when editing memory agents.

## Configuration and feature flags

Configuration types and schema live primarily under `codex-rs/core/src/config/`, `codex-rs/config`, and `codex-rs/core/config.schema.json`. Features live under `codex-rs/features`.

When changing config structs, run `just write-config-schema`. When changing feature behavior, inspect tests in `codex-rs/features/src/tests.rs` and any product surface that exposes the feature.
