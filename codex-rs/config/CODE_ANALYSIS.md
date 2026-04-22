# codex-config crate analysis

## Overview

`codex-config` is the shared configuration model and normalization crate for Codex. It does not appear to be the full top-level config loader by itself; instead, it provides:

- strongly typed TOML schemas for `config.toml`, `requirements.toml`, permissions, profiles, MCP servers, and related subdomains
- config-layer primitives that represent system/user/project/session layers with stable precedence
- merge and canonicalization helpers for composing layered TOML
- requirement/constraint machinery for turning managed policy into runtime-enforced limits
- diagnostics utilities that map TOML/schema failures back to concrete file locations
- focused editors for mutating config-managed sections such as MCP servers and marketplaces

The public surface is intentionally broad because this crate acts as a central shared vocabulary for other Codex crates. `src/lib.rs` mostly re-exports submodules rather than implementing behavior directly.

## Primary responsibilities

### 1. Define the user-facing config schema

The largest schema type is `ConfigToml` in `src/config_toml.rs`. It models the contents of `config.toml` across many product areas:

- model selection and provider configuration
- approval and sandbox defaults
- shell environment policy
- MCP server declarations
- profiles, projects, skills, plugins, marketplaces
- TUI, analytics, feedback, memories, apps, realtime, OTEL, Windows-specific settings

Notable design choices:

- nearly every field is optional, so callers can layer and default values later
- `#[schemars(deny_unknown_fields)]` is used widely to keep the schema strict
- some fields use custom deserializers to preserve backward compatibility while still validating the final shape
- `ConfigToml` includes a few behavioral helpers, not just raw data, such as sandbox-policy derivation and active-project/profile resolution

### 2. Represent and merge layered config

`src/state.rs` provides the core layering abstraction:

- `ConfigLayerEntry` stores one config source, its raw/parsed TOML, a content fingerprint, and optional disabled reason
- `ConfigLayerStack` stores ordered layers plus requirement objects and exposes merged/effective views
- `ConfigLayerStack::effective_config()` merges enabled layers from lowest precedence to highest precedence
- `ConfigLayerStack::origins()` records which layer supplied each effective leaf value

This is the backbone for explaining configuration provenance to higher-level consumers.

### 3. Normalize alias keys during merge/origin tracking

`src/merge.rs` and `src/key_aliases.rs` ensure layered config behaves correctly even when old and new key names coexist. Today the implemented alias is:

- `memories.no_memories_if_mcp_or_web_search` -> `memories.disable_on_external_context`

The merge path normalizes both the base and overlay at each table level, and origin tracking also uses canonicalized keys. This keeps mixed-version configs from surfacing duplicate or misleading state.

### 4. Convert managed requirements into enforceable constraints

`src/config_requirements.rs` and `src/constraint.rs` are the policy side of the crate.

- `ConfigRequirementsToml` models managed/system/cloud policy inputs
- `ConfigRequirementsWithSources` preserves provenance while merging sources
- `ConfigRequirements` is the normalized runtime form
- `Constrained<T>` wraps a value with a validator and optional normalizer

The crate uses this machinery to restrict:

- approval policy
- approvals reviewer
- sandbox policy
- web search mode
- residency
- exec policy rules
- network/filesystem permissions
- MCP server availability
- feature/app allow-lists

The managed-policy design is notable because it preserves both:

- an effective constrained value for runtime use
- the original requirement source for user-facing error messages

### 5. Surface precise config diagnostics

`src/diagnostics.rs` maps parse/deserialize errors to file/line/column ranges. It supports:

- low-level TOML parse error reporting
- typed-deserialization error reporting using `serde_path_to_error`
- scanning layer entries to find the first concrete file responsible for a merged-config failure
- formatting source snippets with carets for CLI/TUI display

This is important because the crate merges multiple files, but users still need actionable file-local feedback.

### 6. Provide focused config mutation helpers

The crate includes small editors rather than a generic mutable config API:

- `src/mcp_edit.rs` reads and rewrites `[mcp_servers]`
- `src/marketplace_edit.rs` updates/removes `[marketplaces]`

These use `toml_edit` so the crate can preserve TOML structure more safely than string concatenation, while still regenerating targeted sections in a deterministic way.

## Public API shape

The public API is mostly defined by re-exports in `src/lib.rs`. The main clusters are:

### Config model and schema

- `config_toml::ConfigToml`
- `types::*`
- `profile_toml::ConfigProfile`
- `permissions_toml::*`
- `schema::*`

### Layering and diagnostics

