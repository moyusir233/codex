# codex-utils-sandbox-summary

## Overview

`codex-utils-sandbox-summary` is a very small presentation-focused utility crate. It turns effective runtime configuration into human-readable summary strings, with a particular focus on sandbox policy formatting.

The crate contains two internal modules and re-exports two public functions from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/lib.rs#L1-L5):

- [create_config_summary_entries](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L6-L39) builds ordered `(label, value)` entries for display.
- [summarize_sandbox_policy](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L4-L51) converts a `SandboxPolicy` into a compact status string.

This crate does not own policy semantics, validation, or config resolution. Those responsibilities live in upstream crates such as `codex-core` and `codex-protocol`. This crate only formats already-resolved state for display.

## Responsibilities

- Format the effective working directory, model, provider, approval mode, and sandbox policy into UI-friendly entries in [config_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L6-L39).
- Format `SandboxPolicy` variants into a compact single-line summary in [sandbox_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L4-L51).
- Hide protocol-level representation differences, especially the fact that `ReadOnly` uses a `bool` `network_access` while `ExternalSandbox` uses the `NetworkAccess` enum from [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L1003-L1059).
- Preserve a stable display order by returning `Vec<(&'static str, String)>` rather than a map in [config_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L7-L20).

## Module Breakdown

### `src/lib.rs`

- Declares the two modules and re-exports the public surface in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/lib.rs#L1-L5).
- Keeps the crate intentionally tiny: consumers only need the two formatting helpers.

### `src/config_summary.rs`

- Accepts a resolved `codex_core::config::Config` plus a concrete model string in [create_config_summary_entries](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L7-L39).
- Pulls the following fields directly from `Config`:
  - `cwd`
  - `model_provider_id`
  - `permissions.approval_policy`
  - `permissions.sandbox_policy`
  - `model_provider.wire_api`
  - `model_reasoning_effort`
  - `model_reasoning_summary`
- Adds reasoning-related entries only when the provider wire API is `WireApi::Responses`, matching the provider definition in [model-provider-info/lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L44-L60).
- Delegates sandbox string rendering to [summarize_sandbox_policy](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L4-L51) instead of duplicating variant formatting logic.

### `src/sandbox_summary.rs`

- Matches all current `SandboxPolicy` variants from [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L998-L1060).
- Returns normalized strings:
  - `danger-full-access`
  - `read-only`
  - `external-sandbox`
  - `workspace-write [...]`
- Appends ` (network access enabled)` when the selected policy allows outbound networking.
- For `WorkspaceWrite`, constructs a write-scope list starting with `workdir`, then conditionally includes `/tmp` and `$TMPDIR`, then appends any extra writable roots in [sandbox_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L21-L49).

## Public API

### `create_config_summary_entries(config: &Config, model: &str) -> Vec<(&'static str, String)>`

Defined in [config_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L7-L39).

Behavior:

- Produces an ordered summary intended for UI or CLI status rendering.
- Always includes:
  - `workdir`
  - `model`
  - `provider`
  - `approval`
  - `sandbox`
- Conditionally includes:
  - `reasoning effort`
  - `reasoning summaries`

Representative implementation:

```rust
pub fn create_config_summary_entries(config: &Config, model: &str) -> Vec<(&'static str, String)> {
    let mut entries = vec![
        ("workdir", config.cwd.display().to_string()),
        ("model", model.to_string()),
        ("provider", config.model_provider_id.clone()),
        ("approval", config.permissions.approval_policy.value().to_string()),
        ("sandbox", summarize_sandbox_policy(config.permissions.sandbox_policy.get())),
    ];
    if config.model_provider.wire_api == WireApi::Responses {
        // reasoning entries appended here
    }
    entries
}
```

Notable design choices:

- The caller provides `model` separately instead of reading `config.model`; this lets the summary reflect the effective live model even if the config field is unset or overridden elsewhere.
- Labels are `'static` string keys, which keeps allocation low and lets downstream code search by well-known labels.

### `summarize_sandbox_policy(sandbox_policy: &SandboxPolicy) -> String`

Defined in [sandbox_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L4-L51).

Behavior by variant:

- `DangerFullAccess` → `"danger-full-access"`
- `ReadOnly { network_access: false, .. }` → `"read-only"`
- `ReadOnly { network_access: true, .. }` → `"read-only (network access enabled)"`
- `ExternalSandbox { network_access: Restricted }` → `"external-sandbox"`
- `ExternalSandbox { network_access: Enabled }` → `"external-sandbox (network access enabled)"`
- `WorkspaceWrite { ... }` → `"workspace-write [workdir, ...]"`, with optional network suffix

Representative implementation:

```rust
match sandbox_policy {
    SandboxPolicy::DangerFullAccess => "danger-full-access".to_string(),
    SandboxPolicy::ReadOnly { network_access, .. } => {
        let mut summary = "read-only".to_string();
        if *network_access {
            summary.push_str(" (network access enabled)");
        }
        summary
    }
    SandboxPolicy::WorkspaceWrite { writable_roots, .. } => {
        // build "workspace-write [workdir, ...]"
    }
    // ...
}
```

## End-to-End Flow

The crate’s main flow is straightforward:

