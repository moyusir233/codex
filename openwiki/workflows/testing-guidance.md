# Testing guidance

Use this page to choose focused validation for Codex changes. The repository is large; prefer scoped tests first, then broader checks only when the touched area is shared or high-risk.

## Default commands

From `AGENTS.md` and the root `justfile`:

```bash
# Preferred routine test command
just test -p <crate>

# Full nextest suite; ask before running if expensive
just test

# Format after code changes
just fmt

# Clippy fix for a touched crate
just fix -p <crate>

# GitHub script tests
just test-github-scripts

# Bazel tests
just bazel-test
```

Do not run `cargo test` directly for routine work. The `just test` recipe sets `RUST_MIN_STACK=8388608`, `NEXTEST_PROFILE=local`, and uses `cargo nextest run --no-fail-fast`.

## Test organization rules

Repository guidance:

- Prefer integration tests for agent logic changes.
- Core integration tests live under `codex-rs/core/tests/suite` and use `test_codex` helpers.
- Unit tests, when needed, should usually live in dedicated sibling `*_tests.rs` files with explicit `#[path = "..."]` module attributes.
- Prefer comparing whole objects over checking fields one by one.
- Do not add tests for values that are statically defined.
- Do not add negative tests for logic that was removed.
- Avoid test-only helpers in main implementation code.

## Choose tests by area

| Changed area | Start with |
| --- | --- |
| TUI UI/rendering/settings | `just test -p codex-tui`; targeted tests under `codex-rs/tui/src/**`; update `snapshots/` when intentional |
| CLI command parsing/doctor/plugin/MCP commands | `just test -p codex-cli`; inspect module-specific tests or add focused unit tests |
| Core agent/session/tool behavior | `just test -p codex-core`; relevant files under `codex-rs/core/tests/suite` |
| App-server API | `just test -p codex-app-server`; suite under `codex-rs/app-server/tests/suite`, especially `suite/v2` |
| Protocol/app-server schema | `just test -p codex-protocol` or `codex-app-server-protocol`; run `just write-app-server-schema` for generated schema fixtures |
| Rollout/thread persistence | `just test -p codex-rollout`, `just test -p codex-thread-store`, plus core/app-server resume/list tests if behavior crosses layers |
| Multi-agent tools | Core handler tests under `core/src/tools/handlers/multi_agents*`, core suite `subagent_notifications`, `spawn_agent_description`, `multi_agent_mode` |
| Skills | `just test -p codex-skills-extension` or the relevant ext skills crate; `codex-rs/ext/skills/tests/*`; selector unit tests |
| Plugins | `just test -p codex-core-plugins`; manager/marketplace/startup sync tests |
| Connectors/MCP runtime | `just test -p codex-connectors`, `codex-mcp`, and core MCP suite tests depending on boundary touched |
| Hooks | `just test -p codex-hooks`; core suite `hooks.rs`, `hooks_mcp.rs` if runtime behavior changes |
| Memories | `just test -p codex-memories-write`, `codex-memories-read`; startup/phase tests; preserve sandbox assumptions |
| Bazel metadata/build files | `just bazel-lock-check`, `just bazel-test` or targeted Bazel command |

## Snapshot tests

The TUI uses snapshot tests extensively, visible under `codex-rs/tui/src/**/snapshots/`. Recent commits added/updated snapshots for reasoning selection popups, skill toggle widths, composer completion behavior, and diff rendering.

When a UI change intentionally alters output:

1. Run the relevant TUI test target.
2. Inspect the snapshot diff carefully.
3. Update snapshots only when the change is intended and source behavior supports it.
4. Avoid broad UI churn from style-only refactors.

## High-risk integration tests

Run broader or cross-crate coverage when touching these areas:

- `codex-rs/protocol` or `codex-rs/app-server-protocol`: app-server clients, TUI, core event mapping, schema artifacts.
- `codex-rs/core/src/session`, `thread_manager.rs`, `rollout`, `thread-store`: resume/fork/history/replay and app-server thread APIs.
- `codex-rs/core/src/tools`: core suite tool tests, approval/sandbox tests, and any affected TUI/app-server surface.
- Permissions/sandbox/guardian: approvals, exec policy, guardian review, network approval, platform sandbox tests.
- Model metadata/reasoning: TUI popups/settings, app-server model list/resume, thread metadata sync, unsupported image behavior.
- Plugins/connectors/MCP: app-server plugin list, core plugin manager tests, MCP auth/refresh/tool exposure tests.

## Recent-history test hints

The recent git log is a good guide to current regression-sensitive areas:

- Skill shadow selection changes added `codex-rs/ext/skills/tests/implicit_invocation.rs` and selector tests.
- Multi-agent model restriction updated `multi_agents_spec_tests.rs`, `multi_agents_tests.rs`, protocol model metadata, and TUI popup/settings tests.
- Advanced reasoning selection added TUI app tests and popup snapshots, plus app-server thread resume and thread metadata sync coverage.
- Connector runtime extraction moved tests from `codex-mcp` into `codex-rs/connectors/src/connector_runtime/tests.rs`.
- Rollout ordinals touched app-server protocol projection, CLI doctor inventory, core suite SQLite state, and rollout tests.

## When to run full suite

Use scoped tests for most changes. Consider full `just test` when changes affect shared crates such as `codex-core`, `codex-protocol`, `codex-app-server-protocol`, config loading, persistence, or cross-cutting model/tool behavior. `AGENTS.md` says to ask before running the complete test suite after common/core/protocol changes.
