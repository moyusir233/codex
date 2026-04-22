# codex-models-manager: Code Analysis

## Overview

`codex-models-manager` is the crate that owns Codex's model catalog lifecycle. It starts from a bundled catalog (`models.json`), optionally refreshes that catalog from a remote `/models` endpoint, caches successful refreshes on disk, derives UI-facing `ModelPreset` values, and resolves detailed `ModelInfo` for a selected slug.

At a high level, the crate does six things:

1. Ships a bundled fallback model catalog for offline startup and deterministic defaults.
2. Refreshes that catalog from a provider-backed `/models` endpoint when allowed by auth mode and refresh strategy.
3. Persists successful fetches to `models_cache.json` with a TTL and client-version guard.
4. Resolves the default model and the current picker-visible presets.
5. Produces `ModelInfo` by combining catalog data with local config overrides and fallback heuristics.
6. Generates collaboration-mode presets and instructions for the UI/runtime.

This crate is mostly orchestration code. It delegates transport, auth, provider creation, protocol types, telemetry, and collaboration-mode template content to sibling workspace crates.

## Crate Layout

- `src/lib.rs`: public module exports plus helpers for reading the bundled catalog and computing the whole client version.
- `src/manager.rs`: main orchestration logic in `ModelsManager`; remote refresh, cache application, preset derivation, telemetry, and default-model selection.
- `src/cache.rs`: JSON cache persistence with freshness/version validation.
- `src/model_info.rs`: fallback metadata generation and config-driven overrides for `ModelInfo`.
- `src/collaboration_mode_presets.rs`: static Plan/Default collaboration mode presets and instruction templating.
- `src/model_presets.rs`: legacy compatibility constants for removed migration-prompt config keys.
- `models.json`: bundled model catalog compiled into the crate.
- `prompt.md`: default base instructions used by fallback model metadata.

## Responsibilities by Module

### `lib.rs`

- Re-exports auth and provider-facing types so downstream users can depend on this crate as the integration surface.
- Exposes `bundled_models_response()` to parse the embedded `models.json`.
- Exposes `client_version_to_whole()` to normalize the build version to `major.minor.patch`; that version is used as a cache compatibility key.

## `manager.rs`

`ModelsManager` is the core type:

```rust
pub struct ModelsManager {
    remote_models: RwLock<Vec<ModelInfo>>,
    catalog_mode: CatalogMode,
    collaboration_modes_config: CollaborationModesConfig,
    etag: RwLock<Option<String>>,
    cache_manager: ModelsCacheManager,
    provider: SharedModelProvider,
}
```

Its responsibilities are:

- initialize state from either a caller-supplied catalog or the bundled `models.json`;
- coordinate refresh behavior through `RefreshStrategy`;
- decide whether remote refresh is allowed for the current auth/provider setup;
- fetch `/models` using provider-specific auth plumbing;
- merge fetched models into the bundled baseline;
- persist and reload cached results;
- derive `ModelPreset` values from the active catalog;
- resolve `ModelInfo` with longest-prefix and limited namespace-suffix matching;
- expose collaboration-mode presets.

`CatalogMode` is important:

- `Default`: the manager starts from bundled models and may update them from cache or network.
- `Custom`: the caller supplied a `ModelsResponse`, and refresh is disabled for the life of the manager.

### `cache.rs`

`ModelsCacheManager` handles the on-disk cache. The cache stores:

- `fetched_at`: used for TTL enforcement;
- `etag`: used by `refresh_if_new_etag`;
- `client_version`: used to invalidate cache when the client version changes;
- `models`: the fetched `Vec<ModelInfo>`.

Key behaviors:

- `load_fresh()` rejects missing, malformed, version-mismatched, or stale entries.
- `persist_cache()` writes a pretty-printed JSON snapshot.
- `renew_cache_ttl()` extends freshness without refetching when the ETag is unchanged.

### `model_info.rs`

This module has two jobs:

- produce fallback metadata for unknown models with `model_info_from_slug()`;
- apply runtime config overrides with `with_config_overrides()`.

The fallback path uses `prompt.md` as `BASE_INSTRUCTIONS` and synthesizes a minimal but usable `ModelInfo` with conservative defaults. It also marks `used_fallback_model_metadata = true`, which gives downstream consumers a signal that the metadata was synthesized rather than catalog-backed.

