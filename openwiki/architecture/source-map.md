# Source map

This is a task-oriented map, not an inventory of every Rust workspace member.

## Repository control files

- `README.md`: end-user introduction and install links.
- `AGENTS.md`: authoritative repository-local engineering and test rules.
- `docs/install.md`, `docs/contributing.md`, `docs/config.md`: setup, contribution, and config links/notes.
- `justfile`: standard local commands; most recipes execute in `codex-rs/`.
- `codex-rs/Cargo.toml`: workspace members, shared dependencies, Rust 2024 edition.
- `MODULE.bazel`, `defs.bzl`, `.bazelrc`, per-crate `BUILD.bazel`: hermetic/cross-platform build mirror.
- `package.json`, `pnpm-workspace.yaml`: maintenance tooling and SDK/package workspace.
- `.github/workflows/`: blocking, fast Rust, full/postmerge, Bazel, repository, and SDK checks.

## Find code by task

| Change | Primary sources | Follow-through |
| --- | --- | --- |
| CLI flag or command | `codex-rs/cli/src/main.rs` | Command module and parsing tests |
| Interactive UI | `tui/src/lib.rs`, `app/`, `chatwidget/`, `bottom_pane/` | Focused `*_tests.rs` and snapshots |
| Core turn/session | `core/src/session/`, `codex_thread.rs`, `thread_manager.rs` | context, client, `core/tests/suite/` |
| Model-visible tool | `core/src/tools/spec_plan.rs` | handlers, approvals/sandboxing, suite tests |
| Approval/sandbox | `core/src/tools/{approvals,sandboxing}.rs` | `execpolicy/`, sandbox crates, Guardian, hooks |
| Internal Op/Event | `protocol/src/protocol.rs` | items, models, event mapping, clients |
| External app API | `app-server/src/request_processors/` | app-server protocol, schemas, suite tests |
| Resume/fork/history | `rollout/`, `thread-store/`, `state/` | history projection and UI/core tests |
| Model/provider | `model-provider/`, `model-provider-info/`, `models-manager/` | core client, model list, TUI settings |
| Typed extension | `ext/extension-api/src/` | `app-server/src/extensions.rs`, concrete `ext/*` |
| Installable plugin | `plugin/src/manifest.rs`, `core-plugins/` | app-server plugin processors and policy tests |
| MCP client | `codex-mcp/`, `rmcp-client/` | core MCP exposure/calls and auth tests |
| MCP server | `mcp-server/`, `docs/codex_mcp_interface.md` | adapter/interface tests |
| Connector tools | `connectors/src/lib.rs` | connector runtime, codex-mcp, app-server MCP processor |
| Host skills | `core-skills/` | core skill injection/service and policy tests |
| Skills extension | `ext/skills/` | providers, read tools, selector experiment/tests |
| Lifecycle hook | `hooks/src/` | core hook runtime, approvals, hook schemas |
| Memory pipeline | `memories/read/`, `memories/write/` | startup, state DB, sandbox tests |
| Config/feature | `core/src/config/`, `features/` | schema, app-server config, surface tests |

## Key domain clusters

### Client and runtime

`cli`, `tui`, `exec`, `app-server`, `app-server-daemon`, `app-server-client`, `core`, `protocol`, and `app-server-protocol` form the primary user-to-agent path.

### Execution and safety

`tools`, `exec`, `exec-server`, `execpolicy`, `shell-command`, `shell-escalation`, `sandboxing`, `linux-sandbox`, Windows sandbox crates, and `process-hardening` implement execution below core policy orchestration.

### Persistence and memory

`rollout`, `thread-store`, `state`, `agent-graph-store`, `memories/read`, and `memories/write` own durable history, projections, and reusable memory. `codex-rs/memories/README.md` still says Phase 1/2 orchestration is under `core/src/memories/`; current source puts startup and phases in `memories/write/`.

### Integration and model infrastructure

`ext/*`, `plugin`, `core-plugins`, `connectors`, `codex-mcp`, `rmcp-client`, `hooks`, `core-skills`, `model-provider`, `model-provider-info`, `models-manager`, `login`, and network/auth crates form the extensibility boundary.

## Existing deep documentation

- `codex-rs/docs/protocol_v1.md`: terminology and example flows; explicitly a potentially lagging spec.
- `codex-rs/docs/codex_mcp_interface.md`: experimental MCP server interface.
- `codex-rs/docs/bazel.md`: Cargo/Bazel relationship and BuildBuddy behavior.
- `codex-rs/memories/README.md`: detailed memory phases and artifacts, with the ownership caveat above.
- `codex-rs/tui/styles.md`: TUI styling conventions.

## High-risk search checklist

Before changing shared behavior, search both protocol crates and schemas; thread manager plus all persistence layers; tool specs plus approval/sandbox code; TUI settings/session/snapshots; app-server processors and v2 tests; and both Cargo and Bazel metadata.
