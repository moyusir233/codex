# codex-collaboration-mode-templates crate analysis

## Overview

`codex-collaboration-mode-templates` is a very small asset crate. Its job is not to implement collaboration-mode logic itself, but to package the canonical markdown prompt fragments that other crates inject into session configuration.

At a high level, the crate does three things:

1. embeds four markdown files into the Rust binary at compile time,
2. exposes those files as `pub const &str` values,
3. makes the same markdown files available to Bazel builds as compile data.

The implementation is intentionally minimal:

- Manifest: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/Cargo.toml#L1-L13)
- Rust API surface: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/src/lib.rs#L1-L4)
- Embedded assets:
  - [plan.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/plan.md#L1-L128)
  - [default.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/default.md#L1-L10)
  - [execute.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/execute.md#L1-L45)
  - [pair_programming.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/pair_programming.md#L1-L7)
- Bazel target: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/BUILD.bazel#L1-L12)

This crate has no internal module tree beyond `src/lib.rs`, no dynamic loading, and no logic beyond `include_str!`.

## Concrete responsibilities

### 1. Package collaboration-mode prompt text

`src/lib.rs` exports four constants:

```rust
pub const PLAN: &str = include_str!("../templates/plan.md");
pub const DEFAULT: &str = include_str!("../templates/default.md");
pub const EXECUTE: &str = include_str!("../templates/execute.md");
pub const PAIR_PROGRAMMING: &str = include_str!("../templates/pair_programming.md");
```

Source: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/src/lib.rs#L1-L4)

This makes the templates:

- versioned with the codebase,
- embedded into the compiled artifact,
- accessible without filesystem I/O at runtime,
- easy for downstream crates to re-export or post-process.

### 2. Define the canonical wording for each collaboration style

The markdown assets themselves are the real product of this crate:

- [default.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/default.md#L1-L10)
  - acts as a parameterized template rather than fixed prose,
  - contains placeholder tokens such as `{{KNOWN_MODE_NAMES}}`,
  - is meant to be rendered downstream with runtime-specific values.

- [plan.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/plan.md#L1-L128)
  - defines the detailed “Plan Mode” conversational contract,
  - includes rules about exploration, allowed mutations, question-asking, and `<proposed_plan>` formatting,
  - is already fully authored text, not a parameterized template.

- [execute.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/execute.md#L1-L45)
  - describes an assumptions-first, execution-oriented collaboration style,
  - encourages independent completion and visible progress updates.

- [pair_programming.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/pair_programming.md#L1-L7)
  - defines a more interactive, alignment-heavy collaboration style.

### 3. Bridge Cargo and Bazel builds

[BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/BUILD.bazel#L1-L12) mirrors the Cargo crate as a Bazel target and explicitly marks `templates/*.md` as `compile_data`.

That is important because `include_str!` conceptually depends on those source files during compilation. The Bazel rule ensures the markdown files are available to the Rust compile action.

## Public API

The public API is intentionally tiny:

- `pub const PLAN: &str`
- `pub const DEFAULT: &str`
- `pub const EXECUTE: &str`
- `pub const PAIR_PROGRAMMING: &str`

There are no functions, traits, structs, builder types, feature flags, or error types in this crate.

### Practical API contract

Although the exported type is only `&'static str`, there is an implicit contract around each constant:

- `PLAN` is expected to be used as ready-to-apply developer instructions.
- `DEFAULT` is expected to be rendered as a template before use because it contains placeholders.
- `EXECUTE` and `PAIR_PROGRAMMING` are intended as ready-to-use prompt bodies for future or internal collaboration modes.

That last point is design-relevant: the crate’s API is “just strings,” but consumers are expected to know whether each string is raw prompt text or a templated prompt body.

## Internal flow

Even though the crate itself has almost no runtime logic, its integration flow inside the workspace is clear.

### Compile-time flow

1. Cargo or Bazel compiles this crate.
2. `include_str!` copies each markdown file into the binary as a `&'static str`.
3. Downstream crates depend on the compiled constants rather than reading template files at runtime.

### Runtime flow in current workspace usage

The main current consumer is [collaboration_mode_presets.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets.rs#L1-L115) in `codex-models-manager`.

The flow there is:

1. Import `DEFAULT` and `PLAN` from this crate.
2. Parse `DEFAULT` with `codex_utils_template::Template`.
3. Fill in placeholder values such as known mode names and whether `request_user_input` is available.
4. Build `CollaborationModeMask` presets that carry the final `developer_instructions`.
5. Expose those presets via `ModelsManager::list_collaboration_modes()`.

Representative code:

```rust
use codex_collaboration_mode_templates::DEFAULT as COLLABORATION_MODE_DEFAULT;
use codex_collaboration_mode_templates::PLAN as COLLABORATION_MODE_PLAN;

static COLLABORATION_MODE_DEFAULT_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    Template::parse(COLLABORATION_MODE_DEFAULT)
        .unwrap_or_else(|err| panic!("collaboration mode default template must parse: {err}"))
});
```

Source: [collaboration_mode_presets.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets.rs#L1-L16)

And later:

```rust
fn plan_preset() -> CollaborationModeMask {
    CollaborationModeMask {
        name: ModeKind::Plan.display_name().to_string(),
        mode: Some(ModeKind::Plan),
        model: None,
        reasoning_effort: Some(Some(ReasoningEffort::Medium)),
        developer_instructions: Some(Some(COLLABORATION_MODE_PLAN.to_string())),
    }
}
```

Source: [collaboration_mode_presets.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets.rs#L35-L43)

For Default mode:

```rust
COLLABORATION_MODE_DEFAULT_TEMPLATE
    .render([
        (KNOWN_MODE_NAMES_TEMPLATE_KEY, known_mode_names.as_str()),
        (
            REQUEST_USER_INPUT_AVAILABILITY_TEMPLATE_KEY,
            request_user_input_availability.as_str(),
        ),
        (
            ASKING_QUESTIONS_GUIDANCE_TEMPLATE_KEY,
            asking_questions_guidance.as_str(),
        ),
    ])
    .unwrap_or_else(|err| panic!("collaboration mode default template must render: {err}"))
```

Source: [collaboration_mode_presets.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets.rs#L64-L76)

### Important integration observation

Only `PLAN` and `DEFAULT` are consumed in current Rust call sites found in this workspace:

- consumer import site: [collaboration_mode_presets.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets.rs#L1-L2)

No direct Rust usage of `EXECUTE` or `PAIR_PROGRAMMING` was found in the current codebase search, despite those constants being exported.

## Module and asset shape

This crate has a deliberately flat layout:

- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/src/lib.rs)
  - export-only module,
  - no helper functions,
  - no tests,
  - no conditional compilation.

- [templates/default.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/default.md)
  - parameterized template.

- [templates/plan.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/plan.md)
  - longest and most policy-heavy mode definition.

- [templates/execute.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/execute.md)
  - execution-focused instructions.

- [templates/pair_programming.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/pair_programming.md)
  - interactive style instructions.

This is a classic “asset-only library” pattern: code is trivial, while the embedded files carry the product behavior.

## Dependencies and build surface

### Cargo dependencies

[Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/Cargo.toml#L1-L13) declares:

- no `[dependencies]`,
- no `[dev-dependencies]`,
- workspace-inherited edition, version, license, and lint settings.

So the crate’s runtime dependency surface is effectively zero.

### Workspace inheritance

From the workspace manifest:

- the crate is a workspace member: [workspace Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L1-L126)
- it inherits Rust edition 2024 and Apache-2.0 license from `[workspace.package]`: [workspace Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L99-L106)
- it is exposed as a workspace dependency for downstream crates: [workspace Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/Cargo.toml#L123-L126)

### Downstream dependencies

The directly visible downstream dependency found during inspection is:

- `codex-models-manager`: [models-manager/Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/models-manager/Cargo.toml#L15-L29)

That crate combines these static strings with:

- `codex-protocol` types for collaboration mode modeling,
- `codex-utils-template` for placeholder rendering,
- `std::sync::LazyLock` for one-time parse setup.

## Testing and verification

### Tests inside this crate

There are currently no unit tests or integration tests in `codex-collaboration-mode-templates`.

Running:

```bash
cargo test -p codex-collaboration-mode-templates
```

produced:

```text
running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Indirect test coverage

The actual behavior of the most important template, `DEFAULT`, is tested indirectly in `codex-models-manager`:

- [collaboration_mode_presets_tests.rs](file:///Users/bytedance/project/codex/codex-rs/models-manager/src/collaboration_mode_presets_tests.rs#L1-L53)

Those tests verify that:

- preset names match `ModeKind::display_name()`,
- `PLAN` maps to medium reasoning effort,
- `DEFAULT` placeholder markers are fully replaced after rendering,
- `request_user_input` guidance changes based on feature configuration.

This means the current workspace treats template validation as a consumer responsibility rather than a responsibility of the asset crate itself.

## Design choices

### Strengths

- **Very low complexity**
  - The crate is almost impossible to misuse internally because it has no mutable state, parsing logic, or runtime I/O.

- **Compile-time embedding**
  - `include_str!` removes deployment/runtime path concerns and guarantees the strings stay in sync with the compiled revision.

- **Clear separation of concerns**
  - This crate owns text assets.
  - `codex-models-manager` owns rendering and configuration-specific specialization.

- **Build-system parity**
  - Cargo and Bazel both know about the markdown inputs.

### Tradeoffs

- **Stringly typed API**
  - Consumers get raw `&str` values with no type distinction between:
    - fully static instructions,
    - parameterized templates,
    - currently active modes,
    - exported-but-unused modes.

- **Validation lives downstream**
  - If `default.md` acquires malformed placeholders, this crate still compiles.
  - The failure appears when a consumer parses or renders the text.

- **No discoverable semantic metadata**
  - There is no enum, registry, or metadata structure that says which modes are supported, visible, templated, deprecated, or unused.

## Notable code-level observations

### 1. `DEFAULT` is not just content; it is a template contract

`default.md` contains:

```md
Known mode names are {{KNOWN_MODE_NAMES}}.

## request_user_input availability

{{REQUEST_USER_INPUT_AVAILABILITY}}

{{ASKING_QUESTIONS_GUIDANCE}}
```

Source: [default.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/default.md#L5-L10)

That makes this crate a quiet dependency of the placeholder protocol between `collaboration-mode-templates` and `codex-utils-template` consumers.

### 2. `PLAN` is the most behaviorally important asset

`plan.md` is large and prescriptive. It defines:

- what “Plan Mode” means,
- what kinds of actions are allowed,
- when the agent must ask questions,
- the exact `<proposed_plan>` output protocol.

So although the crate is mechanically tiny, changes to [plan.md](file:///Users/bytedance/project/codex/codex-rs/collaboration-mode-templates/templates/plan.md#L1-L128) have broad behavioral impact on the user-facing agent.

### 3. Exported modes exceed currently surfaced modes

`ModeKind` still contains `PairProgramming` and `Execute`, but they are hidden from serde and from the TUI-visible mode list:

- mode definitions: [config_types.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/config_types.rs#L392-L430)
- visible modes: [config_types.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/config_types.rs#L412-L425)

At the same time, this crate exports `EXECUTE` and `PAIR_PROGRAMMING`.

That suggests one of two architectural interpretations:

- these templates are retained for future reactivation or internal use,
- or they are legacy prompt assets that have outlived their active product wiring.

## Open questions

1. Are `EXECUTE` and `PAIR_PROGRAMMING` intentionally dormant?
   - They are exported here, but no active Rust consumer was found.
   - `ModeKind` includes matching variants, yet those variants are hidden from serialization and not TUI-visible.

2. Should this crate validate template correctness itself?
   - A tiny test that parses `DEFAULT` with `codex_utils_template::Template` would catch placeholder syntax regressions earlier and closer to the source.

3. Should the API distinguish rendered templates from raw prompt bodies?
   - Today callers must know out of band that `DEFAULT` needs rendering while `PLAN` does not.
   - An enum or typed accessor could make that contract explicit.

4. Should prompt assets carry metadata?
   - For example: mode name, visibility, whether placeholders are required, and whether the mode is product-supported.
   - That could reduce duplication across this crate, `codex-protocol`, and `codex-models-manager`.

5. Is the current ownership boundary still the right one?
   - The crate is cleanly isolated now.
   - If more templating logic or mode metadata accumulates, it may want to own more than raw strings.

## Bottom line

`codex-collaboration-mode-templates` is a deliberately tiny but behaviorally important crate. Its code is simple; its embedded markdown is the real product surface. The crate centralizes the source text for collaboration modes, keeps those assets compile-time embedded, and lets downstream crates such as `codex-models-manager` turn them into concrete session instructions.

The main architectural subtlety is that the crate exports raw strings with different semantic roles:

- some are immediately usable instructions,
- some are templates requiring downstream rendering,
- and some appear to be exported for modes that are not currently wired into the visible product flow.

That makes the crate easy to maintain in the small, but it also means the most important contracts are implicit and enforced socially or downstream rather than by the type system.
