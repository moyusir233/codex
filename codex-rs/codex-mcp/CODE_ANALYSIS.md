# codex-mcp Code Analysis

## Overview

`codex-mcp` is the crate that turns Codex MCP configuration into a runnable, queryable, and model-facing MCP layer. It sits between:

- configuration and auth state from sibling crates such as `codex-config` and `codex-login`
- transport/runtime machinery from `codex-rmcp-client` and `codex-exec-server`
- protocol-facing types from `codex-protocol`

At a high level, the crate does six things:

1. resolves the effective set of MCP servers, including the built-in `codex_apps` server
2. computes auth status and OAuth discovery/scopes for HTTP-based MCP servers
3. starts and manages one RMCP client per enabled server
4. lists, qualifies, filters, caches, and annotates tools for model consumption
5. aggregates tools, resources, and auth state into snapshot payloads
6. helps skill installation detect missing MCP server dependencies

The public API is intentionally small at the crate root (`src/lib.rs`) and mostly re-exports functionality from `src/mcp/mod.rs` and `src/mcp_connection_manager.rs`.

## Module Map

### `src/lib.rs`

Thin re-export surface. Consumers mostly interact with:

- `McpConfig`
- `McpConnectionManager` / `McpManager`
- snapshot helpers such as `collect_mcp_snapshot(...)`
- auth helpers such as `compute_auth_statuses(...)`
- naming helpers such as `split_qualified_tool_name(...)`

### `src/mcp/mod.rs`

High-level orchestration layer for configuration-driven MCP behavior.

Responsibilities:

- defines `McpConfig`
- injects/removes the built-in `codex_apps` MCP server
- derives effective server maps from config + auth
- constructs short-lived `McpConnectionManager` instances for snapshot/resource queries
- converts RMCP models into `codex_protocol` models
- groups model-visible tools back by server
- exposes plugin provenance and permission auto-approval helpers

### `src/mcp/auth.rs`

OAuth/auth-status helper module.

Responsibilities:

- determines whether a transport supports OAuth login
- discovers supported OAuth scopes for streamable HTTP servers
- resolves final scopes from explicit/configured/discovered sources
- computes auth status per server in parallel
- decides when OAuth failures should be retried without discovered scopes

### `src/mcp/skill_dependencies.rs`

Skill-to-MCP dependency resolution.

Responsibilities:

- derives canonical server identity for installed servers and skill dependencies
- compares skill-declared MCP dependencies against installed MCP servers
- builds `McpServerConfig` values for missing dependencies
- deduplicates aliases pointing to the same underlying MCP endpoint/command

### `src/mcp_connection_manager.rs`

Core runtime manager.

Responsibilities:

- owns one async RMCP client startup future per server
- emits MCP startup progress/summary events
- exposes unified tool/resource listing and tool execution
- handles elicitation request routing back into Codex events
- applies allow/deny tool filters
- supports local and executor-backed stdio startup
- caches `codex_apps` tools on disk per user
- decorates tools with model-visible schema adjustments and plugin provenance

This is the architectural center of the crate.

### `src/mcp_tool_names.rs`

Model-visible naming layer.

Responsibilities:

- sanitizes names to fit Responses API constraints
- disambiguates collisions caused by sanitization
- enforces the 64-character limit
- preserves raw MCP identity in `ToolInfo` while generating callable names

## Core Data Types

### `McpConfig`

`McpConfig` is the crate’s long-lived configuration object. It intentionally stores durable runtime inputs only:

- ChatGPT base URL and Codex home path
- OAuth storage/callback settings
- approval and sandbox-related configuration
- whether built-in apps integration is enabled
- configured/plugin-provided MCP servers
- plugin metadata used for tool provenance

Notably, request-scoped auth is not stored in `McpConfig`; helper entry points accept `Option<&CodexAuth>` explicitly. That avoids stale auth/config coupling.

### `ToolInfo`

`ToolInfo` is the bridge between raw MCP and model-facing tool declarations. It stores:

- raw routing identity: `server_name`, `tool.name`
- model-facing identity: `callable_namespace`, `callable_name`
- `server_instructions` from MCP initialize
- connector metadata for `codex_apps`
- plugin attribution metadata

This separation is central to the crate design: models see sanitized callable names, while MCP calls still use the raw tool name.

### `McpConnectionManager`

`McpConnectionManager` is a thin orchestration wrapper around many `RmcpClient` instances. Its public surface is operational:

- list all tools/resources/templates
- call a tool on a specific server
- read/list resources from a specific server
- resolve elicitation responses
- wait for server readiness
- inspect required-server startup failures

Internally, the manager does not eagerly block on all clients before being usable. Each server is represented by an `AsyncManagedClient` shared future, which allows:

- startup in parallel
- startup status reporting
- cached or startup-snapshot tool visibility while initialization is still in flight

### `McpAuthStatusEntry`

Pairs the original `McpServerConfig` with a computed `McpAuthStatus`. This lets the crate preserve config context for better startup error messages.

### `ToolPluginProvenance`

Builds reverse lookup maps from plugin metadata so tools can say which plugin(s) contributed them. The provenance is applied lazily when listing tools rather than being stored in the cache payload.

## Main Flows

## 1. Effective server resolution

Entry points:

- `configured_mcp_servers(...)`
- `effective_mcp_servers(...)`
- `with_codex_apps_mcp(...)`

Flow:

1. start from user/plugin-configured MCP server map
2. if `apps_enabled` is true and auth is ChatGPT auth, inject `codex_apps`
3. build the `codex_apps` transport URL from the configured ChatGPT base URL
4. prefer `CODEX_CONNECTORS_TOKEN` env-based auth when present; otherwise derive HTTP headers from `CodexAuth`

This means `codex_apps` is treated as a synthetic server layered over user config rather than hardcoded into the base server map.

## 2. Auth discovery and status computation

Entry points:

- `oauth_login_support(...)`
- `discover_supported_scopes(...)`
- `resolve_oauth_scopes(...)`
- `compute_auth_statuses(...)`

Flow:

1. inspect server transport
2. only streamable HTTP can support OAuth discovery/status
3. use `codex-rmcp-client` helpers to discover OAuth metadata or determine auth status
4. compute statuses in parallel with `join_all`
5. downgrade failures to `McpAuthStatus::Unsupported` with warning logs

Design note: auth-status calculation is intentionally resilient; failures do not abort the whole snapshot/build process.

## 3. Manager startup

Entry point:

- `McpConnectionManager::new(...)`

Flow:

1. filter enabled servers
2. emit `McpStartupStatus::Starting` for each server
3. create an `AsyncManagedClient` per server
4. spawn parallel startup tasks and later emit per-server final status
5. send a final `McpStartupComplete` event summarizing ready/cancelled/failed servers

Per-server startup (`AsyncManagedClient::new` -> `start_server_task(...)`):

1. validate server name against `^[a-zA-Z0-9_-]+$`
2. create transport-specific `RmcpClient`
3. initialize the server with protocol version `2025-06-18`
4. advertise elicitation capability
5. list tools immediately after initialize
6. write `codex_apps` tool cache if applicable
7. store filtered tool set, timeout, server instructions, and capability info

The manager is designed so startup can fail per server without collapsing the entire MCP subsystem.

## 4. Tool listing and qualification

Entry points:

- `McpConnectionManager::list_all_tools()`
- `qualify_tools(...)`

Flow:

1. each managed client returns listed tools
2. `codex_apps` may serve startup-snapshot/disk-cached tools before live startup finishes
3. tools are filtered by server-specific allow/deny config
4. `codex_apps` file-upload parameters are rewritten into model-visible absolute-path string fields
5. plugin provenance is appended into tool descriptions
6. `qualify_tools(...)` produces a global map keyed by `mcp__...` callable names

The naming layer handles three hard constraints:

- Responses API naming restrictions
- collisions after sanitization
- maximum length of 64 characters

This is one of the crate’s most important boundaries: raw MCP identities remain intact, but model-facing identities are made deterministic and safe.

## 5. Resource and snapshot collection

Entry points:

- `collect_mcp_snapshot(...)`
- `collect_mcp_server_status_snapshot(...)`
- `read_mcp_resource(...)`

Flow:

1. resolve effective servers
2. compute auth statuses
3. create a short-lived `McpConnectionManager`
4. collect tools/resources/templates, optionally skipping resources for `ToolsAndAuthOnly`
5. convert RMCP resource/tool models into `codex_protocol` types
6. cancel the manager after the one-shot operation completes

There are two snapshot shapes:

- model-facing `McpListToolsResponseEvent`
- server-oriented `McpServerStatusSnapshot`

The split is useful:

- one is shaped for protocol/event consumers
- the other preserves grouping by server for status/diagnostics use cases

## 6. Skill dependency auto-install detection

Entry point:

- `collect_missing_mcp_dependencies(...)`

Flow:

1. walk mentioned skill metadata
2. inspect `dependencies.tools`
3. keep only entries with type `mcp`
4. derive a canonical identity from transport + URL/command
5. compare against installed canonical identities
6. return only missing configs, preserving the skill-declared alias as the map key

This design prevents duplicate installs when different aliases point to the same MCP endpoint.

## Public API Surface

The main public API groups cleanly into five categories.

### Configuration and server resolution

- `McpConfig`
- `configured_mcp_servers(...)`
- `effective_mcp_servers(...)`
- `with_codex_apps_mcp(...)`

### Snapshot and status

- `collect_mcp_snapshot(...)`
- `collect_mcp_snapshot_with_detail(...)`
- `collect_mcp_server_status_snapshot(...)`
- `McpSnapshotDetail`
- `McpServerStatusSnapshot`

### Auth and OAuth

- `compute_auth_statuses(...)`
- `oauth_login_support(...)`
- `discover_supported_scopes(...)`
- `resolve_oauth_scopes(...)`
- `should_retry_without_scopes(...)`

### Runtime manager

- `McpConnectionManager`
- `McpRuntimeEnvironment`
- `ToolInfo`
- `SandboxState`
- `codex_apps_tools_cache_key(...)`

### Naming and dependency helpers

- `qualified_mcp_tool_name_prefix(...)`
- `split_qualified_tool_name(...)`
- `group_tools_by_server(...)`
- `collect_missing_mcp_dependencies(...)`
- `canonical_mcp_server_key(...)`

## Dependency Analysis

## Internal workspace dependencies

- `codex-config`: MCP server config types, constrained policy wrappers, OAuth store mode
- `codex-login`: account/token context for `codex_apps`
- `codex-rmcp-client`: actual RMCP transport/client implementation, OAuth discovery/status, elicitation interface
- `codex-protocol`: tool/resource/event/auth types returned to the rest of Codex
- `codex-exec-server`: executor environment for remote or sandbox-aware stdio startup
- `codex-utils-plugins`: connector ID sanitization and allowlist filtering
- `codex-plugin`: plugin capability metadata used for provenance
- `codex-otel`: latency metrics for tool listing/cache writes
- `codex-async-utils`: cancellation helpers around async startup

## External dependencies

- `rmcp`: underlying MCP model types and protocol constants
- `tokio` / `tokio-util`: async runtime, tasks, cancellation
- `async-channel`: event delivery to outer layers
- `futures`: shared startup futures and `join_all`
- `serde` / `serde_json`: cache serialization and model conversion
- `sha1`: stable compact hashes for tool-name disambiguation and cache file keys
- `regex-lite`: server name validation
- `url`: origin extraction for server diagnostics
- `anyhow` / `thiserror`: operational error handling
- `tracing`: non-fatal warnings and instrumentation

## Design Observations

### Strong separation between raw MCP identity and model-visible identity

This is the strongest design choice in the crate.

- raw identity is needed for protocol correctness
- sanitized identity is needed for model/tool API constraints
- `ToolInfo` carries both, avoiding lossy conversions

### Graceful degradation over fail-fast behavior

The crate prefers partial usefulness over all-or-nothing startup:

- auth status failures become warnings
- per-server startup failures do not prevent other servers from working
- cached `codex_apps` tools can still appear while startup is pending or even after failure
- resource listing failures are logged and skipped server-by-server

This makes sense for a heterogeneous MCP environment where some servers may be flaky or optional.

### `codex_apps` is treated as a first-class special case

Special handling appears in several places:

- synthetic server injection
- URL construction
- auth/header derivation
- per-user disk cache
- connector-based naming normalization
- filtering disallowed connector IDs
- file-path schema masking

This is practical, but it also means `codex_apps` is not just “another MCP server”; it is a product-specific extension path embedded into the generic MCP layer.

### Runtime/environment placement is explicit

`McpRuntimeEnvironment` cleanly separates:

- what servers exist (`McpConfig`)
- where they run for this caller (`McpRuntimeEnvironment`)

That keeps “config description” and “execution placement” from being conflated.

### Short-lived managers for snapshot APIs