- `ConfigLayerEntry`
- `ConfigLayerStack`
- `ConfigLayerStackOrdering`
- `merge_toml_values`
- `ConfigError`
- `ConfigLoadError`
- `first_layer_config_error`

### Managed requirements / constraints

- `ConfigRequirements`
- `ConfigRequirementsToml`
- `ConfigRequirementsWithSources`
- `Constrained`
- `ConstraintError`
- `RequirementsExecPolicy*`
- `CloudRequirementsLoader`

### MCP / marketplace editing

- `McpServerConfig` and related MCP types
- `ConfigEditsBuilder`
- `load_global_mcp_servers`
- `MarketplaceConfigUpdate`
- `record_user_marketplace`
- `remove_user_marketplace_config`

### Thread/session-scoped config

- `ThreadConfigLoader`
- `ThreadConfigContext`
- `SessionThreadConfig`
- `StaticThreadConfigLoader`
- `NoopThreadConfigLoader`

## End-to-end configuration flow

The crate does not contain one single `load_everything()` entrypoint. Instead, the effective flow appears to be:

1. Higher-level code reads one or more config files and constructs `ConfigLayerEntry` values.
2. A `ConfigLayerStack` is created in strict precedence order.
3. `effective_config()` merges enabled TOML layers using `merge_toml_values()`.
4. Callers deserialize the merged TOML into `ConfigToml` or other typed structs.
5. If managed/system/cloud requirements exist, they are merged via `ConfigRequirementsWithSources`.
6. Requirements are converted into `ConfigRequirements`, which produce `Constrained<T>` runtime guards.
7. Callers apply helpers such as:
   - `ConfigToml::derive_sandbox_policy()`
   - `ConfigToml::get_active_project()`
   - `ConfigToml::get_config_profile()`
8. If deserialization fails, diagnostics utilities map the failure back to a specific config file and text range.

This separation suggests that orchestration likely lives in another crate, while `codex-config` remains the shared domain layer.

## Key modules

### `src/lib.rs`

Role:

- crate façade and re-export hub
- establishes what downstream crates should consider stable/public

Observations:

- only a handful of submodules are public directly; most are re-exported selectively
- the crate prefers stable type aliases/re-exports over exposing all internal module structure

### `src/config_toml.rs`

Role:

- central user config schema
- a few important resolution helpers

Important behaviors:

- `derive_sandbox_policy()` resolves sandbox mode from explicit overrides, profile config, base config, project trust, Windows capability, and managed constraints
- `get_active_project()` matches cwd or repo-root against normalized project keys
- `get_config_profile()` resolves the chosen profile or errors if the requested one does not exist
- `validate_model_providers()` prevents overriding reserved built-in provider IDs and delegates provider-specific validation
- `validate_oss_provider()` restricts accepted local-model provider identifiers and emits a targeted error for removed legacy Ollama mode

### `src/types.rs`

Role:

- repository for secondary config types with minimal business logic
- defaulting and simple conversions for subdomains like memories, apps, OTEL, shell policy, TUI, and marketplace/plugin settings

Important behaviors:

- `MemoriesConfig::from(MemoriesToml)` clamps unsafe/unbounded numeric inputs into supported ranges
- `ShellEnvironmentPolicyToml -> ShellEnvironmentPolicy` compiles string wildcards into case-insensitive matchers
- multiple types are shared with JSON Schema generation and app-server protocol conversion

### `src/state.rs`

Role:

- config-layer representation, precedence validation, effective merge, and origin tracking

Important behaviors:

- verifies layers are sorted and project layers progress from root to cwd
- supports replacing or injecting a user layer without rebuilding the entire stack manually
- excludes disabled layers from effective merge/origin tracking by default

### `src/merge.rs` + `src/key_aliases.rs`

Role:

- deep TOML merge with overlay precedence and key-alias canonicalization

Important behaviors:

- tables merge recursively
- non-table values replace the previous value entirely
- aliases are normalized both when merging and when inserting new subtrees

### `src/config_requirements.rs`

Role:

- managed requirement schema, source-aware merging, and conversion to runtime constraints

Important behaviors:

- merges fields from multiple managed sources without losing provenance
- merges app enablement in a restrictive descending manner, where any `enabled = false` wins
- applies hostname-specific remote-sandbox rules
- translates requirement TOML into `Constrained<T>` values with good error messages

This is the most policy-heavy module in the crate and likely one of its highest-risk maintenance areas because it encodes cross-source precedence and enforcement semantics.

### `src/requirements_exec_policy.rs`

