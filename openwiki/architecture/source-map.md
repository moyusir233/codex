# Source map

Use this page to find the right part of the repository quickly. It is intentionally a navigation map, not a complete inventory of the 100+ Rust workspace members.

## Root-level files and docs

- `README.md`: user-facing Codex CLI introduction and install links.
- `docs/install.md`: source build requirements, Cargo build flow, `just` helper commands, and tracing notes.
- `docs/contributing.md`: contribution policy, local checks, model metadata guidance, PR expectations, and CLA/security notes.
- `docs/config.md`: external links for config docs plus lifecycle hook note for `allow_managed_hooks_only`.
- `codex-rs/config.md`: moved-config notice pointing to `docs/config.md` canonical links.
- `AGENTS.md`: repository-local engineering rules. Treat it as high-priority guidance when changing code.
- `justfile`: root command runner; defaults to `codex-rs` for most recipes.
- `package.json`: repo-wide maintenance scripts (`format`, `format:fix`, `write-hooks-schema`) and Node/pnpm versions.
- `MODULE.bazel`, `.bazelrc`, `defs.bzl`, `codex-rs/*/BUILD.bazel`: Bazel workspace, Rust toolchain, rules, and per-crate targets.

## Rust workspace manifest

`codex-rs/Cargo.toml` is the workspace source of truth for crates and internal dependency names. The workspace uses Rust 2024 edition and Apache-2.0 licensing. New crates should follow the existing `codex-*` crate naming convention.

Large categories in the manifest:

- User/runtime surfaces: `cli`, `tui`, `exec`, `app-server`, `app-server-client`, `app-server-daemon`, `mcp-server`.
- Core engine and protocols: `core`, `protocol`, `core-api`, `app-server-protocol`, `exec-server-protocol`, `code-mode-protocol`.
- Persistence/state: `rollout`, `thread-store`, `state`, `agent-graph-store`, `message-history`.
- Tools/sandboxing/execution: `exec`, `exec-server`, `execpolicy`, `sandboxing`, `linux-sandbox`, `windows-sandbox-rs`, `process-hardening`, `shell-command`, `shell-escalation`.
- Integrations: `codex-mcp`, `rmcp-client`, `connectors`, `core-plugins`, `plugin`, `hooks`, `ext/*`.
- Models/auth/network: `login`, `model-provider`, `model-provider-info`, `models-manager`, `backend-client`, `http-client`, `network-proxy`, `ollama`, `lmstudio`.
- Memory and context: `memories/read`, `memories/write`, `context-fragments`, `prompts`, `response-debug-context`.
- Utilities: many `utils/*` crates for paths, cache, image, CLI, pty, readiness, output truncation, templates, and other shared helpers.

## Where to start by task

| Task | Start here | Then inspect |
| --- | --- | --- |
| CLI flag/subcommand | `codex-rs/cli/src/main.rs` | Related module such as `mcp_cmd.rs`, `plugin_cmd.rs`, `doctor.rs`, `remote_control_cmd.rs`; CLI tests if present |
| Interactive TUI behavior | `codex-rs/tui/src/main.rs`, `codex-rs/tui/src/lib.rs` | `app/`, `chatwidget/`, `bottom_pane/`, snapshots and focused tests |
| App-server API | `codex-rs/app-server/src/main.rs`, `codex-rs/app-server/src/lib.rs` | `request_processors/`, `message_processor.rs`, `app-server-protocol/src/protocol/{v1,v2}.rs`, app-server suite tests |
| Core agent turn/session behavior | `codex-rs/core/src/lib.rs`, `core/src/session/`, `core/src/thread_manager.rs` | `core/src/tools/`, `client.rs`, `context/`, `context_manager/`, `core/tests/suite` |
| Protocol/event shape | `codex-rs/protocol/src/protocol.rs` | `protocol/src/items.rs`, `models.rs`, `response_item_id.rs`, app-server event mapping/tests |
| Tool spec or handler | `codex-rs/core/src/tools/spec_plan.rs` | `core/src/tools/handlers/*`, `codex-rs/tools`, suite tests for specific tool |
| Session persistence/resume/fork/list | `codex-rs/rollout/src/lib.rs`, `codex-rs/thread-store/src/types.rs` | `rollout/src/*`, `thread-store/src/*`, `state/src/*`, TUI resume picker/history tests |
| MCP integration | `codex-rs/docs/codex_mcp_interface.md` | `codex-rs/codex-mcp`, `rmcp-client`, `mcp-server`, core MCP tool call files/tests |
| Plugins/apps/connectors | `codex-rs/core-plugins/src/lib.rs`, `connectors/src/lib.rs` | `core-plugins/src/manager.rs`, `connectors/src/connector_runtime/`, app-server plugin/request processors |
| Skills | `codex-rs/ext/skills/src/lib.rs` | `catalog`, `selection`, `dynamic_skill_selector`, `shadow_selection_experiment.rs`, skills tests |
| Hooks | `codex-rs/hooks/src/lib.rs` | `hooks/src/events/*`, `registry.rs`, `schema.rs`, core hook runtime/tests |
| Memories | `codex-rs/memories/README.md` | `memories/read`, `memories/write`, startup tests and state DB interactions |
| Bazel build issue | `codex-rs/docs/bazel.md` | `MODULE.bazel`, `defs.bzl`, crate `BUILD.bazel`, lockfile/update scripts |

## High-risk source surfaces

These areas tend to affect multiple clients or persistent data:

- `codex-rs/protocol` and `codex-rs/app-server-protocol`: schema/event/API compatibility.
- `codex-rs/core/src/session`, `thread_manager.rs`, and `rollout`/`thread-store`: turn lifecycle, resume/fork, and persisted history.
- `codex-rs/core/src/tools` and `codex-rs/tools`: model-visible tool surface and approval/sandbox behavior.
- `codex-rs/tui/src/app_server_session.rs`: TUI-to-app-server/core bridge; recent reasoning/model changes touched this file.
- `codex-rs/core-plugins`, `connectors`, `codex-mcp`, and app-server plugin processors: external app/plugin/MCP behavior.
- Config types and schema: when `ConfigToml` or nested config types change, regenerate `codex-rs/core/config.schema.json` with `just write-config-schema`.

## Existing documentation worth linking, not duplicating

- `codex-rs/docs/protocol_v1.md`: protocol concepts and example flows.
- `codex-rs/docs/codex_mcp_interface.md`: experimental MCP server interface.
- `codex-rs/docs/bazel.md`: Bazel/BuildBuddy/local vs CI build behavior.
- `codex-rs/memories/README.md`: memory pipeline phases and artifacts.
- `docs/install.md`: source build and tracing setup.
- `docs/contributing.md`: contribution workflow and local checks.
