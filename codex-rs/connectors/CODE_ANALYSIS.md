# codex-connectors crate analysis

## Scope

This document analyzes the `codex-connectors` crate at `/Users/bytedance/project/codex/codex-rs/connectors` and its most important integration points in higher-level crates.

Primary references:

- [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/connectors/Cargo.toml)
- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L13-L410)
- [accessible.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/accessible.rs#L8-L76)
- [filter.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/filter.rs#L5-L68)
- [merge.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/merge.rs#L8-L119)
- [metadata.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/metadata.rs#L3-L27)

Important consumers and tests:

- [chatgpt/src/connectors.rs](file:///Users/bytedance/project/codex/codex-rs/chatgpt/src/connectors.rs#L40-L185)
- [core/src/connectors.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors.rs#L90-L185)
- [core/src/connectors_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors_tests.rs#L146-L316)
- [core/src/connectors_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors_tests.rs#L1115-L1180)

## High-level role

`codex-connectors` is a small data-shaping crate for app/connector inventory. It does not own authentication, HTTP clients, plugin loading, or MCP connectivity. Instead, it provides reusable logic for:

- listing connector metadata from ChatGPT directory endpoints,
- normalizing and merging duplicate directory records,
- building accessible connector inventories from discovered tools,
- merging directory inventory with accessible/plugin-derived inventory,
- filtering out blocked or non-discoverable connectors,
- generating connector-facing metadata such as install URLs and mention-safe slugs.

The crate works on `codex_app_server_protocol::AppInfo` as its central data model, which makes it an interoperability layer between the directory fetch path and the MCP/plugin discovery path.

## Module responsibilities

### `lib.rs`

Core directory inventory logic lives in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L18-L410).

- Defines the crate-wide cache TTL as `CONNECTORS_CACHE_TTL = 1 hour`.
- Defines `AllConnectorsCacheKey`, which scopes cached results by base URL, account ID, ChatGPT user ID, and workspace status.
- Defines deserialization types `DirectoryListResponse` and `DirectoryApp` for ChatGPT directory responses.
- Exposes `cached_all_connectors()` and `list_all_connectors_with_options()`.
- Implements pagination over `/connectors/directory/list`.
- Optionally augments results with `/connectors/directory/list_workspace`.
- Filters hidden apps (`visibility == "HIDDEN"`).
- Merges duplicate `DirectoryApp` entries by connector ID.
- Normalizes names, descriptions, and install URLs before converting to `AppInfo`.

### `accessible.rs`

Accessible connector aggregation lives in [accessible.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/accessible.rs#L8-L76).

- Defines `AccessibleConnectorTool`, a lightweight per-tool input struct.
- Collapses many tool records into one `AppInfo` per connector ID.
- Prefers a real connector name over the placeholder connector ID.
- Preserves the first meaningful description.
- Unions and deduplicates plugin display names with a `BTreeSet`.
- Marks all produced connectors as `is_accessible = true`.

### `filter.rs`

Filtering rules live in [filter.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/filter.rs#L5-L68).

- `filter_disallowed_connectors()` removes blocked connector IDs and an entire blocked prefix.
- `filter_tool_suggest_discoverable_connectors()` narrows directory results to tool-suggest candidates:
  - removes blocked connectors,
  - excludes already-accessible connectors,
  - keeps only IDs present in a supplied discoverable set,
  - sorts by name then ID.
- The behavior depends on `originator_value`; first-party chat originators use a slightly different disallow list.

### `merge.rs`

Inventory reconciliation lives in [merge.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/merge.rs#L8-L119).

- `merge_connectors()` merges directory/plugin placeholder connectors with accessible connectors.
- `merge_plugin_connectors()` injects configured plugin app IDs as placeholder `AppInfo` values.
- `merge_plugin_connectors_with_accessible()` limits placeholder injection to plugin IDs that are actually accessible, then merges.
- `plugin_connector_to_app_info()` creates a minimal placeholder connector with the connector ID as its initial name.

This module is where directory truth and runtime accessibility truth are combined into the user-facing connector list.

### `metadata.rs`

Small presentation helpers live in [metadata.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/metadata.rs#L3-L27).

- `connector_display_label()` currently returns `name`.
- `connector_mention_slug()` builds a mention-safe slug from the display label.
- `connector_install_url()` re-exports install URL generation.
- `sanitize_name()` converts the slug format from hyphens to underscores.
- `sort_connectors_by_accessibility_and_name()` provides the canonical connector ordering used by merge paths.

## Public API surface

The main public entry points are:

- [AllConnectorsCacheKey::new](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L28-L41)
  - Creates the cache key for directory inventory.
- [cached_all_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L79-L95)
  - Returns cached directory-derived `Vec<AppInfo>` if the single-entry cache is still valid.
- [list_all_connectors_with_options](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L97-L137)
  - Fetches all directory pages through an injected callback, optionally includes workspace apps, merges duplicates, normalizes fields, sorts, and caches.
- [collect_accessible_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/accessible.rs#L15-L76)
  - Aggregates tool-level connector accessibility into connector-level `AppInfo`.
- [filter_disallowed_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/filter.rs#L42-L53)
  - Removes blocked connectors based on originator-specific rules.
- [filter_tool_suggest_discoverable_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/filter.rs#L5-L28)
  - Produces the install/discovery subset shown for tool suggestions.
- [merge_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/merge.rs#L8-L58)
  - Joins directory/plugin inventory with accessible inventory.
- [merge_plugin_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/merge.rs#L60-L78)
  - Adds placeholder connectors for configured plugin app IDs.
- [merge_plugin_connectors_with_accessible](file:///Users/bytedance/project/codex/codex-rs/connectors/src/merge.rs#L80-L97)
  - Same idea, but only for plugin IDs that are present in the accessible set.
- [plugin_connector_to_app_info](file:///Users/bytedance/project/codex/codex-rs/connectors/src/merge.rs#L99-L119)
  - Builds a minimal placeholder entry.

## End-to-end flow

### 1. Directory inventory flow

The directory path is implemented in [list_all_connectors_with_options](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L97-L137).

Flow:

1. Read the single-entry global cache with `cached_all_connectors()`.
2. If cache miss or forced refetch, page through `/connectors/directory/list`.
3. If the caller says the account is a workspace account, also fetch `/connectors/directory/list_workspace`.
4. Drop hidden entries.
5. Merge duplicate `DirectoryApp` records by connector ID.
6. Convert each merged item to `AppInfo`.
7. Normalize:
   - blank names become connector IDs,
   - descriptions are trimmed and empty strings become `None`,
   - missing install URLs are synthesized,
   - `is_accessible` is reset to `false`.
8. Sort by name then ID.
9. Cache the result for one hour.

Important implementation detail:

- The fetcher is injected as `FnMut(String) -> Future<Result<DirectoryListResponse>>`, so the crate remains transport-agnostic. Authentication, headers, and timeout policy are all owned by higher layers.

### 2. Accessible inventory flow

This crate does not directly talk to MCP. Instead, higher layers transform MCP tool data into `AccessibleConnectorTool` and then call [collect_accessible_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/accessible.rs#L15-L76).

In [core/src/connectors.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors.rs#L512-L529), the runtime:

- scans `ToolInfo` values from the codex apps MCP server,
- extracts `connector_id`, `connector_name`, `connector_description`, and `plugin_display_names`,
- passes those records to `codex_connectors::accessible::collect_accessible_connectors`.

The output is connector-level `AppInfo` with `is_accessible = true`.

### 3. Final merged inventory

The final list usually comes from a wrapper like [chatgpt/src/connectors.rs](file:///Users/bytedance/project/codex/codex-rs/chatgpt/src/connectors.rs#L40-L127), which:

- fetches directory connectors via `codex_connectors::list_all_connectors_with_options`,
- injects configured plugin app IDs via `merge_plugin_connectors`,
- filters disallowed connectors via `filter_disallowed_connectors`.

When accessible runtime information is also available, [core/src/connectors.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors.rs#L166-L185) calls:

- `merge_connectors(connectors, accessible_connectors)`,
- then `filter_disallowed_connectors(...)`.

This makes the final UI-visible list reflect both catalog knowledge and current runtime accessibility.

### 4. Tool-suggest discovery flow

The discoverability filter is used by [core/src/connectors.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors.rs#L115-L139).

That path:

- fetches directory connectors suitable for suggestion,
- computes a set of allowed discoverable connector IDs from config/plugin state,
- removes any connector already accessible to the current user,
- removes blocked connectors,
- returns the remaining uninstalled but suggestible connectors.

## Merge and normalization rules

### Duplicate directory entries

[merge_directory_app](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L212-L350) applies a conservative fill-missing-fields policy:

- `name` only replaces an empty existing name.
- `description` replaces when the incoming description is present and non-blank.
- `logo_url`, `logo_url_dark`, and `distribution_channel` fill only missing values.
- `branding` and `app_metadata` are merged field-by-field, mostly only filling `None` fields.
- `is_discoverable_app` is merged with logical OR.
- `labels` fill only when missing.

This shows the intent: duplicate records from different directory sources are treated as partial views of the same connector, not as conflicting authorities.

### Placeholder replacement

[plugin_connector_to_app_info](file:///Users/bytedance/project/codex/codex-rs/connectors/src/merge.rs#L99-L119) intentionally uses the connector ID as the placeholder name. Later, [merge_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/merge.rs#L20-L58) replaces that placeholder name with better metadata from the accessible or directory side when available.

### Install URL generation

Install URLs are generated by [connector_install_url](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L374-L377) and the helper [connector_name_slug](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L379-L394):

- non-alphanumeric characters become `-`,
- ASCII letters are lowercased,
- leading and trailing hyphens are trimmed,
- empty slugs fall back to `"app"`.

Example:

```rust
let url = "https://chatgpt.com/apps/google-calendar/calendar";
```

That URL is synthesized even for placeholder connectors, so callers always have a stable install link shape.

## Dependencies

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/connectors/Cargo.toml#L10-L18):

- `anyhow`
  - Error propagation for async fetch paths.
- `codex-app-server-protocol`
  - Provides `AppInfo`, `AppMetadata`, and `AppBranding`, the crate's core data model.
- `serde` with `derive`
  - Deserializes directory API responses.
- `urlencoding`
  - Encodes pagination tokens in directory list requests.

Dev dependencies:

- `pretty_assertions`
  - Improves failure output for structural equality assertions.
- `tokio` with `macros` and `rt-multi-thread`
  - Supports async unit tests.

Notably absent:

- No HTTP client dependency.
- No auth dependency.
- No config dependency.
- No tracing/logging dependency.

That keeps the crate reusable and easy to test.

## Testing coverage

### In-crate tests

Tests in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L412-L616) cover the most important directory behaviors:

- shared cache reuse across repeated requests,
- merge-and-normalize behavior across directory and workspace sources,
- hidden app exclusion,
- pagination over all pages,
- URL encoding of page tokens.

Representative assertions:

- cached fetch avoids a second directory request,
- workspace results enrich existing directory entries,
- hidden workspace entries do not surface,
- blank names are normalized to connector IDs,
- generated install URLs match the expected slug format.

### Cross-crate tests that exercise this crate

Important usage-facing tests live in [core/src/connectors_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors_tests.rs#L146-L316) and [core/src/connectors_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors_tests.rs#L1115-L1180).

They verify:

- placeholder plugin names are replaced with canonical accessible names,
- plugin display names are unioned and deduplicated,
- accessible descriptions are preserved,
- accessible connector cache refresh writes the latest installed apps,
- tool-suggest discoverability keeps only plugin-backed uninstalled apps,
- accessible apps are excluded from suggestion even when disabled.

### Gaps

The current tests are solid for core behaviors, but there are some visible gaps:

- `filter.rs` has no local unit tests; behavior is only covered indirectly in `core`.
- `metadata.rs` has no direct tests.
- `merge_directory_app()` has good coverage through aggregate tests, but not exhaustive field-by-field tests for every `AppMetadata` and `AppBranding` field.
- There is no explicit TTL expiry test for the one-hour cache.
- There is no test documenting behavior when `list_workspace` fails, even though that path intentionally swallows errors.

## Design characteristics

### Strengths

- Transport-agnostic fetch boundary keeps the crate isolated from auth and networking concerns.
- `AppInfo` as the shared model makes this crate easy to compose with `chatgpt`, `core`, and `app-server`.
- Merge policies are conservative and predictable.
- Sorting helpers give stable output ordering.
- Small module split is clean: directory fetch, accessibility aggregation, filtering, merge, metadata.

### Trade-offs

- The cache is process-global and single-entry, not keyed in a map.
- Filtering policy is hardcoded in source, which is simple but operationally brittle.
- Install URL generation is opinionated and host-specific.
- `AppInfo` is used as both canonical metadata and placeholder state, which is convenient but slightly muddy semantically.

## Open questions

1. Why is the directory cache a single `Option<CachedAllConnectors>` instead of a map keyed by `AllConnectorsCacheKey`?
   - Today, requests for different accounts or base URLs evict each other even though the key type suggests multi-tenant caching.

2. Should synthesized install URLs use `chatgpt_base_url` instead of the hardcoded `https://chatgpt.com` host?
   - The current implementation may be wrong for non-default environments or region-specific deployments.

3. Is swallowing all workspace-directory errors in [list_workspace_connectors](file:///Users/bytedance/project/codex/codex-rs/connectors/src/lib.rs#L183-L198) intentional long-term behavior?
   - It improves resilience, but it also hides configuration and auth problems.

4. Should disallowed connector IDs live in config or server-provided policy instead of source code constants in [filter.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/filter.rs#L30-L40)?
   - Hardcoded lists can drift as connector inventory changes.

5. Should directory merging have a stronger notion of source precedence?
   - Current behavior is mostly "fill missing fields", which is safe, but it does not resolve conflicting non-empty values in a principled way.

6. Should `connector_name_slug()` preserve repeated separators more carefully or normalize Unicode more explicitly?
   - The current ASCII-only approach is simple, but it is not a full slugification strategy.

7. Should `is_workspace_account` remain both inside `AllConnectorsCacheKey` and as a separate `list_all_connectors_with_options()` parameter?
   - The duplication is harmless, but it suggests API overlap.

## Bottom line

`codex-connectors` is a focused normalization and reconciliation crate. Its main value is not fetching data by itself, but turning several incomplete connector inventories into a stable, user-facing `Vec<AppInfo>`. The design is intentionally lightweight and composable. The main risks are operational rather than algorithmic: the single-entry global cache, hardcoded filtering policy, host-specific install URL synthesis, and silent workspace fetch fallback.
