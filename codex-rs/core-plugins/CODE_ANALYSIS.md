# codex-core-plugins Code Analysis

## Overview

`codex-core-plugins` is the workspace crate that centralizes plugin and marketplace mechanics for the wider Codex runtime. It does not own the top-level product workflow itself; instead, it provides the lower-level building blocks that other crates call to:

- discover marketplaces and plugin manifests,
- normalize local and Git-backed plugin sources,
- install plugins into a versioned local cache,
- load plugin skills, MCP servers, and app connectors from installed plugins,
- refresh cached plugins when curated or non-curated marketplaces change,
- upgrade configured Git marketplaces atomically, and
- call remote ChatGPT plugin APIs for list/featured/enable/uninstall operations.

The crate’s public surface is intentionally split by concern in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/lib.rs#L1-L7): `loader`, `manifest`, `marketplace`, `marketplace_upgrade`, `remote`, `store`, and `toggles`.

## Crate Layout

- **Entry point**: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/lib.rs#L1-L7) simply re-exports the functional modules.
- **Cargo dependencies**: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/core-plugins/Cargo.toml#L1-L40) shows that this crate sits between configuration (`codex-config`), plugin model types (`codex-plugin`), skill loading (`codex-core-skills`), protocol/app types, login/auth, git helpers, and filesystem/path utilities.
- **Bazel packaging**: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/core-plugins/BUILD.bazel#L1-L15) exposes the crate as `codex_core_plugins` and includes all non-manifest data as compile data.

## Module Responsibilities

### `manifest`

The manifest layer parses `plugin.json`, validates path fields, and resolves plugin interface metadata.

- Core types live in [manifest.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L32-L64): `PluginManifest`, `PluginManifestPaths`, and `PluginManifestInterface`.
- `load_plugin_manifest()` in [manifest.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L117-L234) reads a discoverable plugin manifest, trims the plugin version, falls back to the directory name when `name` is blank, and resolves optional interface data.
- `resolve_manifest_path()` in [manifest.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L340-L381) enforces a strict `./relative/path` rule and rejects `..` traversal or other non-normal components.
- `resolve_default_prompts()` in [manifest.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L244-L299) normalizes `interface.defaultPrompt`, allowing either a single string or a bounded list of strings, capped at 3 prompts and 128 characters each.
- Interface asset paths such as `composerIcon`, `logo`, and `screenshots` are turned into absolute paths only when they obey the same manifest path rules in [manifest.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L236-L242).

Concrete responsibility: this module is the trust boundary for plugin-owned metadata. Other modules assume its output is already path-normalized and safe to join back into the filesystem.

### `marketplace`

The marketplace layer discovers marketplace manifests, resolves plugin sources, and exposes installability metadata.

- Public types are defined in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L25-L162), including `Marketplace`, `MarketplacePlugin`, `MarketplacePluginSource`, `MarketplacePluginPolicy`, and `ResolvedMarketplacePlugin`.
- Supported marketplace manifest layouts are hard-coded in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L20-L23): `.agents/plugins/marketplace.json` and `.claude-plugin/marketplace.json`.
- `find_marketplace_plugin()` and `find_installable_marketplace_plugin()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L170-L218) locate a single plugin entry and optionally enforce install policy plus product restrictions.
- `list_marketplaces()` / `list_marketplaces_with_home()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L220-L332) discover marketplace manifests from the user home and additional roots, dedupe by path, and return both successful marketplace loads and soft errors.
- `resolve_plugin_source()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L455-L499) supports:
  - local relative paths,
  - Git URLs,
  - Git subdirectory sources,
  - GitHub shorthand like `owner/repo`.
- `plugin_interface_with_marketplace_category()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L666-L677) merges manifest interface metadata with marketplace taxonomy, letting marketplace category override plugin category.

Concrete responsibility: this module transforms marketplace JSON into normalized, consumer-ready plugin entries while staying tolerant of bad individual plugins.

### `store`

The store layer owns the local plugin cache under `plugins/cache`.

