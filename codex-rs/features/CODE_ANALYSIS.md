# codex-features crate analysis

## Scope

This document analyzes the crate at `/Users/bytedance/project/codex/codex-rs/features`. The crate is small, but it sits on an important configuration boundary: it defines the canonical feature registry for the workspace, resolves feature state from multiple configuration layers, preserves backward compatibility for legacy toggles, and emits metadata that other crates use for schema generation, validation, warnings, and telemetry.

## High-level responsibility

The crate owns the following responsibilities:

1. Define the canonical set of feature flags through the `Feature` enum and the `FEATURES` registry.
2. Classify each feature by lifecycle stage (`Stable`, `Experimental`, `UnderDevelopment`, `Deprecated`, `Removed`) and default enablement.
3. Parse feature configuration from TOML-friendly structures, including one typed feature config (`multi_agent_v2`).
4. Merge feature state from layered sources: base config, profile config, and runtime overrides.
5. Translate legacy feature names and old top-level booleans into canonical features while preserving deprecation notices.
6. Normalize a few feature dependencies after merging.
7. Expose helper APIs used by downstream crates for validation, UI/schema generation, warning generation, and telemetry.

## File/module layout

- `src/lib.rs`
  - Main crate API and implementation.
  - Defines `Stage`, `Feature`, `Features`, `FeatureOverrides`, `FeatureConfigSource`, `FeaturesToml`, `FeatureToml`, `FeatureConfig`, `FeatureSpec`, `FEATURES`, and `unstable_features_warning_event`.
- `src/legacy.rs`
  - Encapsulates legacy feature aliases and a few pre-feature-table booleans.
  - Contains the compatibility layer for old keys such as `connectors`, `collab`, `telepathy`, and legacy apply-patch / unified-exec toggles.
- `src/feature_configs.rs`
  - Holds typed TOML config for `multi_agent_v2`.
- `src/tests.rs`
  - Unit tests for registry invariants, normalization, aliasing, typed feature parsing, and unstable warning behavior.
- `Cargo.toml`
  - Declares a lean dependency set focused on serialization, schema generation, logging, telemetry, and protocol event emission.
- `BUILD.bazel`
  - Exposes the crate to Bazel and includes `node-version.txt` as compile data because tests interpolate the expected Node version into feature menu text.

## Core types and APIs

### `Stage`

`Stage` models feature lifecycle state and supports experimental-menu rendering via helper methods:

- `experimental_menu_name()`
- `experimental_menu_description()`
- `experimental_announcement()`

This is not only classification metadata. Experimental features also carry UI strings for `/experimental`, so the crate acts as both a configuration registry and a source of user-facing feature metadata.

### `Feature`

`Feature` is the canonical enum of all known feature toggles. The enum is large because the crate centralizes flags used across the workspace. Important helper methods:

- `key()` returns the canonical config key.
- `stage()` returns lifecycle metadata.
- `default_enabled()` returns the built-in default.

All three delegate through `info()`, which looks up the corresponding `FeatureSpec` in `FEATURES`. That means the enum itself is not the single source of truth; the enum and `FEATURES` must stay in sync.

### `FeatureSpec` and `FEATURES`

`FEATURES` is the registry table that drives almost everything:

- canonical config key
- lifecycle stage
- default enablement

This table is used by:

- `Features::with_defaults()` to build baseline state
- `feature_for_key()` / `canonical_feature_for_key()` for key resolution
- schema generation in `codex-config`
- UI-facing experimental metadata
- telemetry emission
- unstable warning generation
- unit tests that assert invariants about defaults and lifecycle stages

### `Features`

`Features` is the resolved feature set. Internally it stores:

- `enabled: BTreeSet<Feature>` for deterministic feature membership
- `legacy_usages: BTreeSet<LegacyFeatureUsage>` for deduplicated deprecation/compatibility notices

Important methods:

- `with_defaults()`
- `enabled()`
- `apps_enabled_for_auth()`
- `use_legacy_landlock()`
- `enable()`, `disable()`, `set_enabled()`
- `record_legacy_usage()` / `record_legacy_usage_force()`
- `legacy_feature_usages()`
- `emit_metrics()`
- `apply_map()`
- `from_sources()`
- `enabled_features()`
- `normalize_dependencies()`

This is the operational core of the crate.

### `FeaturesToml`, `FeatureToml<T>`, and `FeatureConfig`

`FeaturesToml` is the deserializable TOML shape for `[features]`.

- Most flags are flattened as `BTreeMap<String, bool>`.
- `multi_agent_v2` is special-cased to support either a boolean or a richer config object.

`FeatureToml<T>` is an untagged enum:

- `Enabled(bool)`
- `Config(T)`

`FeatureConfig` is a tiny trait that lets typed feature configs expose an optional enabled bit. Today only `MultiAgentV2ConfigToml` implements it.

### `FeatureConfigSource` and `FeatureOverrides`

These model input layers for feature resolution:

- `FeatureConfigSource` holds a `FeaturesToml` reference plus a few legacy booleans still accepted outside `[features]`.
- `FeatureOverrides` models late runtime overrides currently used for `include_apply_patch_tool` and `web_search_request`.

### Lookup helpers

- `feature_for_key(key)` resolves canonical keys and then falls back to legacy aliases.
- `canonical_feature_for_key(key)` only accepts canonical keys.
- `is_known_feature_key(key)` accepts either canonical or legacy keys.
- `legacy_feature_keys()` exposes the alias list for consumers such as schema generation.

### Warning/event API

`unstable_features_warning_event()` examines the effective features table and the resolved `Features` set and returns a `codex_protocol::protocol::Event` warning when explicitly configured under-development features are enabled and warnings are not suppressed.

## Resolution flow

The normal resolution path is `Features::from_sources(base, profile, overrides)`.

### 1. Start from built-in defaults

`Features::with_defaults()` iterates over `FEATURES` and enables everything whose `default_enabled` field is `true`.

### 2. Apply legacy top-level toggles for each source

For each `FeatureConfigSource`, `LegacyFeatureToggles::apply()` translates old booleans into canonical feature states:

- `include_apply_patch_tool` -> `ApplyPatchFreeform`
- `experimental_use_freeform_apply_patch` -> `ApplyPatchFreeform`
- `experimental_use_unified_exec_tool` -> `UnifiedExec`

These also record legacy usage notices.

### 3. Apply `[features]` table entries

If the source contains `FeaturesToml`, `apply_toml()` converts it to a plain map through `FeaturesToml::entries()` and then delegates to `apply_map()`.

Notable behavior inside `apply_map()`:

- canonical keys resolve directly
- legacy keys resolve via `legacy::feature_for_key()`
- unknown keys emit a `tracing::warn!`
- some removed compatibility flags are explicitly ignored:
  - `tui_app_server`
  - `image_detail_original`
- certain deprecated keys force more specific legacy notices:
  - `web_search_request`
  - `web_search_cached`
  - `use_legacy_landlock`

### 4. Apply late overrides

`FeatureOverrides::apply()` runs after base/profile merging. It currently handles:

- apply-patch inclusion override through the same legacy compatibility path
- `web_search_request` explicit override with legacy notice recording

### 5. Normalize feature dependencies

`normalize_dependencies()` enforces a small set of one-way dependency rules:

- `SpawnCsv` implies `Collab`
- `CodeModeOnly` implies `CodeMode`
- `JsReplToolsOnly` requires `JsRepl`; otherwise it is disabled and a warning is logged

The dependency model is intentionally simple and hard-coded.

## Legacy compatibility model

Backward compatibility is a first-class concern in this crate.

### Alias mapping

`src/legacy.rs` defines `ALIASES`, mapping old keys to canonical features. Examples:

- `connectors` -> `Apps`
- `collab` -> `Collab`
- `telepathy` -> `Chronicle`
- `request_permissions` -> `ExecPermissionApprovals`
- `include_apply_patch_tool` -> `ApplyPatchFreeform`