Role:

- TOML representation and validation layer for executable command policy rules

Important behaviors:

- validates that prefix-rule lists and tokens are non-empty
- forbids `allow` decisions in requirements, only permitting restrictive overlays (`prompt`/`forbidden`)
- converts TOML-friendly token structures into `codex-execpolicy`’s internal `Policy`

### `src/diagnostics.rs`

Role:

- precise file/range mapping for config errors

Important behaviors:

- typed deserialization with path-aware error localization
- best-effort backtracking from merged config failure to the first offending concrete layer file
- formatted codeframe output suitable for CLI display

### `src/mcp_types.rs`

Role:

- validated MCP server schema and transport model

Important behaviors:

- distinguishes the raw input shape (`RawMcpServerConfig`) from the validated runtime shape (`McpServerConfig`)
- enforces mutual exclusivity and transport-specific field legality
- blocks inline bearer tokens and instead steers callers toward env-var-backed secrets
- supports per-tool approval settings and parallel-tool-call declarations

The `Raw -> validated` split is a strong design choice because it forces every new MCP field through explicit validation logic.

### `src/mcp_edit.rs`

Role:

- deterministic read/write path for global MCP server config inside `config.toml`

Important behaviors:

- `load_global_mcp_servers()` parses only the MCP section and rejects inline bearer tokens
- `ConfigEditsBuilder` updates the MCP section asynchronously via `spawn_blocking`
- serialization is deterministic for maps, tool tables, headers, env vars, and arrays

### `src/marketplace_edit.rs`

Role:

- targeted editing of marketplace metadata in the user config

Important behaviors:

- upserts marketplace metadata
- removes entries from both standard and inline-table TOML forms
- detects case-mismatch removals and returns a structured outcome

### `src/thread_config.rs`

Role:

- abstraction for session/thread-scoped config sources

Important behaviors:

- `ThreadConfigLoader` fetches typed source payloads, then can translate them into ordinary config layers
- session-owned config is converted into a synthetic `SessionFlags` config layer
- user-owned thread config is intentionally a placeholder with no TOML-backed fields yet

This is a good extension seam for remote/session-owned config without complicating file-based config loading.

### `src/schema.rs`

Role:

- generates JSON Schema for `config.toml`

Important behaviors:

- injects only known feature keys for `[features]`
- uses `RawMcpServerConfig` to describe MCP input syntax rather than the validated runtime type
- canonicalizes schema JSON for stable output ordering

## Merge, precedence, and normalization semantics

### Layer precedence

`ConfigLayerStack` expects layers ordered from lowest precedence to highest precedence. Later layers override earlier ones. Validation also checks:

- only one user layer exists
- project layers form a root-to-cwd ancestry chain

Disabled layers are kept in the stack but skipped by default for effective merge/origin reporting.

### Merge strategy

`merge_toml_values()` performs:

- recursive table merge
- whole-value replacement for non-tables
- path-sensitive alias normalization during the merge

This is a simple but reliable model and avoids trying to do array-level semantic merges.

### Requirement precedence

Managed requirement merging differs from config merging:

- regular config is overlay-based by layer precedence
- requirements use source-aware field filling, where lower-priority managed sources can provide values only when higher-priority ones did not
- app enablement is special-cased so restrictive disablement propagates downward

The crate therefore has two distinct precedence systems:

- configuration override precedence
- managed policy constraint precedence

That distinction is deliberate and important.

## Dependency analysis

### Internal workspace dependencies

The crate sits at the center of configuration concerns and depends on many Codex workspace crates:

- `codex-app-server-protocol`: config layer/source metadata and protocol conversion types
- `codex-protocol`: shared enums for approval, sandbox, web search, reasoning, personality, etc.
- `codex-execpolicy`: exec-policy representation used by requirements rules
- `codex-features`: known feature list and typed feature config
- `codex-model-provider-info`: provider descriptors and validation
- `codex-network-proxy`: network-permission config mapping
- `codex-utils-absolute-path` and `codex-utils-path`: validated absolute paths and path normalization

This shows the crate is a central integration point rather than a narrow leaf crate.

### External crates

- `serde`, `toml`, `serde_json`: core serialization/deserialization
- `schemars`: JSON Schema generation
- `toml_edit`: safe targeted TOML mutation
- `serde_path_to_error`: precise schema path diagnostics
- `sha2`: stable content fingerprints for config-layer versioning
- `wildmatch`: wildcard matching for hostnames and environment-variable filters
- `async-trait`, `futures`, `tokio`: async loaders and file IO
- `thiserror`, `anyhow`, `tracing`: error and observability support