Config overrides can:

- force-enable reasoning summaries;
- clamp `context_window` by `max_context_window`;
- set `auto_compact_token_limit`;
- rewrite truncation policy from a token limit;
- replace base instructions or disable personality-derived messages.

### `collaboration_mode_presets.rs`

This module is independent from model refresh. It returns built-in collaboration modes:

- Plan mode: fixed reasoning effort and fixed developer instructions from `codex-collaboration-mode-templates`.
- Default mode: instructions rendered from a parsed template with feature-flag-driven substitutions.

`CollaborationModesConfig` currently has a single flag:

- `default_mode_request_user_input`: controls whether Default mode documents `request_user_input` as available and how it instructs the agent to ask questions.

### `model_presets.rs`

This module does not build presets. It only preserves two legacy config keys for backward compatibility with older migration prompts.

## Public API Surface

The externally meaningful API is centered on `ModelsManager` plus a few crate-level helpers.

### Constructors

- `ModelsManager::new(codex_home, auth_manager, model_catalog, collaboration_modes_config)`
  - Creates an OpenAI provider by default.
  - Uses `model_catalog` as authoritative when provided.
- `ModelsManager::new_with_provider(...)`
  - Same manager behavior, but injects an explicit `ModelProviderInfo`.
  - This is the real constructor used by `new()`.

### Catalog and preset access

- `list_models(refresh_strategy) -> Vec<ModelPreset>`
  - Refreshes if needed, logs refresh errors, and returns picker-ready presets.
- `raw_model_catalog(refresh_strategy) -> ModelsResponse`
  - Same refresh behavior, but returns raw `ModelInfo` values instead of derived presets.
- `try_list_models() -> Result<Vec<ModelPreset>, TryLockError>`
  - Avoids awaiting on the internal `RwLock`; useful for opportunistic reads.

### Selection and lookup

- `get_default_model(model, refresh_strategy) -> String`
  - Returns the user-specified model when present.
  - Otherwise refreshes, derives presets, and returns the first default or first available model.
- `get_model_info(model, config) -> ModelInfo`
  - Looks for the best catalog candidate, applies fallback behavior if unknown, then applies config overrides.

### Collaboration modes

- `list_collaboration_modes() -> Vec<CollaborationModeMask>`
- `list_collaboration_modes_for_config(config) -> Vec<CollaborationModeMask>`

### Cache/refresh helpers

- `refresh_if_new_etag(etag)`
  - Renews TTL when the supplied ETag matches the in-memory ETag.
  - Otherwise forces an online refresh.

### Crate-level helpers

- `bundled_models_response() -> Result<ModelsResponse, serde_json::Error>`
- `client_version_to_whole() -> String`

## Core Runtime Flow

### 1. Construction

When a manager is created:

1. `new_with_provider()` builds a `SharedModelProvider` with `create_model_provider(...)`.
2. It points the cache at `<codex_home>/models_cache.json`.
3. It chooses `CatalogMode::Custom` when a catalog is injected; otherwise `CatalogMode::Default`.
4. It seeds `remote_models` from the injected catalog or from bundled `models.json`.

Important implication: even the field named `remote_models` initially contains bundled models unless a custom catalog is supplied.

### 2. Refresh decision

Most read APIs call `refresh_available_models(refresh_strategy)` first.

The refresh gate works like this:

1. If `catalog_mode == Custom`, do nothing.
2. Read auth mode from the provider's `AuthManager`.
3. If auth mode is not `Chatgpt` and the provider does not advertise command auth, do not hit the network.
4. Otherwise apply the selected strategy:
   - `Offline`: only try cache;
   - `OnlineIfUncached`: try cache first, then fetch;
   - `Online`: always fetch.

This means remote discovery is intentionally narrow. For many provider/auth combinations, the crate behaves as a bundled-catalog reader instead of a live catalog client.

### 3. Remote fetch

`fetch_and_update_models()` performs the online path:

