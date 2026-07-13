# Integration points

Codex exposes several integration surfaces: app-server JSON-RPC/MCP, core MCP clients/tools, plugins/apps/connectors, skills, hooks, model/provider configuration, and local persistence. These are the areas most likely to affect external clients or user data.

## App-server and MCP server

`codex-rs/docs/codex_mcp_interface.md` documents the experimental MCP server interface.

At a glance:

- Server binary: `codex mcp-server` or `codex-mcp-server`.
- Transport: MCP over stdio, JSON-RPC 2.0, line-delimited.
- Primary v2 RPCs: `thread/start`, `thread/resume`, `thread/fork`, `thread/read`, `thread/list`, `turn/start`, `turn/steer`, `turn/interrupt`, account/config/model/app/collaboration-mode reads and writes.
- Remaining v1 compatibility RPCs: `getConversationSummary`, `getAuthStatus`, `gitDiffToRemote`, and fuzzy file search flows.
- Approval requests from server to client include `applyPatchApproval` and `execCommandApproval`.

Source entrypoints:

- `codex-rs/app-server/src/main.rs`: command-line app-server startup.
- `codex-rs/app-server/src/lib.rs`: transport/runtime wiring.
- `codex-rs/app-server/src/request_processors/`: request-specific business logic.
- `codex-rs/app-server-protocol/src/protocol/{common,v1,v2}.rs`: request/response/notification shapes.
- `codex-rs/mcp-server`: MCP server binary crate.
- `codex-rs/codex-mcp` and `codex-rs/rmcp-client`: MCP client/connection/tool plumbing.

Change guidance:

- Prefer v2 thread/turn APIs for new clients.
- Update generated schema fixtures with `just write-app-server-schema` when app-server protocol schemas change.
- Add or update app-server suite tests under `codex-rs/app-server/tests/suite`, especially `suite/v2/`, for API changes.
- Treat v1 compatibility methods as legacy but still supported unless source removes them.

## Plugins and app connectors

Plugins provide capabilities such as MCP servers, hooks, skills, and app declarations. App connectors represent app/tool metadata and runtime snapshots used by connector-backed MCP tools.

Source entrypoints:

- `codex-rs/core-plugins/src/lib.rs`: exported plugin manager, marketplace, loader, provider, and remote types.
- `codex-rs/core-plugins/src/manager.rs`: install/list/read/uninstall, marketplace policy, remote plugin sync, hook/skill/app loading, and callbacks for effective plugin changes.
- `codex-rs/core-plugins/src/manifest.rs`: plugin manifest surface.
- `codex-rs/connectors/src/lib.rs`: app directory cache, app metadata, tool policy, connector runtime exports.
- `codex-rs/connectors/src/connector_runtime/`: process-local runtime snapshots keyed by account/workspace identity with best-effort disk cold-start persistence.
- `codex-rs/app-server/src/effective_plugin_change.rs`: app-server view of effective plugin changes.

Recent `2f7d89b14` extracted connector runtime snapshot management from `codex-mcp` into `codex-rs/connectors/src/connector_runtime/`. The module docs say runtime snapshots are process-local live state scoped by account and workspace; disk is best-effort cold-start persistence, and full connector metadata is owned by the connector metadata store.

Recent `076a110eb` changed trust handling for hooks from materialized workspace plugins. If you change plugin materialization, hook trust, or app MCP routing, inspect both `core-plugins` and app-server tests.

## Hooks

Hooks allow configured commands to run at lifecycle events.

`codex-rs/hooks/src/lib.rs` exports:

- Hook event names: `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `SessionStart`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, `Stop`.
- Matcher-meaningful events: all except `UserPromptSubmit` and `Stop` in the current `HOOK_EVENT_NAMES_WITH_MATCHERS` list.
- Event request/outcome types for compact, permission request, post/pre tool use, session start, stop, and user prompt submit.
- Schema writing helpers used by the root `package.json`/`justfile` schema recipes.

Operational notes:

- `docs/config.md` says admins can set `allow_managed_hooks_only = true` in `requirements.toml` to ignore user/project/session hook configs while still allowing managed hooks from requirements and managed config layers.
- Do not put that setting in `config.toml`; the doc says it is only supported in `requirements.toml`.
- If hook schema changes, use the `write-hooks-schema` script/recipe.

## Skills integration

Skills are exposed through the extension system and can be provided by host, executor, or orchestrator sources.

Source entrypoints:

- `codex-rs/ext/skills/src/lib.rs`: public installation/provider exports.
- `codex-rs/ext/skills/src/extension.rs`: extension install and metrics wiring.
- `codex-rs/ext/skills/src/tools/read.rs`: skill read/tool behavior.
- `codex-rs/ext/skills/src/dynamic_skill_selector/`: cheap lexical selector.
- `codex-rs/ext/skills/src/shadow_selection_experiment.rs`: temporary shadow metrics experiment.

Recent skill-selection work is intentionally observational: `shadow_selection_experiment.rs` records metrics such as run counts, duration, catalog entries, selected entries, query terms, reduction basis points, and invocation rank/hit tags. It filters to enabled, prompt-visible Host/Orchestrator skills to align with observable invocations.

When changing skill selection, update tests under `codex-rs/ext/skills/tests/` and selector unit tests.

## MCP resources/tools inside core

Core MCP behavior spans:

- `codex-rs/core/src/mcp.rs`
- `codex-rs/core/src/mcp_tool_call.rs` and `mcp_tool_call/`
- `codex-rs/core/src/mcp_tool_approval_templates.rs`
- `codex-rs/core/src/mcp_tool_exposure.rs`
- `codex-rs/core/src/tools/handlers/mcp.rs`
- `codex-rs/core/src/tools/handlers/mcp_resource.rs`
- `codex-rs/codex-mcp` and `codex-rs/rmcp-client`

`AGENTS.md` warns that MCP changes should prefer existing connection manager abstractions and minimize plumbing through multiple call levels. Integration tests exist in `codex-rs/core/tests/suite/` for MCP auth refresh/elicitation, tool exposure, turn metadata, resource clients, and hooks-MCP interactions.

## Model providers and model catalog

Model/provider data appears across:

- `codex-rs/model-provider`, `model-provider-info`, `models-manager`
- `codex-rs/protocol/src/openai_models.rs`
- `codex-rs/tui/src/model_catalog.rs`, `model_migration.rs`, model/reasoning popup code
- `codex-rs/app-server` model list processors and protocol responses

`docs/contributing.md` has a specific model metadata rule: set `input_modalities` explicitly for models that do not support images, because omitted modalities currently imply text + image compatibility. Add tests for unsupported-image warnings and paths when model catalogs change.

Recent `769a5de25` made advanced reasoning selection explicit in the TUI, touching app-server thread resume, state extraction, thread-store metadata sync, TUI config persistence, session lifecycle, popups, and snapshots. Reasoning/model changes often cross UI, persistence, and app-server boundaries.

## Config and auth

Primary files:

- `docs/config.md`: points users to external config docs and documents lifecycle hook managed mode.
- `codex-rs/core/src/config/`: core config structures and loading.
- `codex-rs/core/config.schema.json`: generated schema.
- `codex-rs/login`, `codex-rs/keyring-store`, `codex-rs/secrets`: auth and secret storage.
- `codex-rs/app-server/src/config_manager*`: app-server config manager/service.

Never document or read live secret values. If adding config fields, regenerate schema and update tests.

## Persistence and external compatibility

Persistent thread history is an integration surface because external clients may resume/list/fork old sessions.

Files to inspect:

- `codex-rs/rollout/src/lib.rs` and submodules.
- `codex-rs/thread-store/src/types.rs` and local/in-memory stores.
- `codex-rs/app-server-protocol/src/protocol/thread_history*.rs`.
- `codex-rs/tui/src/resume_picker.rs` and session archive/resume/fork commands.

Avoid changing serialized history or metadata without replay/resume tests. Recent response-item ID work (`c9d52de5c`) and rollout ordinal work (`5c19155cb`) show this area has explicit compatibility requirements.
