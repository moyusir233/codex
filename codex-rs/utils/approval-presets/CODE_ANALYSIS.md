# CODE_ANALYSIS

## Overview

`codex-utils-approval-presets` is a very small utility crate that centralizes the built-in permission presets used by Codex UI flows. It does not implement approval logic itself. Instead, it packages together:

- an approval policy (`AskForApproval`)
- a sandbox policy (`SandboxPolicy`)
- stable metadata for presentation (`id`, `label`, `description`)

The crate exists to keep this preset catalog in one place and keep it UI-agnostic so multiple frontends can consume the same definitions.

## Crate Structure

- `Cargo.toml`
  - Defines the package as `codex-utils-approval-presets`
  - Has exactly one runtime dependency: `codex-protocol`
- `src/lib.rs`
  - Defines the `ApprovalPreset` data type
  - Exposes the built-in preset list through `builtin_approval_presets()`
- `BUILD.bazel`
  - Registers the crate for Bazel as `approval-presets`

There are no submodules and no crate-local integration/unit test files.

## Responsibilities

This crate has one clear responsibility: provide the canonical built-in mapping from a user-facing permissions preset to the underlying protocol-level execution policies.

Concretely, it is responsible for:

1. Defining the public preset shape (`ApprovalPreset`)
2. Providing stable preset identifiers that downstream code can match on
3. Bundling UI copy with executable policy values
4. Reusing `codex-protocol` constructors so the preset semantics stay aligned with the protocol crate

It is not responsible for:

- validating policy transitions
- persisting config
- rendering UI
- enforcing sandbox behavior
- approval routing or review flows

Those responsibilities live in downstream crates, especially `tui` and the protocol/core layers.

## Public API

### `ApprovalPreset`

The crate exposes a single public struct:

```rust
pub struct ApprovalPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub approval: AskForApproval,
    pub sandbox: SandboxPolicy,
}
```

Implications of this design:

- All metadata strings are compile-time static
- The policy values are concrete protocol-layer types, not crate-specific wrappers
- All fields are public, so downstream callers can inspect and pattern-match directly
- The type derives `Debug` and `Clone`, which matches how the TUI duplicates preset values into popup actions and events

### `builtin_approval_presets() -> Vec<ApprovalPreset>`

This is the crate's only function. It constructs and returns a fresh vector every time it is called.

Current built-in presets:

| id | label | approval | sandbox | Meaning |
|---|---|---|---|---|
| `read-only` | `Read Only` | `AskForApproval::OnRequest` | `SandboxPolicy::new_read_only_policy()` | Read workspace/files, but edits and network require escalation |
| `auto` | `Default` | `AskForApproval::OnRequest` | `SandboxPolicy::new_workspace_write_policy()` | Workspace write access and command execution, but outside edits/network still require approval |
| `full-access` | `Full Access` | `AskForApproval::Never` | `SandboxPolicy::DangerFullAccess` | No approval prompt and unrestricted access |

Important detail: the function constructs protocol policies via `codex-protocol` helper constructors rather than manually spelling out the enum variants for the restricted modes. That keeps the preset defaults aligned with protocol semantics.

## Dependency Analysis

This crate has exactly one dependency:

- `codex-protocol`

That dependency provides:

- `AskForApproval`
- `SandboxPolicy`
- the helper constructors:
  - `SandboxPolicy::new_read_only_policy()`
  - `SandboxPolicy::new_workspace_write_policy()`

This means the crate is intentionally thin: it does not define permission semantics itself, it imports the canonical semantics from the shared protocol layer.

### Effective semantics imported from `codex-protocol`

From the protocol definitions:

- `AskForApproval::OnRequest` means the model decides when to ask the user for approval
- `AskForApproval::Never` means the system should never ask the user to approve commands
- `SandboxPolicy::new_read_only_policy()` creates `ReadOnly { access: FullAccess, network_access: false }`
- `SandboxPolicy::new_workspace_write_policy()` creates `WorkspaceWrite { writable_roots: [], read_only_access: FullAccess, network_access: false, exclude_tmpdir_env_var: false, exclude_slash_tmp: false }`
- `SandboxPolicy::DangerFullAccess` is unrestricted

So the preset meanings come almost entirely from protocol-layer policy behavior.

## Control Flow and Downstream Usage

Even though this crate is small, it sits at a meaningful boundary between protocol policy and UI behavior.

### Main flow in TUI permissions popup

The main consumer is the TUI permissions picker:

1. `ChatWidget::open_permissions_popup()` calls `builtin_approval_presets()`
2. The TUI filters or annotates presets based on platform and config state
3. The selected preset is converted into application events
4. Event handlers update the in-memory approval policy and sandbox policy
5. The UI logs a history cell such as `Permissions updated to Default`

In practice, the preset object is treated as the source of truth for the two policy values plus display copy.

### Downstream special-casing by `id`

The TUI relies on the preset `id` field as a stable protocol between this crate and the UI. Current downstream behavior includes:

- `read-only`
  - hidden from the permissions popup on non-Windows
- `auto`
  - treated as the default/agent mode preset
  - reused in Windows sandbox setup flows
  - label adjusted in degraded Windows sandbox mode
- `full-access`
  - requires a confirmation dialog before being applied unless warning suppression is enabled

This is an important design property: the ids are not just decorative. They are behaviorally significant.

### Matching current state to a preset

The TUI has a custom `preset_matches_current()` helper to determine which preset should be shown as current. It compares:

- approval policy equality
- sandbox variant equality
- network access equality for read-only/workspace-write modes

It intentionally does not require exact equality for every `WorkspaceWrite` detail. For example, a current workspace-write sandbox with extra writable roots still counts as matching the `auto` preset in UI tests. That means the preset catalog is treated as a coarse-grained mode classification, not a byte-for-byte serialization of current runtime policy.

### Windows-specific reuse

On Windows, the `auto` preset is reused in sandbox onboarding and fallback flows. That means this crate also acts as a small policy seed for setup wizards, not only for the generic permissions popup.

## Design Characteristics

### Strengths

- **Single source of truth**
  - Built-in presets are defined once instead of duplicated across UI layers
- **UI-agnostic core**
  - The crate exports plain Rust data, not widget-specific code
- **Low maintenance**
  - Small API surface and one dependency keep it easy to understand
- **Protocol alignment**
  - Semantics stay anchored to `codex-protocol` constructors and enums

### Trade-offs

- **Stringly-typed preset identity**
  - Downstream code matches on `"read-only"`, `"auto"`, and `"full-access"` rather than using an enum
- **Fresh allocation on each call**
  - `builtin_approval_presets()` returns a new `Vec` every time instead of a static slice
- **No lookup helpers**
  - Consumers must iterate and search manually
- **No local invariant tests**
  - There is no crate-local test ensuring ids remain unique or required presets remain present

### Why the current design still makes sense

For such a small catalog, the current design is pragmatic:

- three presets make reallocation negligible
- cloning is cheap enough for popup/event use
- public fields reduce friction for TUI consumers
- keeping policy ownership in `codex-protocol` avoids semantic duplication

## Testing Status

### Crate-local tests

This crate currently has no unit tests, integration tests, or doctests with assertions. Running:

```bash
cargo test -p codex-utils-approval-presets
```

completed successfully and reported `0` unit tests and `0` doctests.

### Downstream tests that indirectly cover this crate

Most confidence currently comes from TUI tests that depend on the preset catalog:

- permissions popup snapshots
- matching current config to the `auto` preset
- full-access confirmation flow
- Windows sandbox onboarding/fallback flows
- history-cell emission when a preset is selected

These tests exercise the crate indirectly by assuming:

- the built-in preset ids exist
- the labels and descriptions remain sensible for UI rendering
- the policy pairings keep their current meaning

### Coverage gap

Because coverage is indirect, a change inside this crate could fail in a downstream UI snapshot or behavior test, but the crate itself does not currently assert its own invariants.

Good lightweight tests for this crate would include:

- preset ids are unique
- required preset ids exist
- `full-access` uses `AskForApproval::Never` + `DangerFullAccess`
- `auto` uses `OnRequest` + `new_workspace_write_policy()`
- `read-only` uses `OnRequest` + `new_read_only_policy()`

## Key Design Dependencies Outside the Crate

The crate is small enough that much of its meaning comes from external code:

- `protocol/src/protocol.rs`
  - defines what `AskForApproval` and `SandboxPolicy` actually do
- `tui/src/chatwidget.rs`
  - renders the selection UI, matches current state, adds Windows-specific behavior, and dispatches updates
- `tui/src/app_event.rs`
  - carries `ApprovalPreset` across popup/setup flows
- `tui/src/app/event_dispatch.rs`
  - applies the selected preset values to live config state
- `tui/src/chatwidget/tests/permissions.rs`
  - documents most observable behavior expected from the preset catalog

## Design Risks and Maintenance Notes

1. **Stable ids are effectively API**
   - Renaming `auto`, `read-only`, or `full-access` would break UI behavior unless downstream code is updated everywhere.

2. **Descriptions are part of UX contract**
   - They are user-visible copy, and snapshot tests may depend on them remaining stable enough.

3. **Comment promises broader reuse than current code shows**
   - The source comment says the crate should be reusable by both TUI and MCP server, but the current repository usage appears TUI-only.

4. **Preset ordering may matter**
   - The returned vector order determines popup ordering and keyboard navigation behavior in the TUI.

5. **Current matching logic is broader than exact equality**
   - The TUI intentionally treats some customized workspace-write policies as still matching `auto`, so callers should not assume presets represent exact runtime config round-trips.

## Open Questions

1. Should preset identity become a dedicated enum instead of string ids?
   - That would make downstream matching safer and easier to refactor.

2. Should the crate expose lookup helpers?
   - Example: `find_builtin_approval_preset(id)` or dedicated accessors like `default_preset()`.

3. Should the built-in list be static?
   - A `static` slice or `LazyLock<[ApprovalPreset; 3]>` would avoid repeated allocation, though the current overhead is trivial.

4. Should crate-local tests lock the catalog?
   - Right now correctness is enforced indirectly by downstream tests.

5. Is MCP server consumption planned or obsolete?
   - The code comment suggests reuse outside TUI, but repository references currently point only to TUI.

6. Should there be an explicit compatibility policy for preset ordering and ids?
   - Downstream code already behaves as if these are stable API contracts.

## Bottom Line

This crate is intentionally tiny, but it is not trivial in impact. It defines the canonical built-in permission modes that the TUI uses to present and apply execution/sandbox behavior. Its core value is centralization and consistency rather than complex logic.

The implementation is clean and deliberately minimal. The main weaknesses are the lack of crate-local tests and the reliance on string ids as behavioral keys. If the preset catalog stays small, the current design is perfectly workable; if more modes or more consumers are added, a stronger typed identity and local invariant tests would become more valuable.