1. Start an OTEL timer.
2. Resolve provider auth state and active auth mode.
3. Ask the provider for API endpoint and auth provider.
4. If the provider uses Codex login auth and the user is on ChatGPT auth, convert that auth into an authorization-header provider, including FedRAMP routing when needed.
5. Build `ModelsRequestTelemetry` from auth mode, attached header information, and auth-env telemetry.
6. Create `ModelsClient` with `ReqwestTransport`.
7. Call `list_models()` with a 5-second timeout and an empty `HeaderMap`.
8. On success:
   - apply the returned models to the active catalog,
   - store the returned ETag in memory,
   - persist the fetched models to the cache with the client version.

Errors are mapped into `codex_protocol::error::CodexErr` and returned to the caller. Public reader methods usually swallow those errors after logging them.

### 4. Catalog application

`apply_remote_models()` does not replace the catalog wholesale with the fetched list. Instead, it:

1. reloads the bundled catalog;
2. upserts each fetched model by matching on `slug`;
3. stores the merged result in `remote_models`.

This design preserves bundled-only models while allowing remote entries to replace or extend them. It also means a later remote refresh can remove previously fetched dynamic models, because each merge starts from the bundled baseline rather than from the previous merged state.

### 5. Cache path

`try_load_cache()`:

1. computes the whole client version;
2. calls `load_fresh(expected_version)`;
3. if a valid entry exists, copies its ETag into memory and applies its models through the same merge path used for network responses.

Cache freshness depends on TTL and exact `major.minor.patch` version equality.

### 6. Preset derivation

`build_available_models()` converts `Vec<ModelInfo>` into `Vec<ModelPreset>`:

1. sort by ascending `priority`;
2. convert `ModelInfo -> ModelPreset`;
3. filter by auth mode using `ModelPreset::filter_by_auth(...)`;
4. call `ModelPreset::mark_default_by_picker_visibility(...)`.

Notably, hidden models can still remain in the returned vector; default selection is re-evaluated after visibility filtering.

### 7. Model info resolution

`get_model_info()` uses `construct_model_info_from_candidates()`:

1. try longest-prefix matching against current catalog slugs;
2. if that fails, allow one restricted fallback for `namespace/model` inputs by stripping exactly one ASCII-word namespace segment;
3. if still not found, synthesize fallback metadata from the slug;
4. apply config overrides.

If a catalog match is found, the returned `ModelInfo` keeps the matched metadata but rewrites `slug` to the exact requested model string. This allows suffixed or namespaced variants to inherit capabilities from a canonical base model.

## Data and State Model

### In-memory state

- `remote_models: RwLock<Vec<ModelInfo>>`
  - shared mutable catalog snapshot;
- `etag: RwLock<Option<String>>`
  - last seen ETag;
- `provider: SharedModelProvider`
  - encapsulates provider metadata, auth, and API endpoint resolution.

The crate uses `tokio::sync::RwLock`, which fits its read-mostly behavior. `try_list_models()` specifically exposes a non-blocking path when the read lock cannot be obtained.

### Persistent state

The only persistent state is `models_cache.json` under `codex_home`. The cache is not critical for correctness; failures during read/write only log errors and fall back to bundled/in-memory state.

## Key Dependencies and Why They Matter

### Auth, provider, and transport

- `codex-login`
  - owns `AuthManager`, `CodexAuth`, and auth-env telemetry collection.
- `codex-model-provider` / `codex-model-provider-info`
  - abstract provider metadata, endpoint selection, and auth-provider creation.
- `codex-api`
  - provides `ModelsClient`, `ReqwestTransport`, request telemetry hooks, and API error mapping.

These crates keep `codex-models-manager` focused on orchestration rather than low-level HTTP or auth mechanics.

### Shared protocol types

- `codex-protocol`
  - supplies `ModelInfo`, `ModelPreset`, `ModelsResponse`, truncation types, collaboration-mode types, and shared error/result types.
- `codex-app-server-protocol`
  - supplies `AuthMode`.

This crate does not define its own model schema; it operates on shared protocol-layer types.

### Instructions and templates

- `codex-collaboration-mode-templates`
  - source of default/plan collaboration-mode instruction templates.
- `codex-utils-template`
  - renders the Default mode instructions.
- `prompt.md`
  - local base prompt for fallback model metadata.

### Telemetry and logging

- `codex-otel`
  - duration timers and event targets.
- `codex-feedback`
  - feedback tag emission for auth-related request diagnostics.
- `codex-response-debug-context`
  - extracts request IDs and auth error details from failed responses.