- Cache constants and core types are in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L14-L28).
- `PluginStore::try_new()` in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L35-L40) requires an absolute Codex home and resolves `<codex_home>/plugins/cache`.
- `install()` and `install_with_version()` in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L88-L130) validate the source directory, enforce that the manifest name matches the marketplace plugin name, compute the version, and atomically replace the cached plugin root.
- `active_plugin_version()` in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L56-L77) prefers the sentinel version `local` over other versions; otherwise it picks the lexicographically last valid version directory.
- `replace_plugin_root_atomically()` in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L246-L311) stages the copied tree in a temp dir, renames the previous root into a backup, then swaps in the staged tree with rollback on failure.
- `plugin_version_for_source()` in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L156-L161) reads the manifest `version` field and falls back to `"local"` when absent.

Concrete responsibility: this module is the filesystem-backed installation layer. It does not discover plugins or interpret marketplace policy; it only validates and materializes versioned plugin trees.

### `loader`

The loader layer is the main integration point between config, store, manifests, skill loading, MCP parsing, app loading, cache refresh, and telemetry.

- `load_plugins_from_layer_stack()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L102-L140) is the top-level plugin load entry used by higher layers. It:
  - reads configured plugins from the user config layer,
  - sorts them for deterministic processing,
  - loads each plugin from the active cache,
  - tracks duplicate MCP server names across plugins and warns on collisions,
  - returns `PluginLoadOutcome<McpServerConfig>`.
- `load_plugin()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L436-L534) handles the per-plugin workflow:
  - parse configured key into `PluginId`,
  - locate active installed root,
  - fail early if disabled, uninstalled, invalid, or missing a manifest,
  - resolve skills, MCP servers, apps, display name, and description,
  - return a `LoadedPlugin` with any accumulated error.
- `load_plugin_skills()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L553-L581) converts plugin skill roots into `SkillRoot`s, delegates to `codex_core_skills::loader::load_skills_from_roots`, filters by `Product`, and computes disabled skill paths from config rules.
- `load_plugin_apps()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L628-L699) reads `.app.json` or a manifest-declared app config file and produces sorted, deduped `AppConnectorId`s.
- `load_plugin_mcp_servers()` and helpers in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L740-L866) read `.mcp.json` or a manifest-declared MCP config, support both wrapped and top-level map shapes, normalize relative `cwd`, ignore plugin-provided OAuth `callbackPort`, and deserialize into `McpServerConfig`.
- `plugin_telemetry_metadata_from_root()` and `installed_plugin_telemetry_metadata()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L701-L772) summarize plugin capabilities for telemetry.
- Cache refresh helpers split curated and non-curated logic:
  - curated: [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L142-L209),
  - non-curated: [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L211-L320).
- `materialize_marketplace_plugin_source()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L879-L932) turns a `MarketplacePluginSource` into a concrete local directory, cloning Git sources into a staging tempdir when needed.

Concrete responsibility: this is the operational heart of the crate. It bridges abstract plugin configuration to concrete runtime-ready plugin data.

### `marketplace_upgrade`

This module upgrades configured Git marketplaces into a managed install area under `.tmp/marketplaces`.

- `configured_git_marketplace_names()` and `upgrade_configured_git_marketplaces()` live in [marketplace_upgrade.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade.rs#L55-L102).
- `configured_git_marketplaces()` in [marketplace_upgrade.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade.rs#L108-L165) reads `marketplaces` from the user config, filters only `source_type = git`, and normalizes them into an internal `ConfiguredGitMarketplace`.
- `upgrade_configured_git_marketplace()` in [marketplace_upgrade.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade.rs#L167-L244) checks the remote revision, skips work when metadata proves nothing changed, clones into a staging directory, validates the marketplace manifest, writes activation metadata, updates `config.toml`, and atomically swaps the installed root into place.
- The atomic activation and metadata helpers are isolated in [activation.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade/activation.rs#L10-L167).
- Git operations with timeout handling are isolated in [git.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade/git.rs#L7-L198).

Concrete responsibility: manage long-lived local checkouts of configured Git marketplaces safely, with rollback if activation or config update fails.

### `remote`

This module talks to the remote ChatGPT plugin service.

- Service config and result/error types are in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L13-L121).
- `fetch_remote_plugin_status()` in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L123-L165) fetches `/plugins/list` and requires ChatGPT auth.
- `fetch_remote_featured_plugin_ids()` in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L167-L210) fetches `/plugins/featured`, optionally sends ChatGPT auth, and tags the request with the current product platform.
- `enable_remote_plugin()` and `uninstall_remote_plugin()` in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L212-L298) post to `/plugins/{id}/{action}` and validate that the response echoes the expected plugin id and enabled state.
- `remote_plugin_mutation_url()` in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L300-L317) safely appends path segments via `url::Url` instead of string concatenation.

