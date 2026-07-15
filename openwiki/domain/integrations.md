# Integration points

## App-server: external application API

`codex-rs/app-server` is the IDE/app JSON-RPC boundary. It supports stdio, Unix sockets, WebSockets, or no listening transport, plus session-source restrictions, strict config, WebSocket auth, daemon/proxy, and remote-control flows (`app-server/src/main.rs`, `cli/src/main.rs`). Protocol types and generated schemas live in `app-server-protocol`.

Prefer v2 thread/turn RPCs for new clients. When changing this API, update request processors, protocol types, schema fixtures (`just write-app-server-schema`), and `app-server/tests/suite`, especially `suite/v2/`.

## MCP in both directions

Codex plays two different roles:

- **MCP server:** `codex mcp-server` / `codex-mcp-server` exposes an experimental stdio MCP adapter documented in `codex-rs/docs/codex_mcp_interface.md`.
- **MCP client:** `codex-mcp` and `rmcp-client` manage configured stdio/HTTP servers, catalogs, resources/tools, Apps integration, OAuth, elicitation, provenance, conflicts, and sandbox metadata. Core exposes and invokes those tools through `core/src/mcp*` and tool handlers.

Prefer the existing MCP connection manager rather than plumbing mutable connection behavior through core. MCP auth and tool exposure are concurrency/compatibility-sensitive; use core suite and connection-manager tests.

## Typed runtime extensions

`ext/extension-api` is an in-process contributor framework, not an installable plugin format. `ExtensionRegistryBuilder` registers contributors for thread/turn lifecycle, config, token usage, skill invocation, context, runtime MCP servers, turn input/items, native tools, tool lifecycle, and approval review (`ext/extension-api/src/registry.rs`).

Concrete built-ins under `ext/*` install against this API. App-server composition begins in `app-server/src/extensions.rs`. New contribution types should keep the API small, preserve ordering/claim semantics, and include extension plus host integration tests.

## Installable plugins

`plugin/src/manifest.rs` defines packages that can declare skills, MCP servers, apps, hooks, and model/UI-facing metadata. `core-plugins` owns marketplaces, source policy, install/upgrade/uninstall, remote and startup synchronization, and effective plugin state.

Do not confuse plugin package resources with the typed extension registry. Plugin changes may affect app-server plugin processors, hook trust, skill/app visibility, and MCP routing. Materialized workspace-plugin hooks become trusted only after successful serialized config state updates; failures should remain fail-closed (`076a110eb`).

## Connectors

`connectors` has two different caches:

- App-directory metadata with in-memory/disk caching (`connectors/src/lib.rs`).
- Account/workspace-scoped live MCP tool snapshots (`connectors/src/connector_runtime/`). Disk snapshots are best-effort cold-start state, read once per context; full metadata belongs to the connector metadata store.

`2f7d89b14` moved live snapshot ownership out of `codex-mcp` so connector identity, atomic publication, persistence, and stale-generation handling are reusable and transport-independent.

## Skills

- `core-skills`: host loading, policy, namespacing, budgets, and skill services.
- `ext/skills`: typed providers and tools for Host, Executor, and Orchestrator sources.
- Plugin manifests may package skill paths.

Changes to discovery or policy belong in `core-skills`; changes to typed provider/read/selection behavior belong in `ext/skills`. The current weighted lexical path is an observational shadow experiment, not evidence of active automatic selection.

## Hooks and approvals

`hooks` defines pre/post tool use, permission request, pre/post compact, session start, user prompt submit, subagent start/stop, and stop events. Matchers apply to all current events except user-prompt-submit and stop (`hooks/src/lib.rs`). Generated schemas live under `hooks/schema/generated/` and are refreshed with `just write-hooks-schema`.

Permission-request hooks run before Guardian or user review and may resolve the request. Managed policy can restrict accepted hook layers; `docs/config.md` documents `allow_managed_hooks_only` as a `requirements.toml` setting, not a normal `config.toml` field.

## Models, auth, and catalogs

Provider configuration, runtime ownership, and catalog management are split across `model-provider-info`, `model-provider`, and `models-manager`. Auth and secure storage involve `login`, `keyring-store`, and `secrets`; never document live credentials.

Model changes should account for provider capabilities, catalog visibility, app-server model responses, TUI choices, persisted reasoning metadata, and unsupported input modalities. `docs/contributing.md` specifically requires explicit modalities for models without image support.

## Persistence as an integration

External clients depend on old thread history being listable, resumable, and forkable. Changes to response-item IDs, rollout ordinals, metadata, or history projection must cover `rollout`, `thread-store`, `state`, `app-server-protocol/src/protocol/thread_history*`, and client/UI restoration paths.
