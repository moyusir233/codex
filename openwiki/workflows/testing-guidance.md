# Testing guidance

Prefer the narrowest test target that owns the behavior, then expand for shared protocols, persistence, model-visible tools, safety, or cross-client changes.

## Defaults

```bash
just test -p <crate>       # nextest, local profile, no-fail-fast
just clippy -p <crate>
just fmt
```

The default nextest profile retries once and terminates tests after two consecutive 30-second slow periods. App-server integration tests are serialized in the default/CI profile but allow up to four local subprocesses; Windows-heavy and apply-patch groups have tighter concurrency (`codex-rs/.config/nextest.toml`).

## Test by area

| Area | Initial validation | Expand when needed |
| --- | --- | --- |
| TUI | `just test -p codex-tui` | Review/accept intentional `insta` snapshots |
| CLI | `just test -p codex-cli` | Command module and end-to-end parsing/invocation tests |
| Core session/tools | `just test -p codex-core` | `core/tests/suite`, protocol, app-server/TUI consumers |
| App-server | `just test -p codex-app-server` | v2 suite and protocol schema tests |
| Protocol | `codex-protocol`, `codex-app-server-protocol` | event mapping, clients, generated schemas |
| Persistence | `codex-rollout`, `codex-thread-store`, `codex-state` | resume/fork/list/history across core/app-server/TUI |
| Typed extensions | `codex-extension-api` and concrete extension crate | app-server extension composition |
| Skills | `codex-core-skills` or `codex-skills-extension` | implicit invocation and selector tests |
| Plugins | `codex-plugin`, `codex-core-plugins` | app-server effective state and policy/startup sync |
| MCP | `codex-mcp`, `codex-rmcp-client` | core MCP suite, auth/elicitation/exposure |
| Connectors | `codex-connectors` | connector runtime identity/persistence plus MCP consumers |
| Hooks | `codex-hooks` | core `hooks`/`hooks_mcp`, approval ordering, regenerate schema |
| Models | provider/info/manager crate | core client, app-server list, TUI settings, persistence |
| Safety | `codex-execpolicy`, `codex-sandboxing`, platform crate | core approval/sandbox/Guardian integration |
| Memories | read/write crates | startup, phase, DB lease, workspace roots/sandbox tests |

Use exact Cargo package names from `codex-rs/Cargo.toml` when a shorthand above is ambiguous.

## Test organization

`AGENTS.md` requires agent-logic changes to have integration coverage. Core integration tests live in `core/tests/suite` and use `test_codex`. New unit-test modules should generally be sibling `*_tests.rs` files wired with `#[path = "..."]`. Prefer whole-object equality, avoid test-only implementation APIs, and do not test static constants merely for existing.

## Snapshot and generated-file checks

User-visible TUI changes require snapshot coverage. Run `just test -p codex-tui`, inspect `*.snap.new`, and accept only intentional changes (for example with `cargo insta accept -p codex-tui`). Do not hide broad visual churn in a functional change.

Schema and lockfile changes must be regenerated with the matching runbook command. CI's clean-worktree checks catch forgotten generated output, but local review should catch it first.

## Cross-cutting risk matrix

Broaden tests for:

- Internal/external protocol changes: all consumers, schema fixtures, raw item mapping.
- Thread/session/persistence changes: create, resume, fork, list, archive, replay, projection.
- Tool/context changes: model-visible spec, execution handler, truncation, approvals, client events.
- Permission/sandbox changes: hooks, Guardian/user routing, exec/network amendments, platform backends.
- Model/reasoning changes: provider catalog, app-server, TUI, metadata sync, unsupported modalities.
- Plugin/connector/MCP changes: startup/effective state, identity/auth, tool exposure, app-server integration.
- Multi-agent changes: parent/child metadata, model constraints, notifications, app/TUI controls.

## CI expectations

Blocking CI aggregates Bazel, repository policy, supply-chain/spelling/blob checks, fast Rust checks, and SDK tests. The fast Rust workflow is not the full workspace test suite. Full Rust target/build/nextest coverage runs in full/postmerge or opt-in workflows, while Bazel remains a blocking family.

Repository checks also cover workspace-manifest inheritance, TUI/core dependency boundaries, Cargo/Bazel Clippy parity, packaging/installers, Prettier, and clean worktrees. SDK CI separately validates Python and TypeScript surfaces.

## When to run broad checks

Consider full `just test` for changes to core/shared protocols, config loading, persistence, or cross-cutting model/tool behavior; follow `AGENTS.md` guidance about asking before the expensive workspace sweep. Use targeted Bazel tests for build metadata or platform/resource issues before `just bazel-test`. Run full-format/repository checks when touching root scripts, workflows, SDKs, or generated metadata.

Recent commits are useful test maps: skill selection added implicit-invocation/selector tests; advanced reasoning touched app-server resume, state/thread metadata, TUI tests, and snapshots; connector runtime extraction moved its tests into `connectors`; rollout ordinals crossed protocol projection, doctor, core SQLite, and rollout tests.
