# Architecture overview

Codex is a layered Rust application: command-line entrypoints and UI clients drive a local agent engine through typed protocols, while persistence, plugins, tools, and integration surfaces live in separate workspace crates. This page gives the shortest useful path through those layers.

## Runtime layers

```text
User command or IDE/app client
  -> `codex-rs/cli` / `codex-rs/tui` / `codex-rs/app-server`
  -> `codex-rs/protocol` and `codex-rs/app-server-protocol` message types
  -> `codex-rs/core` thread/session runtime
  -> model client, tools, MCP, exec/sandbox, plugins, skills, memories
  -> rollout/thread-store/state persistence
```

Important source references:

- `codex-rs/cli/src/main.rs`: top-level `codex` command and subcommands.
- `codex-rs/tui/src/main.rs`: standalone TUI binary wrapper around `codex_tui::run_main`.
- `codex-rs/app-server/src/main.rs`: app-server binary options (`--listen`, `--session-source`, websocket auth, strict config, remote control).
- `codex-rs/app-server/src/lib.rs`: app-server runtime, transports, message processing, config, outgoing routing, and startup logging.
- `codex-rs/core/src/lib.rs`: public exports and internal module map for the core agent engine.
- `codex-rs/core/src/session/session.rs`: session state/configuration and permission/sandbox helpers.
- `codex-rs/core/src/thread_manager.rs`: thread lifecycle, creation, resume, fork, storage, and multi-agent parent/child metadata.
- `codex-rs/protocol/src/protocol.rs`: core submission/event protocol and shared context tags.
- `codex-rs/docs/protocol_v1.md`: architecture vocabulary for model, Codex, session, task, turn, SQ/EQ submissions/events.

## CLI and TUI entrypoints

`codex-rs/cli/src/main.rs` defines `MultitoolCli`. With no subcommand, options are forwarded to the interactive CLI/TUI. Subcommands include non-interactive `exec`, `review`, login/logout, MCP server management, plugin management, app-server/remote-control tools, desktop app launchers on supported platforms, completion generation, update, doctor, sandbox/debug tools, session resume/archive/delete/unarchive/fork, cloud tasks, and feature inspection.

The standalone TUI binary in `codex-rs/tui/src/main.rs` is thinner: it parses shared config overrides, calls `run_main`, and formats exit messages such as token usage, resume hints, and fatal session IDs. Most TUI behavior lives in `codex-rs/tui/src/lib.rs`, `app.rs`, `app_server_session.rs`, `chatwidget/`, `bottom_pane/`, and many focused submodules.

Design note from `AGENTS.md`: large TUI orchestration files are high-touch. Prefer adding focused modules and moving nearby tests with extracted logic instead of growing `app.rs`, `chatwidget.rs`, or `bottom_pane/chat_composer.rs`.

## App server layer

The app server is the integration boundary for IDE/app clients and MCP-style control surfaces.

- `codex-rs/app-server/src/main.rs` starts the server with a transport URL. Supported listen modes include `stdio://`, `unix://`, `ws://IP:PORT`, and `off`.
- `codex-rs/app-server/src/lib.rs` wires transports, remote-control startup, config layers, auth policy, connection cleanup, message processing, outgoing routing, and logging/OTel setup.
- `codex-rs/app-server-protocol/src/protocol/mod.rs` organizes protocol modules: common types, v1/v2 RPCs, event mapping, item builders, thread history, and thread history projection.
- `codex-rs/docs/codex_mcp_interface.md` documents the experimental MCP server interface exposed by `codex mcp-server`/`codex-mcp-server`.

New integrations should prefer v2 thread/turn APIs where possible (`thread/start`, `thread/resume`, `thread/fork`, `thread/read`, `thread/list`, `turn/start`, `turn/steer`, `turn/interrupt`). Legacy v1 compatibility methods remain for older clients.

## Core engine: threads, sessions, and turns

The core engine maintains a thread/session runtime that maps user turns to model calls, tool execution, approvals, event emission, and persistence.

`codex-rs/docs/protocol_v1.md` defines the key vocabulary:

- **Codex**: local engine, operated via submission queue/event queue style messages.
- **Session**: current configuration and state. It starts through `Op::ConfigureSession`; reconfiguration aborts running work.
- **Task**: work in response to user input; at most one task runs per session.
- **Turn**: one loop of model request, streamed response collection, tool/patch execution, approval pauses, and output fed into the next turn.

