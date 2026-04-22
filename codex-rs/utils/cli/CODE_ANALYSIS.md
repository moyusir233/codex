# codex-utils-cli crate analysis

## Scope and role

The `codex-utils-cli` crate is a small shared utility crate that centralizes reusable command-line parsing types for multiple binaries in the repository. Its public surface is intentionally narrow: it exports a few `clap`-derived types, two enum adapters that bridge CLI flags into protocol/config enums, and one display helper for redacted environment variables.

The crate exists to avoid duplicating the same flag definitions and conversion logic across:

- the interactive TUI entry point
- the non-interactive exec entry point
- the top-level multitool CLI and several utility subcommands
- service-style binaries that still need `-c key=value` config override support

Primary entrypoint: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/lib.rs#L1-L11)

## Module map

- [approval_mode_cli_arg.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/approval_mode_cli_arg.rs#L1-L38)
  - Defines the `--ask-for-approval` value enum used by interactive CLI flows.
  - Converts user-facing CLI values into `codex_protocol::protocol::AskForApproval`.
- [sandbox_mode_cli_arg.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/sandbox_mode_cli_arg.rs#L1-L28)
  - Defines the `--sandbox` / `-s` value enum.
  - Converts user-facing CLI values into `codex_protocol::config_types::SandboxMode`.
- [config_override.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/config_override.rs#L1-L200)
  - Defines `CliConfigOverrides`, the shared `-c/--config key=value` parser.
  - Parses dotted-path overrides into `toml::Value` pairs and can apply them into a TOML tree.
- [shared_options.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/shared_options.rs#L1-L173)
  - Defines the common top-level/shared flags used across interactive and exec-style frontends.
  - Contains merge logic for propagating root options into subcommands and overriding them back from subcommands.
- [format_env_display.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/format_env_display.rs#L1-L24)
  - Formats environment configuration for safe human-readable display using masked values.

## Responsibilities

- Define canonical CLI shapes once and reuse them via `#[clap(flatten)]`.
- Keep user-facing flag values stable and readable through `ValueEnum` kebab-case names.
- Translate CLI-level concepts into protocol/config-layer enums without forcing downstream crates to duplicate conversion code.
- Parse config overrides in a way that is ergonomic for CLI users:
  - typed TOML when possible
  - string fallback when not
  - dotted paths for nested config
  - one compatibility alias for `use_legacy_landlock`
- Provide deterministic, secret-safe formatting of env maps for tables and history views.

## Public API

The crate exports five public items from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/lib.rs#L7-L11):

- `ApprovalModeCliArg`
- `CliConfigOverrides`
- `format_env_display`
- `SandboxModeCliArg`
- `SharedCliOptions`

### `ApprovalModeCliArg`

Defined in [approval_mode_cli_arg.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/approval_mode_cli_arg.rs#L7-L27).

User-facing values:

- `untrusted`
- `on-failure`
- `on-request`
- `never`

Conversion logic: [From<ApprovalModeCliArg> for AskForApproval](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/approval_mode_cli_arg.rs#L29-L38)

Notable detail:

- `Untrusted` maps to `AskForApproval::UnlessTrusted`, which means the CLI terminology is slightly more user-centric than the protocol enum naming.
- `OnFailure` is marked deprecated in doc comments but remains supported for compatibility.

### `SandboxModeCliArg`

Defined in [sandbox_mode_cli_arg.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/sandbox_mode_cli_arg.rs#L12-L18).

User-facing values:

- `read-only`
- `workspace-write`
- `danger-full-access`

Conversion logic: [From<SandboxModeCliArg> for SandboxMode](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/sandbox_mode_cli_arg.rs#L20-L28)

Design note:

- The CLI enum deliberately strips any richer associated data from the lower layer so the command line remains a simple flag interface.

### `CliConfigOverrides`

Defined in [config_override.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/config_override.rs#L18-L37).

Key methods:

- [`parse_overrides`](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/config_override.rs#L39-L77)
  - Converts raw `Vec<String>` inputs into `Vec<(String, toml::Value)>`.
- [`apply_on_value`](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/config_override.rs#L79-L88)
  - Applies parsed overrides into a mutable TOML tree.

Important behaviors:

- Splits only on the first `=` so values may themselves contain `=`.
- Trims whitespace around both key and value.
- Rejects missing `=` and empty keys.
- Parses the value as TOML using a sentinel assignment wrapper in [`parse_toml_value`](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/config_override.rs#L143-L150).
- Falls back to a string literal if TOML parsing fails.
- Canonicalizes `use_legacy_landlock` into `features.use_legacy_landlock` in [`canonicalize_override_key`](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/config_override.rs#L91-L97).

### `SharedCliOptions`

Defined in [shared_options.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/shared_options.rs#L7-L62).

Shared fields:

- image attachments
- model selection
- OSS toggle and provider selection
- config profile
- sandbox mode
- convenience execution modes (`--full-auto`, dangerous bypass)
- working directory
- additional writable directories

Key merge methods:

- [`inherit_exec_root_options`](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/shared_options.rs#L64-L127)
  - Pulls root-level options into a nested/subcommand option set when the subcommand did not explicitly choose its own values.
- [`apply_subcommand_overrides`](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/shared_options.rs#L129-L173)
  - Applies a parsed subcommand option block on top of an existing shared option set.

Important semantics:

- Images from the root are prepended to subcommand images.
- `add_dir` values from the root are prepended when inheriting, but appended when applying subcommand overrides.
- Sandbox-related options are treated as a coordinated group, preventing partial inheritance when the subcommand explicitly selected any sandbox mode.
- Boolean `oss` is sticky in the positive direction only.

### `format_env_display`

Defined in [format_env_display.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/format_env_display.rs#L3-L24).

Behavior:

- Accepts an optional `HashMap<String, String>` and a slice of env var names.
- Sorts map keys lexicographically for deterministic output.
- Masks all values as `*****`.
- Returns `-` when both sources are empty.

This is a pure formatting helper for safe display, not for serialization or execution.

## Concrete flow through the system

### 1. Parsing and reuse

Downstream CLIs flatten these types directly into their own clap parsers:

- [tui/src/cli.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/cli.rs#L53-L74)
- [exec/src/cli.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/cli.rs#L19-L72)
- [cli/src/main.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L84-L99)
- [tui/src/main.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/main.rs#L39-L46)

The crate therefore operates as a reusable schema layer for CLI arguments.

### 2. Conversion into runtime config/protocol values

After parsing, downstream code converts the shared enums into runtime-layer enums:

- exec resolves convenience flags into sandbox mode in [exec/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L270-L276)
- TUI resolves sandbox + approval behavior in [tui/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/lib.rs#L690-L695)

This keeps clap-specific parsing concerns out of lower-level config and protocol crates.

### 3. Config override handling

Typical downstream pattern:

1. capture raw `-c key=value` strings with `CliConfigOverrides`
2. call `parse_overrides()`
3. pass the resulting key/value pairs into config loading

Representative call sites:

- [exec/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L278-L286)
- [tui/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/lib.rs#L706-L717)
- [cli/src/main.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1333-L1383)
- [mcp-server/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/mcp-server/src/lib.rs#L60-L76) (via grep-discovered usage)

Notable cross-command precedence detail:

- the top-level CLI explicitly prepends root-level overrides into subcommand overrides so later subcommand-local flags win: [prepend_config_flags](file:///Users/bytedance/project/codex/codex-rs/cli/src/main.rs#L1374-L1383)

### 4. Safe environment display

The environment display helper is used when rendering MCP server configs and history views:

- [cli/src/mcp_cmd.rs](file:///Users/bytedance/project/codex/codex-rs/cli/src/mcp_cmd.rs#L559-L614)
- [tui/src/history_cell.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/history_cell.rs#L1964-L1964)

The helper ensures these views reveal names but never values.

## Dependency analysis

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/cli/Cargo.toml#L1-L17), the crate keeps dependencies minimal:

- `clap`
  - Used for `Parser`, `Args`, `ValueEnum`, and CLI metadata.
  - This is the dominant dependency because the crate’s main job is to define reusable CLI argument structures.
- `codex-protocol`
  - Supplies the target enums that CLI enums convert into.
  - Couples this crate to protocol/config enum names but avoids duplicated mapping logic in consumers.
- `serde`
  - Used only for TOML parse error customization via `serde::de::Error`.
- `toml`
  - Represents parsed override values and target trees for config mutation.

Dev dependency:

- `pretty_assertions`
  - Used only in sandbox-mode tests for clearer equality failures.

Architecturally, this is a leaf-like helper crate with outward-facing reuse but very limited inward dependency weight.

## Design choices

### Small reusable utility crate

The crate is intentionally small and cohesive. It does not load config, execute commands, or know anything about application runtime beyond enum conversions. That separation is visible in the fact that downstream crates parse with this crate but perform all actual config loading themselves.

### Keep CLI semantics user-facing

Enums use `clap::ValueEnum` and kebab-case names so command-line UX is stable and human-readable. The protocol layer is allowed to use different names because the conversion boundary absorbs that mismatch.

### Prefer ergonomic config overrides

`parse_overrides()` chooses a pragmatic parsing strategy:

- treat valid TOML as typed values
- otherwise preserve the input as a string

That makes flags like `-c model=o3` convenient while still supporting arrays, booleans, and inline tables.

### Make precedence explicit

`SharedCliOptions` contains dedicated merge methods rather than relying on ad hoc field-by-field merging in each consumer. This is important because the precedence rules for sandbox-related flags are subtle and coordinated.

### Deterministic, redacted display

`format_env_display()` sorts keys and always masks values. That avoids test flakiness, reduces noisy diffs, and prevents accidental secret disclosure in UI tables or logs.

## Testing status

Local test run:

- `cargo test -p codex-utils-cli`
- Result: 11 tests passed

Covered tests:

- `config_override.rs`
  - scalar parsing
  - boolean parsing
  - array parsing
  - inline table parsing
  - failure on unquoted strings
  - compatibility alias canonicalization
- `sandbox_mode_cli_arg.rs`
  - enum mapping into protocol config
- `format_env_display.rs`
  - empty formatting
  - sorted output
  - variable-only formatting
  - combined formatting

Observations:

- There are no unit tests for `ApprovalModeCliArg`.
- There are no unit tests for `SharedCliOptions` merge behavior.
- `CliConfigOverrides::apply_on_value()` has no direct tests even though it contains non-trivial tree mutation logic.
- The crate has no integration tests validating clap parsing end-to-end.

Given the semantics encoded in `inherit_exec_root_options()` and `apply_subcommand_overrides()`, missing tests there are the most important gap.

## Strengths

- High reuse across many binaries with very little code.
- Clear separation between CLI parsing and runtime/config loading.
- Good ergonomics for `-c` override parsing.
- Secret-safe display helper with deterministic formatting.
- Minimal dependency surface.

## Risks and edge cases

### Untested merge semantics

The most behavior-dense code is in [shared_options.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/shared_options.rs#L64-L173), but it currently has no tests. Regressions here would affect multiple commands at once.

### Silent string fallback for malformed TOML

In [parse_overrides](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/config_override.rs#L62-L72), invalid TOML falls back to a string instead of erroring. This is ergonomic, but it can also hide typos:

- intended boolean: `-c feature=true`
- typo case: `-c feature=ture`
- result: string `"ture"` rather than a hard failure

That may be desirable, but it is worth calling out explicitly because it trades validation strictness for convenience.

### Dotted keys are always structural

`apply_single_override()` splits on dots unconditionally: [config_override.rs](file:///Users/bytedance/project/codex/codex-rs/utils/cli/src/config_override.rs#L101-L140)

That means literal key names containing dots cannot be targeted unless the surrounding config model already treats dots as hierarchy. This is probably acceptable for this codebase, but it is a deliberate constraint.

### Destructive overwrite on non-table intermediates

If an intermediate path segment already points to a non-table value, `apply_single_override()` replaces it with a table. That behavior is simple and deterministic, but it can silently discard previous scalar content at an ancestor path.

## Open questions

1. Should `SharedCliOptions` have direct unit tests covering inheritance and override precedence, especially for the sandbox flag group?
2. Should `ApprovalModeCliArg` get a symmetry test like `SandboxModeCliArg` already has?
3. Should `CliConfigOverrides::apply_on_value()` have dedicated tests for:
   - nested object creation
   - overwrite of existing scalar nodes
   - values containing `=`
   - conflicting ancestor/descendant paths
4. Is the current invalid-TOML-to-string fallback the desired long-term behavior, or should some categories of malformed values produce an error?
5. Is the `use_legacy_landlock` alias the only compatibility alias this layer should own, or should aliasing live closer to the config loader?
6. Should `format_env_display()` deduplicate keys that appear in both the explicit env map and `env_vars`, or is duplicate display acceptable and informative?

## Summary

`codex-utils-cli` is a focused shared crate that standardizes CLI argument parsing across the Codex Rust workspace. Its most important contribution is not complexity but consistency: one source of truth for shared flags, one parser for `-c` overrides, one pair of enum adapters, and one secret-safe display helper. The implementation is straightforward and pragmatic. The main improvement area is test coverage around merge precedence and override application, because those behaviors are cross-cutting and easy to regress.