### Notice recording

When legacy keys are used, the crate records `LegacyFeatureUsage` entries containing:

- the alias used
- the canonical `Feature`
- a short summary
- optional migration details

This allows downstream code to surface deprecation guidance instead of silently accepting old settings.

### Logging

Legacy alias resolution also emits `tracing::info!` messages nudging callers toward canonical `[features].<key>` usage.

## Typed feature config: `multi_agent_v2`

The one feature with richer configuration is `multi_agent_v2`.

`MultiAgentV2ConfigToml` supports:

- `enabled`
- `usage_hint_enabled`
- `usage_hint_text`
- `hide_spawn_agent_metadata`

The feature crate only uses this struct to:

- deserialize TOML cleanly
- surface schema information
- infer the effective boolean when `enabled` is explicitly set

The richer semantics are consumed downstream. In `core/src/config/mod.rs`, `multi_agent_v2_toml_config()` extracts the config payload when it is provided as a table rather than a boolean.

## Downstream integration points

Even though the crate is self-contained, it is clearly designed as shared infrastructure for several other crates.

### `codex-core`

- `core/src/config/mod.rs` uses `Features::from_sources(...)` to build the configured feature set before wrapping it in `ManagedFeatures`.
- `core/src/config/mod.rs` also extracts `MultiAgentV2ConfigToml` for richer multi-agent behavior.
- `core/src/session/session.rs` uses `unstable_features_warning_event(...)` to emit a startup warning for explicitly enabled under-development features.

### `codex-config`

- `config/src/schema.rs` builds the JSON schema for `[features]` from `FEATURES` plus `legacy_feature_keys()`.
- `multi_agent_v2` receives a typed schema instead of a raw boolean schema.

### `app-server`

- `app-server/src/config_api.rs` validates feature keys, rejects legacy keys for write APIs that require canonical names, and exposes selected experimental enablement state back through API responses.

### managed feature validation

- `core/src/config/managed_features.rs` uses canonical/legacy lookup helpers and `Features::from_sources(...)` while validating workspace or environment feature requirements.

## Dependency analysis

The dependency footprint is intentionally small.

### Internal workspace dependencies

- `codex-otel`
  - Used by `Features::emit_metrics()` to report non-default feature state.
- `codex-protocol`
  - Used by `unstable_features_warning_event()` to construct warning events.

### External dependencies

- `serde` with `derive`
  - Deserializes and serializes feature TOML/config structs.
- `schemars`
  - Derives JSON schema, especially useful for config editor/tooling support.
- `toml`
  - Supplies `toml::Table` for unstable warning inspection.
- `tracing`
  - Logs unknown keys, alias usage, and dependency normalization problems.

### Dev dependency

- `pretty_assertions`
  - Improves test diffs for map/struct equality assertions.

## Testing coverage

The crate has a focused unit test suite in `src/tests.rs`. It does not try to test every feature flag individually; instead it tests invariants and special behaviors.

### Registry invariants

Tests assert broad rules over the registry:

- under-development features are disabled by default
- default-enabled features are only `Stable` or `Removed`
- specific lifecycle decisions for selected features remain intentional

This is a good fit for a registry crate because accidental metadata regressions are more likely than algorithmic bugs.

### Resolution and normalization

Tests verify:

- `CodeModeOnly` implies `CodeMode`
- `SpawnCsv` implies `Collab` one-way
- base config, profile config, and overrides merge in the expected order
- removed compatibility flags such as `image_detail_original` are ignored by `from_sources()`

### Legacy compatibility

Tests cover:

- alias resolution such as `telepathy` -> `Chronicle` and `collab` -> `Collab`
- deprecation notice generation for `use_legacy_landlock`
- recognition of removed keys that are still parseable for compatibility

### Typed feature config

Tests verify that `multi_agent_v2` can deserialize as:

- a plain boolean
- a structured table

They also verify that non-enable subfields alone do not implicitly enable the feature.