In code:

- `Session` in `codex-rs/core/src/session/session.rs` holds the `thread_id`, event sender, `SessionState`, feature set, MCP refresh state, realtime conversation manager, active turn, input queue, guardian review session, and service handles.
- `SessionConfiguration` carries provider/model behavior, collaboration mode, reasoning summary, developer/base instructions, personality, approval policy, permission profile state, sandbox config, environment selections, workspace roots, Codex home, thread metadata, dynamic tools, source labels, and history mode.
- `ThreadManager` in `codex-rs/core/src/thread_manager.rs` creates/resumes/forks threads, coordinates `ThreadStore` implementations, maps fork snapshots, tracks parent/subagent relationships, and emits initial `SessionConfigured` events.

## Protocol and event mapping

`codex-rs/protocol/src/protocol.rs` is the shared type source for the core agent protocol. It defines context tags such as `<user_instructions>`, `<environment_context>`, `<skills_instructions>`, `<plugins_instructions>`, `<collaboration_mode>`, and `<multi_agent_mode>`, plus core structs such as `TurnEnvironmentSelection`.

Two protocol surfaces are easy to confuse:

- `codex-rs/protocol`: internal/core Op/Event, item, permission, model, and user-input types used across core, TUI, exec, and tests.
- `codex-rs/app-server-protocol`: JSON-RPC request/response/notification types and schema fixtures for external app-server clients.

Changes to either surface are breaking-risk areas. `AGENTS.md` explicitly calls out external integration surfaces: app-server APIs, raw response item events (`rawResponseItem/*`), CLI parameters, configuration loading, and resuming sessions from existing rollouts.

## Tool planning and execution

Tool definitions and dispatch are centered under `codex-rs/core/src/tools/`.

- `tools/mod.rs` defines tool-mode selection and output formatting/truncation helpers.
- `tools/spec_plan.rs` plans available tools for a turn, including shell/exec, apply-patch, current-time, MCP resources/tools, dynamic tools, plugin install requests, request-user-input, permissions, web search, image generation, code mode, and multi-agent variants.
- `tools/handlers/` owns implementation-specific handlers and tests. Many specs have adjacent `*_spec.rs` and `*_tests.rs` files.

Recent history shows multi-agent tooling is actively changing. `92938d880` restricted spawned-agent models to the active backend, touching multi-agent specs/tests, protocol model metadata, and TUI session code. When changing model selection or spawned-agent behavior, inspect both core tool specs and UI/app-server request plumbing.

## Persistence model

Persistence is split deliberately:

- `codex-rs/rollout`: JSONL-style rollout persistence, compression, listing/search, session index, archived sessions, state DB bridge, and policies for which rollout items are durable.
- `codex-rs/thread-store`: live thread store abstractions and local/in-memory implementations. `types.rs` captures thread creation/resume metadata including session/thread IDs, parent/fork IDs, source, originator, base instructions, dynamic tools, selected roots, multi-agent version, history mode, and persistence metadata.
- `codex-rs/state`: state extraction and SQLite-backed projections used by app-server/thread metadata and memory flows.

Recent `5c19155cb` added ordinals to paginated rollout records, and `769a5de25` updated thread metadata sync for explicit advanced reasoning selection. When changing resume/fork/list/history behavior, inspect rollout, thread-store, app-server protocol projection, TUI resume/history tests, and core suite tests together.

## Extension and integration subsystems

The core runtime composes several extension-like systems:

- MCP: `codex-rs/codex-mcp`, `codex-rs/rmcp-client`, `codex-rs/mcp-server`, and app-server MCP docs.
- Plugins/apps: `codex-rs/core-plugins`, `codex-rs/plugin`, `codex-rs/connectors`, and app-server plugin processors.
- Skills: `codex-rs/ext/skills`, `codex-rs/core-skills`, `codex-rs/skills`.
- Hooks: `codex-rs/hooks` and core hook runtime.
- Memories: `codex-rs/memories/read`, `codex-rs/memories/write`, and startup orchestration from core.

See [../domain/integrations.md](../domain/integrations.md) for deeper navigation and change risks.
