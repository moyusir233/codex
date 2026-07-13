# Codex repository quickstart

This wiki is an engineer-oriented map for the local Codex repository. Codex CLI is a local coding agent from OpenAI; the public `README.md` describes user installation and points to hosted product documentation, while this wiki focuses on how the repository is organized and where future agents should look before changing code.

## What this repository contains

The active implementation is the Rust workspace under `codex-rs/`. The root also contains Bazel files, release/install documentation, scripts, SDK/package metadata, and repo-wide maintenance tooling.

High-level areas:

- `codex-rs/cli`: the `codex` multitool binary. `codex-rs/cli/src/main.rs` defines subcommands such as interactive TUI, `exec`, `review`, login/logout, MCP, plugins, app-server, session resume/fork/archive/delete, sandbox/debug tooling, cloud tasks, and feature inspection.
- `codex-rs/tui`: the terminal UI and interactive session client. `codex-rs/tui/src/main.rs` calls `codex_tui::run_main`; many UI flows are split under `tui/src/app/`, `chatwidget/`, and `bottom_pane/`.
- `codex-rs/core`: the local agent engine. `codex-rs/core/src/lib.rs` exports thread/session management, config, execution, sandboxing, tools, MCP, plugins, skills, rollout persistence, and model/client plumbing.
- `codex-rs/protocol`: shared Op/Event, user-input, permission, model, item, and context types. `codex-rs/docs/protocol_v1.md` explains the SQ/EQ mental model.
- `codex-rs/app-server` and `codex-rs/app-server-protocol`: JSON-RPC/MCP-like control surface for IDE/app integrations, threads, turns, accounts, config, model listing, file search, and approvals.
- `codex-rs/rollout`, `codex-rs/thread-store`, and `codex-rs/state`: local thread/session persistence and SQLite-backed projections.
- `codex-rs/ext/*`, `core-plugins`, `connectors`, `hooks`, `memories`, and `skills`: extension systems, plugin/app/connector integration, lifecycle hooks, memory pipelines, and skill selection/rendering.

## Start here

1. Read the architecture map: [architecture/overview.md](architecture/overview.md).
2. Use the source-oriented navigation map: [architecture/source-map.md](architecture/source-map.md).
3. Before changing behavior, read the domain glossary and invariants: [domain/core-concepts.md](domain/core-concepts.md).
4. For MCP, app-server, plugins, connectors, skills, and hooks, read [domain/integrations.md](domain/integrations.md).
5. For commands, build/test/run notes, schema regeneration, and operational cautions, read [workflows/engineer-runbook.md](workflows/engineer-runbook.md).
6. For choosing test targets, read [workflows/testing-guidance.md](workflows/testing-guidance.md).

## Local setup and common commands

The public build instructions in `docs/install.md` say to work from `codex-rs/` for Cargo commands and to use the root `justfile` helpers. The root `justfile` sets `working-directory := "codex-rs"`, so `just` commands can generally be run from the repository root.

```bash
# Build the workspace from source
cd codex-rs && cargo build

# Run the interactive CLI/TUI from source
just codex "explain this codebase to me"

# Run non-interactive mode
just exec "summarize the current repository"

# Format all repo-supported languages
just fmt

# Fix lints for a touched crate
just fix -p codex-tui

# Run scoped tests via nextest
just test -p codex-tui

# Regenerate config schema after ConfigToml/config type changes
just write-config-schema

# Regenerate app-server protocol schema artifacts
just write-app-server-schema
```

Do not run `cargo test` directly for routine work; `AGENTS.md` instructs contributors to use `just test` so local runs match repository defaults.

## Important engineering rules from repository guidance

`AGENTS.md` is a primary source for local engineering rules. Highlights that future agents should preserve:

- Crate names are prefixed with `codex-`; the `core/` directory is the `codex-core` crate.
- Avoid adding new code to `codex-core` when a narrower crate is appropriate; the file explicitly says to resist growing `codex-core`.
- Keep crate APIs small and prefer private modules with explicit public exports.
- Avoid large modules; for high-touch TUI files such as `tui/src/app.rs`, `tui/src/chatwidget.rs`, and `tui/src/bottom_pane/chat_composer.rs`, prefer new focused modules over adding more orchestration code.
- If changing Rust dependencies, update `Cargo.toml`/`Cargo.lock` and run `just bazel-lock-update` so `MODULE.bazel.lock` does not drift.
- If adding compile-time file reads (`include_str!`, `include_bytes!`, `sqlx::migrate!`, etc.), update the crate `BUILD.bazel` data attributes; Cargo passing is not enough.
- For code changes, run `just fmt`, scoped `just fix -p <crate>`, and relevant `just test -p <crate>` checks. Ask before a full `just test` sweep when appropriate.
- Do not add general product/user docs to `docs/`; official product docs live elsewhere. This OpenWiki content stays under `openwiki/`.

## Recent development direction

Recent git history shows active work around:

- Skill selection and metrics: `c10010928` added weighted lexical shadow skill selection and metrics; `2b0b37abb` aligned that experiment with observable host/orchestrator sources in `codex-rs/ext/skills/src/shadow_selection_experiment.rs`.
- Multi-agent spawning: `92938d880` restricted spawned-agent model choices to the active backend across `core/src/tools/handlers/multi_agents_*`, protocol model metadata, and TUI session code.
- TUI model/reasoning settings: `769a5de25` made advanced reasoning selection explicit and touched TUI config persistence, popups, app-server session handling, and thread metadata sync.
- Connector runtime snapshots: `2f7d89b14` extracted connector runtime cache/snapshot management into `codex-rs/connectors/src/connector_runtime/`, moving ownership away from `codex-mcp`.
- App/plugin and MCP integration: recent commits also touched effective plugin changes, app-server request processing, core plugins, rollout ordinals, response item IDs, and guardian review/session alignment.

Use these areas as hints when investigating regressions in current code; do not assume older names like “conversation” are canonical when newer code uses “thread” aliases and deprecation shims.

## Worktree notes at initialization

At initialization, `git status --short` reported `M AGENTS.md`, `?? .github/workflows/openwiki-update.yml`, and `?? openwiki/`. This run read `AGENTS.md` as guidance but did not edit it. The generated documentation lives under `openwiki/`; `openwiki/INSTRUCTIONS.md` is user-authored control metadata and should not be rewritten during routine wiki updates.
