# Architecture overview

Codex is a layered Rust system. CLI, TUI, and external app clients drive a thread/session runtime through typed protocols; the runtime builds model context, streams Responses API output, dispatches tools through approval and sandbox policy, emits events, and persists durable history.

```text
User / IDE / app
  -> CLI + TUI, non-interactive exec, or app-server transport
  -> internal `codex-protocol` Op/Event or external app-server JSON-RPC
  -> ThreadManager + CodexThread + Session/Turn runtime
  -> model provider/client + context + tool plan
  -> approval hooks / Guardian / user review -> sandboxed execution
  -> events to clients + rollout/thread-store/state persistence
```

## Product entrypoints

- `codex-rs/cli/src/main.rs` defines the `codex` multitool. With no subcommand it forwards to the interactive TUI; commands include `exec`, `review`, auth, MCP/plugin management, MCP server, app server, remote control, doctor, sandbox/debug tools, session lifecycle, cloud tasks, and features.
- `codex-rs/tui/src/main.rs` is a thin binary wrapper; `tui/src/lib.rs`, `app/`, `chatwidget/`, `bottom_pane/`, and `app_server_session.rs` own interactive behavior.
- `codex-rs/exec` provides non-interactive operation.
- `codex-rs/app-server` exposes Codex to IDE/app clients. `app-server/src/main.rs` accepts `stdio://`, Unix socket, WebSocket, or `off` transports, a session source, strict config, auth, and remote-control options.

The app server is not itself “the MCP server.” The experimental `codex mcp-server` command is a separate stdio MCP adapter in `codex-rs/mcp-server/`; app-server is the broader JSON-RPC application boundary.

## Runtime ownership

`codex-rs/core` remains the main orchestration crate, but repository guidance explicitly discourages growing it when a narrower crate can own new behavior.

- `core/src/thread_manager.rs`: create, resume, fork, store, and shut down threads; parent/subagent relationships.
- `core/src/codex_thread.rs`: public thread handle and settings snapshots.
- `core/src/session/`: session state, active turns, per-turn configuration, event flow, and service coordination.
- `core/src/context/` and `context_manager/`: bounded model-visible history and contextual fragments.
- `core/src/client.rs`: model client/session plumbing.
- `core/src/tools/spec_plan.rs`: model-visible tool inventory for a turn.
- `core/src/tools/handlers/`: tool implementations and focused tests.

`codex-rs/docs/protocol_v1.md` is useful vocabulary but warns that code may not completely match the document. A **thread** is the durable work/conversation, a **session** is active runtime configuration/state, a **task** responds to user input, and a **turn** is one model/tool iteration.

## Protocol boundaries

- `codex-rs/protocol`: internal shared Rust submissions (`Op`), events (`EventMsg`), model metadata, user input, permissions, and items. These are primarily in-process types, not a general stable wire API.
- `codex-rs/app-server-protocol`: external JSON-RPC requests, responses, notifications, history projections, and generated TypeScript/JSON schemas.

New app clients should prefer v2 thread/turn methods (`thread/start`, `thread/resume`, `thread/fork`, `thread/read`, `thread/list`, `turn/start`, `turn/steer`, `turn/interrupt`). Protocol changes should be traced into app-server event mapping, generated schemas, clients, persistence replay, and TUI handling.

## A turn through the system

1. A client starts/resumes a thread and submits a user turn with per-turn context.
2. Core assembles bounded history, instructions, environment state, skills/plugins, and available tool specs.
3. The selected `ModelProvider` creates model services; the Responses API streams model items.
4. Text and item deltas become events. Tool calls route to native, MCP, dynamic, or extension handlers.
5. A permission-request hook may resolve approval first; otherwise the configured reviewer is Guardian or the user (`core/src/tools/approvals.rs`). Exec policy, approval policy, and sandbox policy remain distinct.
6. Commands and patches execute under the resolved platform sandbox; outputs are bounded before returning to model context.
7. Events flow to the client and durable items/metadata flow to rollout, thread-store, and state projections.
8. Additional tool output can trigger another turn; completion, interruption, or fatal error ends the task.

## Persistence

- `codex-rs/rollout`: durable rollout/session JSONL, compression, listing, archives, ordinals, and persistence policy.
- `codex-rs/thread-store`: live local/in-memory thread records and metadata synchronization.
- `codex-rs/state`: SQLite-backed extraction and projections used by listing, app-server, diagnostics, and memories.
- `codex-rs/agent-graph-store`: agent/thread relationships.

Resume, fork, list, archive, and history behavior often crosses all three primary persistence crates. Recent rollout ordinals (`5c19155cb`) and advanced-reasoning metadata (`769a5de25`) show why stored items, derived projections, and UI restoration must be tested together.

## Extension composition

There are three separate concepts:

1. `codex-rs/ext/extension-api` is an in-process typed registry. Contributors participate in thread/turn lifecycle, configuration, context, token usage, skills, MCP servers, turn inputs/items, native tools, tool lifecycle, and approval review.
2. `codex-rs/plugin` defines installable package manifests; `core-plugins` handles marketplaces, installation, policy, startup sync, and effective state. Packages may declare skills, MCP servers, apps, hooks, and UI metadata.
3. Built-in extensions under `codex-rs/ext/*` implement concrete capabilities such as skills, MCP, connectors, memories, Guardian, image generation, and web search.

Do not use “plugin,” “extension,” and “MCP server” interchangeably. See [Integration points](../domain/integrations.md).

## Architectural pressure points

- `codex-core` and central TUI files are protected from unrelated growth (`AGENTS.md`).
- Tool specs and contextual fragments are model-facing APIs; wording, ordering, and size affect behavior and caching.
- App-server/protocol and rollout formats are compatibility surfaces.
- Approval and sandbox changes are safety-sensitive and platform-specific.
- Model/reasoning settings cross provider catalogs, UI, app-server, and persisted metadata.
- Cargo and Bazel metadata must remain synchronized.