- `tracing`
  - primary logging/instrumentation surface.

## Testing Coverage

The crate has 24 tests spread across four files. Coverage is strong around orchestration rules and edge cases.

### `manager_tests.rs`

This is the main behavior suite. It covers:

- fallback vs catalog-backed `ModelInfo`;
- custom catalog injection;
- namespaced suffix matching rules;
- priority ordering of presets;
- provider-command auth token usage;
- cache hit behavior;
- stale-cache refetch;
- client-version mismatch refetch;
- removal of prior remote-only models on refresh;
- network suppression without qualifying auth;
- feedback-tag telemetry emission;
- default selection after visibility filtering;
- bundled catalog serde round-trip.

The tests use `wiremock`, temp directories, and helper auth scripts to exercise the real async/cache/HTTP boundaries instead of only unit-level pure functions.

### `model_info_tests.rs`

Covers override semantics:

- `model_supports_reasoning_summaries`;
- context window clamping;
- no-op behavior when overrides should not reduce capability.

### `model_info_overrides_tests.rs`

Validates that tool-output overrides rewrite truncation policy differently for byte-based and token-based models.

### `collaboration_mode_presets_tests.rs`

Checks template rendering and feature-flag-driven instructional differences for Default mode.

## Design Observations

### Strong points

- Clear fallback chain: bundled catalog -> cache -> network, with graceful degradation on failure.
- Good isolation of concerns: cache, catalog lookup, and collaboration-mode templating are kept separate.
- Conservative error handling for user-facing reads: failures do not prevent the crate from returning some model data.
- Good test coverage on subtle behaviors like suffix matching, TTL invalidation, and telemetry tag extraction.
- Versioned cache invalidation is simple and easy to reason about.

### Intentional design choices

- The crate favors availability over strict freshness. Public getters log refresh failures and continue with current state.
- The active catalog is always rooted in bundled models, even after remote refreshes.
- Model matching is prefix-based rather than exact-match-only, which supports families like experimental suffixed variants.
- Collaboration-mode presets are static and not derived from the model catalog.

### Tradeoffs

- `apply_remote_models()` preserves bundled entries even when the server does not return them. That is robust for offline/bootstrap behavior, but it means the server is not authoritative over the entire catalog.
- `list_models()`, `raw_model_catalog()`, and `get_default_model()` suppress refresh errors. That improves resilience but hides failure details from callers unless they inspect logs.
- Cache validity only checks TTL and client version, not provider identity or auth context.

## Open Questions

1. Should cache validity include provider identity?
   - `models_cache.json` appears to be keyed only by location, TTL, and client version. If the same `codex_home` is reused across providers or provider configuration changes, the cache could be semantically stale even when structurally fresh.

2. Should the remote catalog be authoritative?
   - Current behavior merges fetched entries into bundled entries. If the backend intends `/models` to be the source of truth, this merge strategy may keep obsolete bundled entries visible longer than intended.

3. Why is remote refresh allowed only for ChatGPT auth or command-auth providers?
   - The gate in `refresh_available_models()` is stricter than "provider can produce auth". It is worth confirming whether env-key or bearer-token providers intentionally cannot use `/models`.

4. Should public read APIs surface refresh errors?
   - Swallowing refresh errors keeps the product usable, but it removes observability from API consumers. A richer return type could preserve resilience while exposing freshness state.

5. Should `ModelsManager` own provider construction?
   - There is already a TODO in `new_with_provider()` suggesting that ownership may be backwards and the provider layer may be the better place to own or vend the manager.

6. Is prefix matching too permissive for future catalogs?
   - Longest-prefix matching is useful today, but it can become ambiguous if model slugs become denser or if unrelated models share prefixes unintentionally.

## Bottom Line

This crate is a model-catalog coordinator. It is not a pure data crate and not a low-level API crate. Its core value is combining several concerns into one stable interface:

- model discovery and fallback,
- cache-aware refresh,
- auth-sensitive preset filtering,
- config-aware `ModelInfo` resolution,
- collaboration-mode preset generation.

The implementation is pragmatic and resilient, with good test coverage and a clear bias toward "always return something useful." The main architectural questions are about authority boundaries: who owns provider/manager construction, when remote state should replace bundled state, and how cache identity should be scoped.
