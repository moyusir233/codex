# Engineer runbook

## Prerequisites

`docs/install.md` describes Rust, `rustfmt`, Clippy, `just`, DotSlash, and `cargo-nextest` for local work. Build from `codex-rs/` with Cargo or use root `just` recipes. Root maintenance/SDK tooling requires Node 22+ and pnpm 10.33+ (`package.json`).

## Run and diagnose

```bash
just codex "prompt"                 # interactive TUI
just exec "prompt"                  # non-interactive
just tui-with-exec-server            # TUI plus exec-server
just app-server-test-client          # build CLI and launch test client
just mcp-server-run                  # experimental MCP server
just bazel-codex                     # Bazel-built CLI
just log                             # tail structured logs from state SQLite
```

The CLI also exposes app-server, daemon/remote-control, doctor, debug, sandbox, plugin/MCP management, and session resume/fork/archive/delete flows (`cli/src/main.rs`). App-server directly supports `--listen` with stdio, Unix, WebSocket, or off transports and `--strict-config` (`app-server/src/main.rs`). Verify live `--help` before relying on experimental operational flags.

For plaintext TUI logs, `docs/install.md` documents `log_dir` and `RUST_LOG`; do not assume plaintext logging is enabled by default.

## Format, lint, and test

```bash
just fmt-check
just fmt
just clippy -p <crate>
just fix -p <crate>
just test -p <crate>
```

Routine tests use nextest; do not substitute `cargo test`. `AGENTS.md` requires formatting after code changes, scoped tests, and scoped `fix` before finalizing large changes. Its “do not rerun tests after fix/fmt” note assumes those commands do not introduce semantic edits; always inspect the final diff.

A practical order is: implement and add tests; run focused tests; run scoped fix and final format; inspect the resulting diff and generated artifacts. Ask before an expensive full `just test` when repository guidance requires it.

## Generated artifacts

| Change | Command |
| --- | --- |
| `ConfigToml` or nested config | `just write-config-schema` |
| App-server protocol/schema | `just write-app-server-schema` |
| Hook types/schema | `just write-hooks-schema` |
| Rust dependency graph | `just bazel-lock-update` then `just bazel-lock-check` |

If code uses `include_str!`, `include_bytes!`, `sqlx::migrate!`, or another compile-time source read, add the file/directory to the crate's Bazel compile/build/test data. Cargo does not model Bazel sandbox availability.

## Cargo, Bazel, and CI

Cargo remains the crate/feature source of truth. Bazel supplies hermetic toolchains, cross-platform tests/builds, and release artifacts (`codex-rs/docs/bazel.md`). That document labels the setup experimental “as of 6/1/2026”; treat the date as source qualification rather than a timeless maturity claim.

Useful checks:

```bash
just bazel-lock-check
just bazel-test
just bazel-clippy
just argument-comment-lint
just test-github-scripts
pnpm run format
```

`just bazel-test` is workspace-wide and potentially expensive; prefer a targeted Bazel label when diagnosing one crate. BuildBuddy credentials belong only in local ignored configuration—never in source or documentation.

`.github/workflows/blocking-ci.yml` is the single merge-blocking entrypoint. Its `CI required` gate aggregates Bazel, blob-size policy, cargo-deny, codespell, repo checks, fast Rust CI, and SDK workflows. Fast `rust-ci.yml` is path-sensitive and emphasizes formatting, benchmark smoke, cargo-shear, and argument-comment lint; full cross-platform Rust nextest coverage runs in full/postmerge workflows. CI jobs commonly check for a clean worktree after generators/tests.

## Configuration and secrets

Core config lives under `core/src/config/`; app-server has config manager/service code; `docs/config.md` points to canonical product docs and records managed-hook behavior. Test strict config and loading layers when adding fields. Do not read `.env` or document credentials, tokens, keys, or local secret stores.

## Safe change workflow

1. Inspect `git status --short`; preserve user changes.
2. Read `AGENTS.md`, the owning crate manifest/module docs, and nearby tests.
3. Choose the narrowest owner; resist adding unrelated behavior to core or central TUI files.
4. Identify compatibility surfaces: protocols, config, tool schemas/context, persistence, clients.
5. Add focused tests and run the owning crate first.
6. Run required generators, scoped lint/fix, and final formatting.
7. Inspect snapshots, schemas, lockfiles, and the full final diff.
8. Escalate to cross-crate/full/Bazel checks based on [testing guidance](testing-guidance.md).

## Common failure clues

- Cargo passes but Bazel cannot find a file: missing `BUILD.bazel` data declaration.
- Resume/list differs from live behavior: persisted item, state extraction, and thread metadata projection are out of sync.
- Model setting appears in one UI only: inspect TUI, app-server model API, provider catalog, and persisted metadata.
- MCP tool behavior is stale after account/workspace changes: inspect connector runtime identity and snapshots, not only MCP transport code.
- Approval behavior differs by reviewer: inspect permission hooks first, then Guardian/user routing, exec policy, and sandbox resolution.
