# Engineer runbook

This runbook captures practical commands and operational notes for working in this repository. Prefer source verification for any command that affects release, CI, or generated files.

## Setup

From `docs/install.md`:

- Supported operating systems include macOS 12+, Ubuntu 20.04+/Debian 10+, and Windows 11 via WSL2.
- Install Rust, `rustfmt`, `clippy`, `just`, DotSlash, and `cargo-nextest` for local development.
- Build from `codex-rs/` with `cargo build`.
- Launch from source with `cargo run --bin codex -- "prompt"` or the root `just codex` helper.

The root `justfile` sets `working-directory := "codex-rs"`, so most recipes can be invoked from the repository root.

## Common run commands

```bash
# Interactive Codex CLI/TUI
just codex
just codex "explain this codebase to me"

# Non-interactive exec mode
just exec "summarize the changes"

# TUI with exec-server
just tui-with-exec-server

# App-server test client after building CLI
just app-server-test-client

# MCP server
just mcp-server-run

# Bazel-built CLI
just bazel-codex
```

Relevant `codex-rs/cli/src/main.rs` subcommands include `exec`, `review`, `login`, `logout`, `mcp`, `plugin`, `mcp-server`, `app-server`, `remote-control`, `doctor`, `sandbox`, `debug`, `resume`, `archive`, `delete`, `unarchive`, `fork`, `cloud`, and `features`.

## Formatting and linting

```bash
# Format Rust, Bazel/Starlark, Python SDK code, Python scripts, and justfile-supported files
just fmt

# Check formatting only
just fmt-check

# Fix clippy warnings for a touched crate
just fix -p <crate>

# Run clippy without fixing
just clippy -p <crate>
```

Repository guidance in `AGENTS.md` says to run `just fmt` automatically after code changes, then scoped `just fix -p <project>` before finalizing large changes. Do not re-run tests after `fix`/`fmt` unless the change warrants it.

Rust style rules from `AGENTS.md` include inline `format!` args when possible, collapsed `if` statements, method references over redundant closures, exhaustive matches where practical, doc comments for new traits, and avoiding `#[async_trait]`/`#[allow(async_fn_in_trait)]` in favor of RPITIT-style future-returning trait methods.

## Tests

```bash
# Preferred routine test command
just test -p <crate>

# Full suite via nextest; ask before running if expensive
just test

# GitHub script tests from repository root
just test-github-scripts

# Bazel tests
just bazel-test
```

Do not run `cargo test` directly for routine validation; use `just test` so `RUST_MIN_STACK`, `NEXTEST_PROFILE=local`, and nextest defaults match the repository workflow.

See [testing-guidance.md](testing-guidance.md) for choosing scoped tests.

## Generated files and schemas

```bash
# Config schema after ConfigToml/nested config changes
just write-config-schema

# App-server protocol schema fixtures
just write-app-server-schema

# Hook schema fixtures
just write-hooks-schema

# Bazel lockfile after Rust dependency changes
just bazel-lock-update

# Check Bazel lock drift
just bazel-lock-check
```

Important cautions:

- If Rust dependencies change, include `Cargo.toml`/`Cargo.lock` and `MODULE.bazel.lock` updates together.
- Bazel does not automatically expose source-tree files for compile-time reads. If adding `include_str!`, `include_bytes!`, `sqlx::migrate!`, or similar, update the crate `BUILD.bazel` data attributes.
- Cargo remains the source of truth for Rust crates/features, while Bazel supplies hermetic builds/toolchains/artifacts (`codex-rs/docs/bazel.md`).

## Bazel and BuildBuddy

`codex-rs/docs/bazel.md` explains local and CI Bazel behavior:

- `MODULE.bazel` defines dependencies/toolchains.
- `rules_rs` imports crates from `codex-rs/Cargo.toml` and `Cargo.lock`.
- `defs.bzl` provides `codex_rust_crate` wrappers.
- Each crate usually has a `BUILD.bazel` target.
- Root recipes include `just bazel-test`, `just bazel-clippy`, `just bazel-codex`, and `just build-for-release`.

BuildBuddy credentials must stay outside committed files (for example `~/.bazelrc` or ignored `user.bazelrc`). Do not document or copy credential values.

## Logging and diagnostics

From `docs/install.md`:

- Codex honors `RUST_LOG`.
- TUI diagnostics are bounded local stores by default.
- Set `log_dir` to enable plaintext TUI logs:

```bash
codex -c log_dir=./.codex-log
tail -F ./.codex-log/codex-tui.log
```

Non-interactive `codex exec` defaults to `RUST_LOG=error` and prints messages inline.

Other diagnostics:

- `codex doctor` is defined in `codex-rs/cli/src/doctor.rs` and exposed as a CLI subcommand.
- App-server logging setup is in `codex-rs/app-server/src/lib.rs` and tracing helpers under `app_server_tracing.rs`.

## Config operations

- Basic/advanced/reference config docs are external links from `docs/config.md`.
- The in-repo `codex-rs/config.md` is only a moved-config notice pointing to canonical docs.
- App-server config manager code lives in `codex-rs/app-server/src/config_manager*.rs`.
- Core config is under `codex-rs/core/src/config/`.

When editing config behavior, inspect config loading tests and app-server strict-config tests, then regenerate schema if needed.

## Safe workflow for future agents

1. Check `git status --short` before editing. Preserve user changes.
2. Read `AGENTS.md` and any nearby module docs/tests before modifying source.
3. Choose the narrowest crate/module that owns the behavior; avoid growing `codex-core` by default.
4. Add/update focused tests near the behavior.
5. Run `just fmt`, scoped `just fix -p <crate>`, and scoped `just test -p <crate>`.
6. If schemas/locks/generated files are involved, run the matching generation command.
7. For app-server/protocol/persistence/tool changes, consider compatibility and replay/resume behavior explicitly.

## OpenWiki maintenance notes

Generated documentation belongs under `openwiki/`. Do not edit `openwiki/INSTRUCTIONS.md` unless explicitly asked; it is user-authored control metadata. Keep future updates concise and grounded in source evidence.