Concrete responsibility: isolate the HTTP contract and auth checks for remote plugin management so the rest of the codebase can treat it as a clean API.

### `toggles`

This is a small helper module used during config editing.

- `collect_plugin_enabled_candidates()` in [toggles.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/toggles.rs#L4-L43) scans config write edits and extracts candidate `plugins.<id>.enabled` changes from direct field writes, table writes, or full `[plugins]` table writes.

Concrete responsibility: infer which plugin enablement flags may have changed so higher layers can selectively reload telemetry or plugin state.

## Main Public APIs

The public API is fairly cohesive by boundary:

- **Manifest boundary**
  - `load_plugin_manifest()` in [manifest.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L117-L234)
- **Marketplace boundary**
  - `find_marketplace_plugin()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L170-L193)
  - `find_installable_marketplace_plugin()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L195-L218)
  - `list_marketplaces()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L220-L224)
  - `load_marketplace()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L271-L305)
  - `validate_marketplace_root()` in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L226-L235)
- **Store/cache boundary**
  - `PluginStore` methods in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L29-L135)
  - `plugin_version_for_source()` in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L156-L161)
- **Load/runtime boundary**
  - `load_plugins_from_layer_stack()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L102-L140)
  - `load_plugin_skills()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L553-L581)
  - `load_plugin_apps()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L628-L699)
  - `load_plugin_mcp_servers()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L740-L754)
  - `materialize_marketplace_plugin_source()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L879-L932)
- **Cache refresh / upgrade boundary**
  - `refresh_curated_plugin_cache()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L142-L209)
  - `refresh_non_curated_plugin_cache()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L211-L220)
  - `refresh_non_curated_plugin_cache_force_reinstall()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L222-L231)
  - `upgrade_configured_git_marketplaces()` in [marketplace_upgrade.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade.rs#L64-L102)
- **Remote service boundary**
  - `fetch_remote_plugin_status()` in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L123-L165)
  - `fetch_remote_featured_plugin_ids()` in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L167-L210)
  - `enable_remote_plugin()` and `uninstall_remote_plugin()` in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L212-L298)

## End-to-End Flow

### 1. Runtime plugin loading

The steady-state runtime flow is:

1. Higher layers call `load_plugins_from_layer_stack()`, typically from the plugin manager in [manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/manager.rs#L411-L464).
2. The loader reads configured plugin entries from the user config only, not every layer, via [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L322-L344).
3. Each configured key is parsed into a `PluginId`; the loader resolves the active installed root from `PluginStore`.
4. The manifest is loaded and path-normalized.
5. Skills are discovered from the default `skills/` directory plus any manifest override, then filtered by product and local disable rules.
6. MCP server JSON is loaded and normalized into `McpServerConfig`.
7. App connector IDs are loaded from `.app.json` or a manifest-declared path.
8. A `LoadedPlugin` is produced even on partial failure, with the failure stored in `error` rather than crashing the whole load.

This is a deliberately resilient load path: one bad plugin should not prevent other plugins from loading.

### 2. Marketplace-based installation flow

For marketplace-driven installs, the flow is:

1. A caller lists marketplaces or resolves one installable plugin using [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L170-L305).
2. The plugin source becomes either:
   - a local path, or
   - a Git source that may optionally point at a subdirectory and a specific ref/sha.
3. `materialize_marketplace_plugin_source()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L879-L932) turns that source into a local directory.
4. `PluginStore::install()` or `install_with_version()` copies it into the versioned cache tree.
5. Future runtime loads consume the cached copy rather than the original source.

The cache acts as a stable runtime snapshot separate from marketplace checkout layout.

### 3. Curated cache refresh

Curated refresh is opinionated around a local curated marketplace snapshot:

1. `refresh_curated_plugin_cache()` reads a curated marketplace file at `<codex_home>/.tmp/plugins/.agents/plugins/marketplace.json` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L147-L156).
2. It only accepts local plugin sources from that curated marketplace and skips remote ones.
3. It computes configured curated plugin IDs from `config.toml`.
4. For each configured curated plugin, it reinstalls only when the currently active cached version differs from the supplied curated revision.

The manager starts this after syncing the curated repo in [manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/manager.rs#L1358-L1397).

### 4. Non-curated cache refresh

Non-curated refresh is broader:

1. The loader reads configured plugin IDs from user config.
2. It discovers marketplace manifests from supplied roots via `list_marketplaces()`.
3. It skips the `openai-curated` marketplace.
4. For each matching configured plugin, it materializes the source, computes the plugin version from the manifest, and reinstalls it if:
   - the version changed, or
   - the caller requested forced reinstall.

The manager schedules and dedupes these refreshes asynchronously in [manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/manager.rs#L1284-L1457).

### 5. Git marketplace upgrade flow

Configured Git marketplace upgrades follow a stronger activation protocol:

1. Read Git marketplaces from user config.
2. Resolve the remote revision with `git ls-remote`.
3. Skip the upgrade if the destination already exists, the last known revision matches, and on-disk activation metadata still matches.
4. Clone into a temporary staging dir, optionally sparse-checking out paths.
5. Validate that the staged root contains a supported marketplace manifest and that its declared marketplace name matches config.
6. Write install metadata.
7. Atomically swap the staged root into `.tmp/marketplaces/<name>`.
8. Update user config with the new revision and timestamp; rollback the root if that update fails.

This flow is implemented across [marketplace_upgrade.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade.rs#L167-L244), [activation.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade/activation.rs#L57-L149), and [git.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade/git.rs#L42-L128).

## Key Dependencies and Why They Exist

- `codex-config`: reads user plugin and marketplace config, plus records marketplace upgrade metadata.
- `codex-plugin`: provides canonical `PluginId`, `LoadedPlugin`, `PluginLoadOutcome`, telemetry types, and segment validation.
- `codex-core-skills`: loads skill definitions and applies product / disable rules.
- `codex-app-server-protocol`: provides install/auth policy enums used when projecting marketplace metadata into app-server-facing types.
- `codex-login` + `reqwest` + `url`: power authenticated remote plugin service calls.
- `codex-git-utils`: discovers repository roots during marketplace discovery.
- `codex-utils-absolute-path`: standardizes absolute path handling and prevents accidental relative path leakage.
- `codex-utils-plugins`: discovers plugin manifest locations.
- `tokio::fs`: async file reads for skill/app/MCP loading.
- `tempfile`: staging directories for safe plugin installation, source materialization, and marketplace upgrades.
- `thiserror`: structured public error enums.
- `tracing`: warns on malformed manifests, duplicate entries, and partial failures without hard-failing the whole load path.

## Design Characteristics

### Strengths

- **Strict path hygiene**: both manifests and marketplaces require `./...` relative paths and explicitly reject traversal in [manifest.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L340-L381) and [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L501-L557).
- **Atomic activation patterns**: both plugin installs and marketplace upgrades stage work in tempdirs, then rename into place with rollback support in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L246-L311) and [activation.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade/activation.rs#L57-L149).
- **Partial-failure tolerance**: invalid plugin entries, malformed MCP files, and missing optional config files usually degrade to warnings rather than global failure.
- **Determinism**: plugin config keys, app IDs, versions, and discovered roots are sorted or deduped in many places to reduce nondeterministic behavior.
- **Separation of concerns**: parsing, discovery, install, load, upgrade, and remote mutation are kept in separate modules with minimal overlap.

### Trade-offs

- **Filesystem-heavy implementation**: many operations depend on copying whole directories and spawning `git`, which is simple and portable but can be expensive for large plugin trees.
- **Lexicographic version selection**: `active_plugin_version()` chooses the last sorted version when `local` is absent; this is stable but not semver-aware.
- **Warning-based error handling**: the crate favors resilience over strictness, which makes UX smoother but can hide data-quality issues unless logs are monitored.
- **Direct process execution**: Git integration uses `std::process::Command` instead of a Rust git library, keeping behavior explicit but relying on the host environment.

## Testing Coverage

The crate has focused unit tests around parsing, normalization, and store semantics.

- **Manifest tests** in [manifest.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L383-L545) cover:
  - default prompt normalization,
  - invalid prompt shapes,
  - version trimming,
  - alternate discoverable manifest path support.
- **Marketplace tests** in [marketplace_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_tests.rs#L1-L1530) cover:
  - alternate marketplace layouts,
  - local, URL, and git-subdir source parsing,
  - GitHub shorthand normalization,
  - relative path normalization and traversal rejection,
  - multiple marketplace roots and dedupe behavior,
  - invalid plugin entries being skipped without dropping the marketplace,
  - policy/product gating behavior,
  - category/interface merging.
- **Store tests** in [store_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store_tests.rs#L1-L317) cover:
  - absolute root enforcement,
  - install path layout,
  - manifest name/version handling,
  - `local` version preference,
  - invalid names and mismatches.
- **Loader tests** inside [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L1001-L1129) cover:
  - supported MCP JSON shapes,
  - sparse checkout materialization for Git subdir sources.
- **Toggle tests** in [toggles.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/toggles.rs#L45-L100) cover extraction of plugin enablement edits.
- **Marketplace upgrade git helper tests** in [git.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade/git.rs#L200-L238) cover noninteractive git command construction and SHA detection.

Observations about coverage:

- Parsing and normalization are well-covered.
- Store semantics are well-covered.
- `remote.rs` appears to have no direct unit tests in this crate.
- `marketplace_upgrade.rs` activation/upgrade behavior has little direct local test coverage here; its behavior is exercised more indirectly from higher-level manager tests such as [manager_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/manager_tests.rs#L2763-L3226).

## Consumers in the Workspace

This crate is not isolated; it is a service crate used by higher layers.

- The main consumer is the core plugin manager in [manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/manager.rs#L18-L53), which imports almost every major API.
- Runtime load and cache refresh are invoked from [manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/manager.rs#L424-L430) and [manager.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/manager.rs#L1253-L1457).
- App-server config writes use toggle extraction and telemetry helpers via [config_api.rs](file:///Users/bytedance/project/codex/codex-rs/app-server/src/config_api.rs#L29-L32).

This separation suggests `codex-core-plugins` is intended as reusable plugin infrastructure, while policy and orchestration live above it.

## Notable Design Details

- **Manifest discovery is abstracted** through `find_plugin_manifest_path()`, so plugin layout support can evolve without changing every consumer.
- **Marketplace discovery prefers the first supported manifest layout** under one root, as tested in [marketplace_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_tests.rs#L470-L519).
- **Marketplace names are not globally deduped**; two different roots may produce marketplaces with the same logical name, as tested in [marketplace_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_tests.rs#L657-L761).
- **Duplicate MCP behavior differs by scope**:
  - within a plugin, later files overwrite earlier server definitions in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L517-L531),
  - across plugins, duplicates are warned about at load time in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L114-L137).
- **Remote plugin APIs default marketplace name to `openai-curated`** when the service omits it in [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L18-L24).
- **OAuth callback settings are centralized**: plugin MCP definitions may include `oauth.callbackPort`, but loader code explicitly ignores it in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L847-L855).

## Open Questions

- **Version policy**: should `active_plugin_version()` in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L56-L77) become semver-aware instead of lexicographic for non-`local` versions?
- **Remote test coverage**: should [remote.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/remote.rs#L123-L317) gain HTTP mocking tests, especially for auth mode handling and response validation?
- **Upgrade test coverage**: should the full success/rollback behavior in [marketplace_upgrade.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade.rs#L167-L244) and [activation.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace_upgrade/activation.rs#L57-L149) be directly unit-tested in this crate instead of relying mostly on higher-level integration tests?
- **Symlink handling**: `copy_dir_recursive()` in [store.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L313-L337) copies directories and regular files only; should symlinks be preserved, rejected explicitly, or resolved?
- **Config layering**: `configured_plugins_from_stack()` in [loader.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/loader.rs#L322-L329) only reads the user layer. Is that intentionally the only supported place for plugin enablement and marketplace config?
- **Policy enforcement boundary**: the TODO in [marketplace.rs](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/marketplace.rs#L84-L86) hints that product gating may belong at a different consumer boundary. Is the current split final, or transitional?

## Bottom Line

`codex-core-plugins` is a well-factored infrastructure crate that turns marketplace and plugin metadata into safe, local, runtime-ready plugin artifacts. Its strongest qualities are path normalization, atomic filesystem updates, and tolerant partial-failure behavior. The largest areas that appear worth revisiting are remote/upgrade test coverage, non-semver cache version selection, and a few policy-boundary questions called out by the code itself.
