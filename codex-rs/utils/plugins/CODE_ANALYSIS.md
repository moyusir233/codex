# codex-utils-plugins Code Analysis

## Overview

`codex-utils-plugins` is a small shared utility crate that centralizes plugin-adjacent conventions used across the Codex workspace. Its scope is intentionally narrow:

- plugin manifest discovery and namespace extraction for skill paths
- plaintext mention sigils for tools and plugins
- MCP connector filtering and identifier sanitization

The crate exports three public modules from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/lib.rs#L1-L9):

- [mcp_connector.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L1-L50)
- [mention_syntax.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mention_syntax.rs#L1-L7)
- [plugin_namespace.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L1-L121)

This crate does not own end-user plugin loading or MCP session lifecycle. Instead, it provides small reusable building blocks that other crates compose into higher-level behavior.

## Crate Metadata and Dependencies

The crate definition in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/Cargo.toml#L1-L25) shows a lightweight dependency surface:

- `codex-exec-server`: supplies `ExecutorFileSystem`, which lets plugin namespace lookup work against abstracted file systems rather than hard-coded local disk access
- `codex-login`: supplies the runtime originator check used to decide which connector blocklist applies
- `codex-utils-absolute-path`: supplies `AbsolutePathBuf` for path safety and consistent APIs
- `serde` and `serde_json`: used only for minimal JSON manifest parsing
- `tempfile` and `tokio` as dev-dependencies for async filesystem-backed tests

There are no feature flags, build scripts, or additional binaries. The Bazel target in [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/BUILD.bazel#L1-L6) simply exposes the Rust crate under the same crate name.

## Module Responsibilities

### 1. Plugin namespace resolution

The most substantial logic lives in [plugin_namespace.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L1-L121).

#### Responsibilities

- discover a plugin manifest from a plugin root using one of two supported relative paths:
  - `.codex-plugin/plugin.json`
  - `.claude-plugin/plugin.json`
- read just enough of the manifest to derive a namespace-like plugin name
- walk up from a skill file path to the nearest ancestor that looks like a plugin root
- expose both synchronous local-path discovery and async filesystem-abstracted namespace lookup

#### Public API

- [find_plugin_manifest_path](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L11-L16) -> `Option<PathBuf>`
  - input: plugin root candidate as `&Path`
  - behavior: checks both discoverable manifest paths on local disk and returns the first file that exists
- [plugin_namespace_for_skill_path](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L56-L68) -> `Option<String>`
  - input: abstract filesystem plus an absolute skill path
  - behavior: walks `path.ancestors()` and returns the first valid manifest-derived name

#### Internal flow

The internal resolution path is:

1. `plugin_namespace_for_skill_path` iterates upward through all ancestors of the provided path.
2. For each ancestor, [plugin_manifest_name](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L25-L54) checks both discoverable manifest locations using `ExecutorFileSystem::get_metadata`.
3. If a manifest file exists, it reads the file as text and deserializes only `{ "name": ... }` into `RawPluginManifestName`.
4. If the manifest `name` is blank or missing, it falls back to the plugin root directory name.
5. The first ancestor producing a valid name wins.

This logic matches the fallback behavior in [core-plugins manifest loading](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L117-L136), which is an important design consistency point: skill namespacing stays aligned with full plugin manifest parsing.

#### Workspace usage

- [core-skills loader](file:///Users/bytedance/project/codex/codex-rs/core-skills/src/loader.rs#L643-L651) prefixes discovered skill names as `plugin_name:skill_name`
- [core-plugins manifest loading](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/manifest.rs#L117-L136) uses the same manifest path discovery helper
- [core-plugins store](file:///Users/bytedance/project/codex/codex-rs/core-plugins/src/store.rs#L194-L218) uses the same helper to locate `plugin.json` and read version metadata

This makes the crate a shared source of truth for “where is the manifest?” and “what plugin namespace should this skill inherit?”.

### 2. Mention syntax constants

[mention_syntax.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mention_syntax.rs#L1-L7) is intentionally tiny, but it encodes an important cross-crate contract.

#### Public API

- [TOOL_MENTION_SIGIL](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mention_syntax.rs#L3-L5) = `$`
- [PLUGIN_TEXT_MENTION_SIGIL](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mention_syntax.rs#L6-L7) = `@`

#### Responsibilities

- provide one canonical definition of plaintext mention prefixes
- prevent TUI, core, and skill parsing logic from drifting on syntax

#### Workspace usage

- [tui mention codec](file:///Users/bytedance/project/codex/codex-rs/tui/src/mention_codec.rs#L4-L5) uses both sigils when encoding and decoding linked mentions
- [core plugin mentions](file:///Users/bytedance/project/codex/codex-rs/core/src/plugins/mentions.rs#L13-L14) uses them when collecting tool and plugin mentions from chat messages
- [core-skills injection](file:///Users/bytedance/project/codex/codex-rs/core-skills/src/injection.rs#L254-L255) uses the tool sigil to extract explicit tool mentions

The design value here is consistency, not complexity.

### 3. MCP connector filtering and normalization

[mcp_connector.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L1-L50) contains two distinct policies: connector allow/block decisions and connector/tool name sanitization.

#### Public API

- [is_connector_id_allowed](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L16-L18) -> `bool`
- [sanitize_name](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L31-L33) -> `String`

#### Responsibilities

- maintain a hard-coded denylist of connector IDs
- apply a stricter or alternate denylist for first-party chat originators
- block connector IDs with the prefix `connector_openai_`
- normalize arbitrary names into lowercase ASCII slug-ish identifiers and then convert `-` to `_`

#### Internal flow

1. `is_connector_id_allowed` calls `codex_login::default_client::originator()`.
2. It passes the originator value to [is_connector_id_allowed_for_originator](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L20-L29).
3. That function selects either:
   - the general `DISALLOWED_CONNECTOR_IDS`, or
   - `FIRST_PARTY_CHAT_DISALLOWED_CONNECTOR_IDS` when [is_first_party_chat_originator](file:///Users/bytedance/project/codex/codex-rs/login/src/auth/default_client.rs#L127-L128) returns true
4. It rejects any ID in the chosen denylist and any ID starting with `connector_openai_`.

`sanitize_name` is backed by [sanitize_slug](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L35-L50):

- keeps only ASCII alphanumeric characters
- lowercases ASCII letters
- replaces all other characters with `-`
- trims leading and trailing `-`
- falls back to `"app"` if nothing remains
- finally converts `-` to `_`

#### Workspace usage

- [codex-mcp connection manager](file:///Users/bytedance/project/codex/codex-rs/codex-mcp/src/mcp_connection_manager.rs#L1319-L1370) uses `sanitize_name` to normalize callable names and namespaces for Codex Apps-backed MCP tools
- [codex-mcp connection manager](file:///Users/bytedance/project/codex/codex-rs/codex-mcp/src/mcp_connection_manager.rs#L1712-L1720) uses `is_connector_id_allowed` to filter tool exposure

This module is more policy-heavy than the others because it embeds product decisions directly in code.

## API Summary

The crate’s effective public surface is small:

- [lib.rs re-exports](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/lib.rs#L4-L9)
- [mcp_connector::is_connector_id_allowed](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L16-L18)
- [mcp_connector::sanitize_name](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L31-L33)
- [mention_syntax::TOOL_MENTION_SIGIL](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mention_syntax.rs#L3-L5)
- [mention_syntax::PLUGIN_TEXT_MENTION_SIGIL](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mention_syntax.rs#L6-L7)
- [find_plugin_manifest_path](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L11-L16)
- [plugin_namespace_for_skill_path](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L56-L68)

This is a classic “utility crate” API: small, stable, and dependency-light, with behavior meant to be shared rather than customized.

## End-to-End Flows

### Skill namespacing flow

The most important end-to-end flow is skill namespacing:

1. A skill loader has a `SKILL.md` absolute path.
2. [core-skills loader](file:///Users/bytedance/project/codex/codex-rs/core-skills/src/loader.rs#L643-L651) calls `plugin_namespace_for_skill_path`.
3. `plugin_namespace_for_skill_path` walks ancestors and discovers the nearest plugin manifest.
4. The manifest name, or directory fallback, becomes the namespace.
5. The final skill name is emitted as `namespace:base_name`.

This is how plugin-provided skills become distinguishable from built-in or standalone skills without requiring the loader itself to understand manifest locations.

### Plugin manifest discovery flow

Another shared flow is manifest discovery:

1. A higher-level crate knows a plugin root directory.
2. It calls `find_plugin_manifest_path`.
3. The utility checks `.codex-plugin/plugin.json` and `.claude-plugin/plugin.json`.
4. The caller reuses the returned path for full manifest parsing or version extraction.

This avoids each caller inventing its own notion of supported manifest locations.

### MCP tool normalization flow

For Codex Apps-backed MCP tools:

1. MCP code receives connector metadata and tool names.
2. It normalizes those names with `sanitize_name`.
3. It constructs stable callable names and namespaces.
4. It drops tools tied to blocked connector IDs.

This flow helps prevent naming mismatches and product-excluded connectors from surfacing downstream.

## Design Characteristics

### Strong points

- Single-purpose utilities: each module stays focused on one cross-cutting concern
- Low coupling: the crate holds very little business state and no runtime manager types
- Consistent fallback behavior: plugin namespace logic mirrors full manifest loading semantics
- Abstraction-friendly filesystem access: async namespace lookup takes `&dyn ExecutorFileSystem`
- Shared syntax contracts: mention sigils live in one place instead of being duplicated

### Trade-offs

- Policy is hard-coded: connector denylist contents and alternate manifest locations are compiled in
- Async/sync split: `find_plugin_manifest_path` uses direct local `Path::is_file`, while namespace resolution uses abstract async filesystem access
- Minimal manifest parsing: only `name` is parsed here, which keeps things light but can silently return `None` on malformed files
- Utility-crate discoverability: behavior is spread across several tiny modules, so consumers need workspace context to understand why each helper exists

## Testing

The crate currently has unit tests only in [plugin_namespace.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L70-L121).

### Covered

- manifest-based namespace resolution through `.codex-plugin/plugin.json`
- alternate manifest discovery through `.claude-plugin/plugin.json`
- synchronous `find_plugin_manifest_path` behavior for the alternate path

### Not covered in this crate

- connector allowlist/denylist behavior in [mcp_connector.rs](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/mcp_connector.rs#L1-L50)
- `sanitize_name` edge cases
- blank manifest `name` fallback to directory name
- malformed JSON or unreadable manifest handling
- ancestor walk precedence when multiple ancestors contain manifests

I also ran the crate tests successfully with:

```bash
cargo test -p codex-utils-plugins
```

Result: 2 tests passed, 0 failed.

## Design Observations

### Why the filesystem trait matters

Using `ExecutorFileSystem` in [plugin_namespace_for_skill_path](file:///Users/bytedance/project/codex/codex-rs/utils/plugins/src/plugin_namespace.rs#L58-L68) is a good design choice because the caller can resolve plugin namespaces in execution environments where direct host filesystem access is abstracted or sandboxed. That keeps this helper usable in more places than a simple `std::fs` implementation would.

### Why the crate exists separately

The crate seems to act as a thin “shared contract” layer between several larger crates:

- `core-skills` needs namespacing
- `core-plugins` needs manifest location discovery
- `codex-mcp` needs connector policy and stable naming
- `core` and `tui` need consistent mention syntax

That separation reduces duplication in consuming crates, though not all duplication has been removed yet.

### Notable duplication nearby

There is still similar namespace logic in [plugin/src/plugin_namespace.rs](file:///Users/bytedance/project/codex/codex-rs/plugin/src/plugin_namespace.rs#L1-L70), which only supports `.codex-plugin/plugin.json` and uses synchronous local filesystem access. This suggests the workspace is in a partial migration toward `codex-utils-plugins` as the shared implementation, but not all callers have converged yet.

There is also similar name sanitization logic in [connectors/src/metadata.rs](file:///Users/bytedance/project/codex/codex-rs/connectors/src/metadata.rs), which suggests connector normalization policy may still exist in more than one place.

## Open Questions

1. Should `plugin/src/plugin_namespace.rs` be removed or rewritten to delegate to `codex-utils-plugins` so the workspace has exactly one namespace-resolution implementation?
2. Should `sanitize_name` live in exactly one crate? Right now similar naming behavior exists in both `utils/plugins` and `connectors`.
3. Should connector denylist policy remain hard-coded, or move into a config, manifest, or remotely managed policy source?
4. Should this crate add unit tests for `mcp_connector.rs`, especially the first-party originator branch and slug sanitization edge cases?
5. Should `plugin_namespace_for_skill_path` distinguish between “manifest missing” and “manifest malformed” for better debugging, instead of collapsing both to `None`?
6. Should `find_plugin_manifest_path` also operate through `ExecutorFileSystem` for consistency with the async path-based lookup, or is its current sync/local-only design intentional for caller simplicity?

## Bottom Line

`codex-utils-plugins` is a compact shared-contract crate. Its most important role is not complex logic, but consistency:

- one definition of plugin manifest discovery
- one definition of plugin skill namespacing fallback
- one definition of plaintext mention sigils
- one shared place for MCP connector filtering and name cleanup

The crate is clean and intentionally small, but it currently carries two risks:

- some behavior is under-tested, especially connector policy
- some nearby functionality still appears duplicated elsewhere in the workspace

Those are good targets for future cleanup if this crate is meant to be the long-term shared home for plugin utility behavior.