Snapshot/resource helpers create a manager, use it once, then cancel it. That keeps the high-level API simple and makes the crate usable from one-shot CLI/status commands without forcing a long-lived runtime owner.

## Testing Coverage

The crate has focused unit/integration-style tests in four places:

### `src/mcp/auth.rs`

Tests cover:

- precedence of explicit vs configured vs discovered scopes
- preservation of explicitly empty configured scopes
- retry behavior for discovered-scope OAuth provider failures

### `src/mcp/mod_tests.rs`

Tests cover:

- splitting/grouping qualified tool names
- tool-name prefix sanitization
- plugin provenance mapping
- `codex_apps` URL generation and server insertion
- effective server merging without losing user-configured servers

### `src/mcp/skill_dependencies_tests.rs`

Tests cover:

- canonical installed-key comparison
- deduplication of aliases pointing at the same MCP target

### `src/mcp_connection_manager_tests.rs`

Tests cover:

- file parameter schema masking for model-visible tools
- approval/elicitation policy behavior
- tool qualification, sanitization, collision handling, and max-length behavior
- tool filtering logic
- per-user `codex_apps` cache behavior and schema-version invalidation
- startup snapshot fallback while client startup is pending or failed
- canonical tool resolution from namespaced names
- initialization error message rendering
- transport origin extraction

Overall, the tests are strongest around:

- naming correctness
- cache semantics
- policy handling
- startup fallback behavior

Coverage is lighter around live transport behavior because that logic depends on sibling crates and external servers.

## Notable Behaviors and Invariants

- MCP server names must match `^[a-zA-Z0-9_-]+$` before startup.
- Model-visible tool names are ASCII-safe and capped at 64 characters.
- Duplicate raw tools are skipped rather than overwritten silently.
- Tool filters apply as: allowlist first, then denylist.
- `codex_apps` cache is user-scoped via hashed auth-derived identity.
- `codex_apps` cache payload stores raw tool info; plugin provenance is added on read.
- Streamable HTTP auth status/OAuth discovery apply only to HTTP transports.
- Stdio transports currently report `McpAuthStatus::Unsupported`.
- Snapshot helpers always use read-only sandbox policy for their ephemeral manager.

## Risks and Tradeoffs

### Strengths

- clear responsibility split across modules
- resilient multi-server startup behavior
- careful handling of model/tool naming constraints
- pragmatic caching strategy for the highest-latency server family (`codex_apps`)
- explicit protocol conversion boundaries

### Tradeoffs

- `codex_apps` special casing increases complexity in otherwise generic code paths
- snapshot helpers repeatedly create managers, which may be expensive if called frequently
- auth-status failures are intentionally non-fatal, which can mask root causes unless logs are inspected
- tool qualification uses hashing for collisions/length limits, which is stable enough but harder for humans to predict

## Open Questions

1. `make_rmcp_client(...)` explicitly rejects remote streamable HTTP servers as “not implemented yet”. Is that a near-term roadmap item or should config validation fail earlier with a more targeted UX?
2. `StartServerTaskParams` contains a TODO saying startup timeout should eventually be handled by cancellation. Should timeout ownership move entirely to `CancellationToken`/caller-level orchestration?
3. Snapshot helpers always use `SandboxPolicy::new_read_only_policy()`. Is this intentionally universal for all read/status operations, or are there scenarios where snapshot-time capability detection should reflect a caller’s actual sandbox policy?
4. `codex_apps` caching has schema-version invalidation but no TTL or freshness policy. Is manual/hard refresh sufficient for production usage, or should stale cache age become part of invalidation?
5. `compute_auth_status(...)` treats all stdio transports as `Unsupported`. Is that the intended long-term semantic, or will some stdio servers eventually expose login state through a different mechanism?
6. The crate exposes both model-facing and server-grouped snapshot APIs with partially duplicated setup logic. Would a shared internal “snapshot builder” reduce repetition without hurting clarity?

## Bottom Line

`codex-mcp` is a coordination crate, not a protocol implementation crate. Its value is in operational glue:

- normalize config and auth into effective servers
- start many MCP connections safely
- present stable model-visible tools
- preserve raw routing identity for actual MCP calls
- degrade gracefully when some servers fail

The design is pragmatic and production-oriented. The most important architectural ideas are the separation of raw-vs-callable tool identity, the special but disciplined handling of `codex_apps`, and the bias toward partial availability instead of global failure.