### Warning generation

`unstable_warning_event_only_mentions_enabled_under_development_features` confirms that the startup warning only includes features that are:

- present and `true` in the effective config table
- known in the canonical registry
- still enabled after resolution
- marked as `UnderDevelopment`

### Observed test run

Running `cargo test -p codex-features` from the workspace root succeeded with:

- 35 tests passed
- 0 failed

## Design assessment

### What works well

1. Single registry model
   - `FEATURES` is the central source of truth for keys, lifecycle stage, defaults, and some UI metadata.
   - This keeps schema generation, config parsing, warnings, and tests aligned.

2. Good compatibility posture
   - The crate does not just support legacy keys; it records and explains them.
   - This reduces migration friction while still nudging callers toward canonical names.

3. Deterministic data structures
   - `BTreeSet` and `BTreeMap` keep ordering stable for tests, warnings, and downstream presentation.

4. Clear layering
   - Resolution logic is compact and easy to follow: defaults -> source layers -> overrides -> normalization.

5. Minimal abstraction overhead
   - The implementation mostly uses straightforward data transforms instead of complex trait-heavy machinery, which is appropriate for a registry/config crate.

### Tradeoffs and limitations

1. Enum/registry duplication
   - Every feature exists in both `Feature` and `FEATURES`.
   - Missing registry entries are caught only by `unreachable!()` at runtime or by tests, not at compile time.

2. Hard-coded dependency normalization
   - Feature dependencies are embedded in `normalize_dependencies()` rather than encoded declaratively in `FeatureSpec`.
   - This is fine at current scale, but it may get harder to maintain as cross-feature rules grow.

3. Partial special-casing for removed features
   - Some removed keys are explicitly ignored in `apply_map()`, while others still flow through normal enable/disable logic.
   - The policy is compatible, but it is not fully uniform.

4. Typed config support is narrow
   - The generic `FeatureToml<T>` abstraction exists, but only one feature actually uses it.
   - Most features remain boolean-only even though some may eventually need structured settings.

5. UI text lives in the registry
   - Experimental feature descriptions and announcements are embedded next to configuration metadata.
   - This is convenient, but it ties product copy changes to code changes in the registry crate.

## Open questions

1. Should removed features be represented in the enabled set when they are default-enabled?
   - `with_defaults()` enables every `FeatureSpec` with `default_enabled = true`, including removed compatibility flags such as `Sqlite`, `Steer`, `CollaborationModes`, and `TuiAppServer`.
   - That may be intentional for compatibility, but it means the effective feature set contains flags that no longer have live product meaning.

2. Should removed-feature handling be made consistent in `apply_map()`?
   - `image_detail_original` and `tui_app_server` are silently ignored, while other removed keys still resolve to features and can be toggled.
   - A uniform policy might make the behavior easier to reason about.

3. Should `FeatureSpec` encode dependencies and alias metadata declaratively?
   - Today aliases live in `legacy.rs` and dependencies live in `normalize_dependencies()`.
   - Folding more metadata into the registry could reduce the number of places that must be updated for each feature.

4. Should this crate own richer typed-config accessors, not just deserialization?
   - `MultiAgentV2ConfigToml` lives here, but consumer-specific extraction logic currently lives in `codex-core`.
   - If more typed feature configs are added, the ownership boundary may become less clear.

5. Should telemetry or tests cover legacy-usage recording and metrics emission more directly?
   - The tests cover many functional behaviors, but there is no direct test for `emit_metrics()` and limited direct verification of alias logging behavior.

## Bottom line

`codex-features` is a registry-and-resolution crate rather than a business-logic crate. Its main value comes from centralization: one place defines feature names, defaults, lifecycle state, compatibility aliases, and enough metadata to support schema generation, startup warnings, and telemetry. The implementation is intentionally simple and mostly successful at that goal. The main long-term risk is not algorithmic complexity; it is registry drift as more feature metadata, dependencies, and migration rules accumulate in multiple nearby structures.