## Testing coverage

The crate keeps many tests close to the implementation rather than in a top-level `tests/` directory. Important tested areas include:

- `merge_tests.rs`: alias normalization during layered merge
- `state_tests.rs`: origin tracking uses canonical key names
- `types_tests.rs`: skill-config parsing and memories-range clamping
- `mcp_types_tests.rs`: MCP deserialization, validation, serialization round-trips, and transport-specific field rejection
- `mcp_edit_tests.rs`: deterministic MCP config persistence and reload
- inline tests in `thread_config.rs`: typed thread config converts into synthetic session layers
- inline tests in `constraint.rs`: validator/normalizer semantics
- inline tests in `cloud_requirements.rs`: shared future runs once
- large inline test suite in `config_requirements.rs`: field merging, source attribution, exec-policy parsing, sandbox/network/filesystem constraints
- inline tests in `marketplace_edit.rs`: remove/update semantics, inline-table handling, case-mismatch detection

Overall, the test strategy focuses on semantic edge cases rather than end-to-end loading from real filesystem hierarchies.

## Design patterns and architectural notes

### Strong separation between raw input and validated runtime form

Best example:

- `RawMcpServerConfig` -> `TryFrom<RawMcpServerConfig> for McpServerConfig`

This makes illegal combinations explicit and prevents new schema fields from bypassing validation accidentally.

### Source-aware policy modeling

Best examples:

- `RequirementSource`
- `Sourced<T>`
- `ConstrainedWithSource<T>`

These enable user-facing errors like “value X is disallowed by source Y” instead of generic validation failures.

### Layer-first, type-second architecture

The crate treats config as:

1. layered TOML
2. merged TOML
3. typed structs
4. constrained runtime values

That keeps merging generic and shifts validation/defaulting to typed stages.

### Deterministic fingerprints and origins

The crate tracks:

- content fingerprints for layer versioning (`sha256:` hashes)
- field-level origin metadata for effective values

This is useful for UI/state synchronization and config explainability.

### Focused mutation helpers over generic writers

Rather than attempting arbitrary round-trip-preserving edits for the whole config, the crate offers narrow editors for MCP servers and marketplaces. That reduces complexity and risk.

## Notable strengths

- Broad, well-typed schema coverage for many Codex surfaces
- Good separation between schema, constraints, diagnostics, and editing concerns
- Strong source/provenance handling for both config layers and requirements
- Deterministic serialization/fingerprinting behavior
- Defensive validation around sensitive MCP config and exec-policy rules
- Backward-compatibility handling for renamed config keys

## Risks and maintenance hotspots

- `ConfigToml` is very large, so schema evolution and backward compatibility are ongoing maintenance risks
- `config_requirements.rs` centralizes complex policy semantics and cross-source merging rules
- config loading orchestration is split across crates, so understanding the true runtime order requires cross-crate analysis
- some behaviors are encoded as comments/TODOs rather than explicit types, especially around managed config and sandbox defaults

## Open questions

1. Where is the top-level orchestration that:
   - discovers config files
   - builds `ConfigLayerEntry` values
   - loads cloud/MDM/system requirements
   - deserializes the merged stack into final runtime config
   This crate exposes the primitives, but another crate likely owns the full pipeline.

2. Should `ConfigRequirementsToml` be expanded to specify default `SandboxPolicy` parameters, not just allowed modes? The code explicitly calls this out as a TODO when constructing sandbox constraints.

3. Why is `UserThreadConfig` currently empty? The trait shape suggests future per-user thread-scoped config, but there is no TOML-backed representation yet.

4. Should more alias migrations be centralized in `key_aliases.rs` as the config evolves, or is the current one-off approach sufficient?

5. How much formatting preservation is expected from `toml_edit`-based writers? Current MCP/marketplace writers deterministically rewrite their sections, but not necessarily with minimal diffs relative to the user’s original formatting.

6. Is there a need for higher-level end-to-end tests that construct multi-layer config stacks from disk, rather than mostly unit-testing the merge and validation primitives in isolation?

## Short conclusion

`codex-config` is a central domain crate for configuration semantics, not just a bag of structs. Its most important contributions are:

- typed schemas for user and managed config
- layer/merge/origin primitives
- policy constraint enforcement with provenance
- precise diagnostics
- targeted config editing helpers

If this crate changes, the highest-impact areas to review are `ConfigToml`, `ConfigLayerStack`, `ConfigRequirements*`, and MCP config validation/editing.
