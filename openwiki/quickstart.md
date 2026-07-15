# Codex repository quickstart

Codex CLI is OpenAI's local coding agent. The public `README.md` covers installation and links to hosted product documentation; this wiki is the engineer-facing map of the implementation, its invariants, and the checks expected when changing it.

## Repository shape

The active product implementation is the Rust 2024 workspace under `codex-rs/`. Cargo is the source of truth for its 100+ crates and features; Bazel mirrors the workspace for hermetic, cross-platform builds and release artifacts. Root-level scripts, CI workflows, SDKs, and Node tooling support the Rust product.

| Area | Responsibility | Start here |
| --- | --- | --- |
| CLI and interactive UI | Multitool command, TUI, resume/fork, diagnostics | `codex-rs/cli/src/main.rs`, `codex-rs/tui/src/lib.rs` |
| Agent runtime | Threads, turns, model calls, context, tools, approvals | `codex-rs/core/src/lib.rs`, `codex-rs/core/src/session/` |
| Protocols | Internal Op/Event types and external app-server JSON-RPC | `codex-rs/protocol/`, `codex-rs/app-server-protocol/` |
| External clients | IDE/app control surface and transports | `codex-rs/app-server/` |
| Persistence | Rollout JSONL, thread stores, SQLite projections | `codex-rs/rollout/`, `thread-store/`, `state/` |
| Extensibility | Typed extensions, plugins, MCP, connectors, skills, hooks | `codex-rs/ext/`, `plugin/`, `core-plugins/`, `codex-mcp/` |
| Execution safety | Approval routing, exec policy, sandbox backends | `codex-rs/core/src/tools/`, `execpolicy/`, `sandboxing/` |

## Read next

- [Architecture overview](architecture/overview.md): runtime boundaries and control/data flow.
- [Source map](architecture/source-map.md): task-oriented repository navigation.
- [Core concepts](domain/core-concepts.md): threads, turns, context, tools, permissions, persistence, models, and memory.
- [Integration points](domain/integrations.md): app-server, MCP, typed extensions, plugins, connectors, skills, hooks, and providers.
- [Engineer runbook](workflows/engineer-runbook.md): setup, commands, generators, CI, and operational notes.
- [Testing guidance](workflows/testing-guidance.md): scoped test selection and cross-cutting validation.
- [Interactive CLI conversation flow](../../docs/codex-dev-lesson/openwiki/codex-cli-conversation-flow.md): end-to-end launch, thread, turn, sampling, tool, persistence, and rendering path.
- [Interactive CLI conversation code map](../../docs/codex-dev-lesson/openwiki/codex-cli-conversation-code-map.md): ordered call chains, source locations, tests, identifiers, and critical-function walkthroughs.
- [Interactive CLI conversation type map](../../docs/codex-dev-lesson/openwiki/codex-cli-conversation-type-map.md): ownership scopes, lifecycle APIs, type dossiers, and commonly confused concepts.

## Build and run

The root `justfile` sets `working-directory := "codex-rs"`, so these recipes run from the repository root:

```bash
cd codex-rs && cargo build
just codex "explain this repository"
just exec "summarize the current changes"
just fmt
just clippy -p codex-tui
just test -p codex-tui
just write-config-schema
just write-app-server-schema
just write-hooks-schema
```

Use `just test`, not routine `cargo test`: the recipe uses nextest, an 8 MiB Rust stack, the local profile, and no-fail-fast execution (`justfile`). Root maintenance tooling requires Node 22+ and pnpm 10.33+ (`package.json`); it is not the primary product runtime.

## Engineering invariants

`AGENTS.md` is the primary local contribution brief:

- Prefer a focused crate over adding more to `codex-core`; its size is an acknowledged architectural problem.
- Keep crate APIs small, modules private by default, and new traits documented.
- Do not grow central TUI orchestration files when a focused module is practical.
- Model-visible context must be incremental and bounded; fragments belong under `core/context` and implement the contextual fragment contract.
- Treat app-server APIs, raw response-item events, CLI arguments, configuration loading, and rollout resume compatibility as breaking-change surfaces.
- If dependencies change, refresh `MODULE.bazel.lock` with `just bazel-lock-update`.
- If compile-time source reads are added, declare the files in the crate's `BUILD.bazel`; Cargo success alone is insufficient.
- UI output changes require reviewed `insta` snapshot coverage.
- Generated wiki content belongs under `openwiki/`; do not rewrite `openwiki/INSTRUCTIONS.md` during routine maintenance.

## Current development themes

Recent history is concentrated in cross-layer behavior:

- Weighted lexical skill selection is a **shadow/metrics experiment**, restricted to observable Host/Orchestrator candidates (`c10010928`, `2b0b37abb`). Verify call sites before treating it as active selection.
- Spawned-agent model overrides were constrained to the active backend (`92938d880`), affecting core tools, protocol metadata, and TUI/app-server plumbing.
- Explicit advanced-reasoning selection crossed TUI settings, app-server resume, state extraction, and thread metadata synchronization (`769a5de25`).
- Connector live snapshot ownership moved from `codex-mcp` to `codex-connectors` (`2f7d89b14`).
- Approval hooks can resolve strict automatic-review requests before Guardian or user routing (`2da1b1282`).
- Memory consolidation preserves restrictive parent permission profiles (`54b8f112a`).

The pattern is important: seemingly local model, tool, or UI changes often require protocol, persistence, and integration tests in the same change.

## First steps for a change

1. Check `git status --short` and read `AGENTS.md` plus nearby tests/module docs.
2. Use the [source map](architecture/source-map.md) to identify the narrowest owning crate.
3. Search all compatibility surfaces named above before changing shared types.
4. Add focused tests, run the owning crate's checks, and regenerate affected artifacts.
5. Review the final diff and ensure generators/tests left the worktree as expected.