1. A caller passes a resolved `Config` and effective model string into [create_config_summary_entries](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L7-L39).
2. The function extracts already-computed state from `Config`, whose resolved permissions are assembled by `codex-core` in [core/config/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/config/mod.rs#L2193-L2247).
3. Sandbox formatting is delegated to [summarize_sandbox_policy](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L4-L51).
4. If the provider speaks the Responses API, reasoning-related fields are appended.
5. The caller receives a display-ordered vector of entries.

The sandbox-specific flow inside [summarize_sandbox_policy](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L4-L51) is:

1. Match the `SandboxPolicy` variant.
2. Emit a base policy name.
3. For writable policies, synthesize the writable-root summary.
4. Append a network suffix only when networking is enabled.

## Dependencies and Their Roles

The manifest is intentionally minimal in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/Cargo.toml#L1-L17).

### Runtime dependencies

- `codex-core`
  - supplies `Config`, including resolved permissions and provider info used in [config_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L1-L39).
- `codex-model-provider-info`
  - supplies `WireApi`, used to gate reasoning-related entries in [config_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L21-L35).
- `codex-protocol`
  - supplies `SandboxPolicy` and `NetworkAccess`, the core types formatted in [sandbox_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L1-L51).

### Dev dependencies

- `codex-utils-absolute-path`
  - used by tests to build `AbsolutePathBuf` values for writable roots in [sandbox_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L53-L103).
- `pretty_assertions`
  - improves assertion diffs in the unit tests.

## Consumers and Integration Points

The in-repo usage pattern is mixed:

- `summarize_sandbox_policy` is reused by the TUI status card in [card.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/status/card.rs#L247-L274).
- `exec` still carries its own near-duplicate `config_summary_entries` and `summarize_sandbox_policy` helpers in [event_processor_with_human_output.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_human_output.rs#L418-L508).

During repository search, no in-tree call sites were found for `create_config_summary_entries`; today it appears to be exported for future/shared use more than active internal reuse.

## Testing Coverage

This crate contains unit tests only, all colocated in [sandbox_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/sandbox_summary.rs#L53-L103).

Covered cases:

- `ExternalSandbox` without network suffix
- `ExternalSandbox` with network suffix
- `ReadOnly` with network suffix
- `WorkspaceWrite` with explicit writable root plus network suffix

I also ran:

```bash
cargo test -p codex-utils-sandbox-summary
```

Result:

- 4 unit tests passed
- 0 doc tests failed

Current gaps:

- No tests for [create_config_summary_entries](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L7-L39).
- No test covering `DangerFullAccess`.
- No test covering `ReadOnly` without network access.
- No test covering `WorkspaceWrite` when `/tmp` and `$TMPDIR` are included.
- No test for ordering stability of returned summary entries.
- No test for reasoning-entry gating on `WireApi::Responses`.

## Design Notes

### Thin formatting layer

- The crate intentionally avoids business logic.
- It assumes `Config` and `SandboxPolicy` are already valid and resolved upstream.
- This makes the crate easy to reuse, but also means it cannot explain why a policy was chosen; it only renders the result.

### Ordered vector instead of a struct

- Returning `Vec<(&'static str, String)>` makes the output easy to iterate and display in order.
- It also keeps the API loosely coupled to any specific UI.
- The tradeoff is weaker type safety: keys are stringly typed, and consumers must know the expected labels.

### Compact summaries over full fidelity

- The sandbox formatter intentionally omits several details from the underlying policy:
  - `ReadOnly.access`
  - `WorkspaceWrite.read_only_access`
  - any richer filesystem restrictions beyond writable-root display
- That is good for concise status output, but it means the summary is not a full serialization of the effective policy.

## Design Risks and Observations

- There is duplication between this crate and `exec` in [event_processor_with_human_output.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_human_output.rs#L418-L508).
- There is already a behavior drift around reasoning-summary defaults:
  - this crate uses `"none"` in [config_summary.rs](file:///Users/bytedance/project/codex/codex-rs/utils/sandbox-summary/src/config_summary.rs#L29-L35)
  - the TUI status card uses `"auto"` in [card.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/status/card.rs#L266-L272)
- Because the crate returns human-readable strings rather than typed display models, compatibility depends on exact label spelling and stable phrasing.

## Open Questions

1. Should `exec` and TUI status rendering fully adopt this crate?
   - The duplicated logic in [event_processor_with_human_output.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/event_processor_with_human_output.rs#L418-L508) suggests the formatting contract is not yet centralized.

2. Should reasoning-summary defaults be standardized?
   - This crate reports `"none"` while the TUI card logic reports `"auto"` for missing summaries in [card.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/status/card.rs#L266-L272).

3. Should `create_config_summary_entries` return a typed summary object instead of string pairs?
   - A typed model would reduce downstream label coupling and make testing easier.

4. Should sandbox summaries expose more detail?
   - Current output hides `read_only_access` and other finer-grained filesystem restrictions, which may matter for debugging permission issues.

5. Should the crate add direct tests for config summary generation?
   - The most reusable exported function currently has no local tests, which increases the chance of unnoticed formatting drift.

## Bottom Line

`codex-utils-sandbox-summary` is a small, useful formatting crate that centralizes one piece of otherwise easy-to-duplicate presentation logic: turning sandbox and config state into stable human-readable summaries.

Its strengths are:

- minimal API surface
- low dependency count
- straightforward behavior
- tested sandbox formatting

Its main opportunities are:

- broader reuse across `exec` and TUI
- stronger tests around config summary generation
- clearer standardization of reasoning-summary defaults
