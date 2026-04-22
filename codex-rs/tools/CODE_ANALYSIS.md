# `codex-tools` crate analysis

## 1. What this crate is for

`codex-tools` is a shared, mostly pure-data crate for describing Codex tools without pulling in the full runtime from `codex-core`. Its stated goal is to host reusable tool-facing primitives, schemas, adapters, and spec builders that multiple crates can depend on directly.

Two files make that boundary explicit:

- `README.md` says the crate is the home for shared tool schema and Responses API primitives, while keeping runtime orchestration in `codex-core`.
- `src/lib.rs` is intentionally exports-only and re-exports a broad surface of constructors, schema helpers, adapters, and planning types.

In practice, the crate does three main jobs:

1. Defines tool-facing data models such as `JsonSchema`, `ToolDefinition`, `ResponsesApiTool`, `ResponsesApiNamespace`, and `ToolSpec`.
2. Converts external tool descriptions (dynamic tools, MCP tools, config-driven built-ins) into Responses API-compatible tool specs.
3. Builds a registry plan that decides which tool specs exist for a session and which runtime handler kind should execute each tool name.

It does **not** execute tools. Instead, it produces the declarative metadata and routing plan that another crate can consume.

## 2. High-level architecture

The crate is organized into a few clean layers.

### A. Core schema and wire models

- `json_schema.rs` defines the supported JSON Schema subset and sanitization logic.
- `tool_definition.rs` defines the internal normalized tool metadata shape.
- `responses_api.rs` and `tool_spec.rs` define Responses API-facing shapes.

These types are the stable center of the crate.

### B. Parsers and adapters

- `dynamic_tool.rs` converts `DynamicToolSpec` into the internal `ToolDefinition`.
- `mcp_tool.rs` converts `rmcp::model::Tool` into `ToolDefinition`, including an MCP result wrapper schema.
- `responses_api.rs` lifts normalized tool definitions into `ResponsesApiTool` / `LoadableToolSpec`.

This layer is where compatibility normalization happens.

### C. Tool-spec builders

Most files named `*_tool.rs` build specific `ToolSpec` values:

- local execution: `local_tool.rs`
- patch editing: `apply_patch_tool.rs`
- collaboration / agents: `agent_tool.rs`, `agent_job_tool.rs`
- plan / user interaction: `plan_tool.rs`, `request_user_input_tool.rs`
- MCP resources: `mcp_resource_tool.rs`
- discovery / suggestion: `tool_discovery.rs`, `tool_suggest.rs`
- code-mode adapters: `code_mode.rs`
- media / utilities: `view_image.rs`, `image_detail.rs`, `js_repl_tool.rs`, `utility_tool.rs`

These are mostly pure constructors: they encode descriptions, JSON schemas, and output schemas.

### D. Session configuration and planning

- `tool_config.rs` derives a `ToolsConfig` from model capabilities, feature flags, auth state, session source, and sandbox constraints.
- `tool_registry_plan_types.rs` defines the output planning types.
- `tool_registry_plan.rs` is the orchestrator that uses config + available tool inputs to build the actual spec list and handler map.

This is the most important control-flow layer in the crate.

## 3. Core types and their responsibilities

### `JsonSchema`

`JsonSchema` models the limited JSON Schema subset the crate is willing to preserve and serialize. It supports:

- primitive and union `type`
- `description`
- `enum`
- `items`
- `properties`
- `required`
- `additionalProperties`
- `anyOf`

This is intentionally smaller than full JSON Schema. The crate normalizes richer or malformed inputs down into this subset before exposing them.

### `ToolDefinition`

`ToolDefinition` is the crate’s normalized internal tool metadata:

- `name`
- `description`
- `input_schema`
- optional `output_schema`
- `defer_loading`

This is the bridge type between “some external tool description” and “a Responses API tool spec”.

### `ResponsesApiTool` and `ToolSpec`

`ResponsesApiTool` is the standard function-tool representation used for OpenAI Responses-compatible tool calling. `ToolSpec` wraps all supported tool kinds:

- function tools
- namespace tools
- tool search
- local shell
- image generation
- web search
- freeform tools

That enum is the main external surface for downstream consumers.

### `ToolRegistryPlan`

`ToolRegistryPlan` is the crate’s orchestration output. It contains:

- `specs`: the user-visible tool declarations, wrapped as `ConfiguredToolSpec`
- `handlers`: a mapping from canonical `ToolName` to `ToolHandlerKind`

This lets downstream runtime code know both:

- what to expose to the model
- what backend handler should run when a tool is called

## 4. Major modules and concrete APIs

### `json_schema.rs`

Responsibility:

- normalize incoming schemas from dynamic tools and MCP tools
- preserve important unions like `anyOf` and `["string", "null"]`
- infer missing object/array/string/number types when schemas are underspecified

Important APIs:

- `parse_tool_input_schema`
- `JsonSchema::{string, number, integer, boolean, null, array, object, any_of, string_enum}`

Behavior worth noting:

- boolean JSON Schema forms (`true` / `false`) are coerced to permissive string schemas
- missing `type` is inferred from structural hints such as `properties`, `items`, `enum`, `additionalProperties`, or numeric keywords
- `const` is rewritten as a single-value `enum`
- singleton `null` input schemas are rejected

This module is a compatibility shim first and a validator second.

### `responses_api.rs`

Responsibility:

- define wire-level function and namespace tool shapes
- convert normalized tools into Responses API-compatible structures
- coalesce namespaced tool groups

Important APIs:

- `tool_definition_to_responses_api_tool`
- `dynamic_tool_to_responses_api_tool`
- `dynamic_tool_to_loadable_tool_spec`
- `mcp_tool_to_responses_api_tool`
- `mcp_tool_to_deferred_responses_api_tool`
- `coalesce_loadable_tool_specs`

Design detail:

- `defer_loading` is only serialized when true
- namespace tools are assembled by grouping child `ResponsesApiTool`s under `ResponsesApiNamespace`

### `tool_spec.rs`

Responsibility:

- define the top-level `ToolSpec` enum used by the rest of the crate
- serialize tool specs into Responses API JSON payloads
- construct non-function top-level tools such as `local_shell`, `image_generation`, and `web_search`

Important APIs:

- `ToolSpec::name`
- `create_local_shell_tool`
- `create_image_generation_tool`
- `create_web_search_tool`
- `create_tools_json_for_responses_api`

Notable behavior:

- `create_web_search_tool` returns `None` when web search mode is disabled
- web search options forward cached/live mode, filters, user location, context size, and content types

### `local_tool.rs`

Responsibility:

- build tool specs for shell-like local execution and approval-related tools

Important APIs:

- `create_exec_command_tool`
- `create_write_stdin_tool`
- `create_shell_tool`
- `create_shell_command_tool`
- `create_request_permissions_tool`
- `request_permissions_tool_description`

Design detail:

- the same file encodes both UX guidance and security semantics
- approval-related arguments are injected conditionally depending on feature flags
- `exec_command` and `write_stdin` are the richer unified-exec pair; `shell` / `shell_command` are simpler one-shot execution interfaces

### `agent_tool.rs`

Responsibility:

- build collaboration and sub-agent tool specs for both V1 and V2 agent APIs

Important APIs:

- `create_spawn_agent_tool_v1`
- `create_spawn_agent_tool_v2`
- `create_send_input_tool_v1`
- `create_send_message_tool`
- `create_followup_task_tool`
- `create_resume_agent_tool`
- `create_wait_agent_tool_v1`
- `create_wait_agent_tool_v2`
- `create_close_agent_tool_v1`
- `create_close_agent_tool_v2`
- `create_list_agents_tool`

Version split:

- V1 is id-based and includes `send_input`, `resume_agent`, and `fork_context`
- V2 is task-name-based and includes `send_message`, `followup_task`, `list_agents`, and `fork_turns`

Design detail:

- this module also owns substantial behavioral guidance in the tool descriptions, especially around when agent spawning is appropriate

### `apply_patch_tool.rs`

Responsibility:

- define the patch-editing tool in freeform and legacy JSON forms

Important APIs:

- `create_apply_patch_freeform_tool`
- `create_apply_patch_json_tool`
- `ApplyPatchToolArgs`

Design detail:

- the freeform variant uses a bundled Lark grammar from `tool_apply_patch.lark`
- the JSON variant exists as a compatibility path, and the file contains a deprecation TODO for removing it later

### `code_mode.rs`

Responsibility:

- adapt normal tool specs into code-mode-compatible nested tool declarations
- build the `exec` and `wait` tools used by code mode

Important APIs:

- `augment_tool_spec_for_code_mode`
- `tool_spec_to_code_mode_tool_definition`
- `collect_code_mode_tool_definitions`
- `collect_code_mode_exec_prompt_tool_definitions`
- `create_code_mode_tool`
- `create_wait_tool`
- `code_mode_name_for_tool_name`

Design detail:

- namespace tools are flattened into code-mode-safe symbol names
- code-mode descriptions are augmented with generated TypeScript declaration samples
- only supported nested tool variants are converted

### `tool_discovery.rs` and `tool_suggest.rs`

Responsibility:

- represent discoverable external tools, plugins, and connectors
- create `tool_search` and `tool_suggest` specs
- transform deferred MCP search results into loadable namespace tool specs
- create elicitation metadata for tool suggestions

Important APIs:

- `create_tool_search_tool`
- `create_tool_suggest_tool`
- `collect_tool_search_source_infos`
- `collect_tool_suggest_entries`
- `filter_tool_suggest_discoverable_tools_for_client`
- `tool_search_result_source_to_loadable_tool_spec`
- `build_tool_suggestion_elicitation_request`

Design detail:

- `tool_search` is presented as a client-executed discovery tool over deferred metadata
- `tool_suggest` is much stricter: it is only meant for missing, clearly matching capabilities
- TUI clients intentionally filter out plugin suggestions and keep connectors only

### `tool_config.rs`

Responsibility:

- turn model capabilities and feature flags into an actionable `ToolsConfig`

Important APIs:

- `ToolsConfig::new`
- builder-style modifiers such as `with_web_search_config`, `with_has_environment`, `with_unified_exec_shell_mode_for_session`
- `for_code_mode_nested_tools`

Inputs considered:

- model-reported tool capabilities
- feature flags from `codex-features`
- session source and sub-agent source
- sandbox policy and Windows sandbox level
- image-generation auth allowance

Design detail:

- this is where the crate decides shell backend type, apply-patch mode, whether tool-search is legal, whether agent-job worker tools are enabled, and whether code mode wraps nested tools

### `tool_registry_plan.rs`

Responsibility:

- assemble the final registry plan from `ToolsConfig` plus available MCP, dynamic, discoverable, and deferred tool inputs

Important API:

- `build_tool_registry_plan`

This is the crate’s highest-level orchestration function and the best single entry point for understanding session-level tool exposure.

## 5. End-to-end flow

The dominant runtime-independent flow in this crate is:

1. Build `ToolsConfig` from model info, enabled features, sandbox policy, auth state, and session source.
2. Call `build_tool_registry_plan`.
3. `build_tool_registry_plan` conditionally adds built-in tool specs based on config.
4. If MCP tools are present, convert them to namespaced Responses API tools and register `Mcp` handlers.
5. If deferred MCP or deferred dynamic tools are present, add `tool_search` and register handler mappings for deferred load.
6. If discoverable tools are present and feature-gated, add `tool_suggest`.
7. If dynamic tools are present, sanitize them, group namespaced entries, and add them as `Function` or `Namespace` specs.
8. If code mode is enabled, build a nested plan first, derive nested tool definitions, then expose top-level `exec` and `wait` wrappers instead of relying solely on direct tools.

Two especially important subflows exist.

### A. External tool ingestion flow

- Dynamic tool input: `DynamicToolSpec` -> `parse_dynamic_tool` -> `ToolDefinition` -> `dynamic_tool_to_loadable_tool_spec` -> `ToolSpec`
- MCP tool input: `rmcp::model::Tool` -> `parse_mcp_tool` -> `ToolDefinition` -> `mcp_tool_to_responses_api_tool` -> `ResponsesApiNamespace` child

The normalization step is where the crate fixes underspecified schemas before any wire serialization happens.

### B. Code-mode wrapping flow

- Start with normal `ToolSpec`s
- Convert supported ones to `codex_code_mode::ToolDefinition`
- Generate code-mode-augmented descriptions and tool names
- Build top-level freeform `exec` plus function `wait`

This keeps code mode as an adaptation layer rather than a separate tool universe.

## 6. Dependency analysis

### Internal workspace dependencies

- `codex-protocol`: central source of tool names, config types, model info, session source, and dynamic tool specs
- `codex-code-mode`: generates code-mode descriptions and nested-tool declarations
- `codex-features`: feature-flag evaluation for what tools should exist
- `codex-app-server-protocol`: app / connector / elicitation metadata for discoverable tools
- `codex-utils-absolute-path`: validated absolute path handling for zsh-fork config
- `codex-utils-pty`: capability probing for unified exec on Windows

### External dependencies

- `serde` and `serde_json`: serialization, deserialization, and literal JSON schema building
- `rmcp`: MCP tool and resource model types, plus schema object wrappers
- `tracing`: logging conversion failures and config fallbacks

### Dependency shape observations

- Most modules are intentionally pure and data-oriented.
- The crate depends on protocol/model crates, but not on execution/session runtime types from `codex-core`.
- That keeps the crate highly testable and matches the migration goal in `README.md`.

## 7. Testing strategy and coverage

The crate follows a consistent “one implementation file, one sibling `*_tests.rs` file” convention. There are 20 sibling test files with 127 `#[test]` cases, which is strong coverage for a data/model crate.

### What the tests focus on

1. **Schema normalization correctness**
   - `json_schema_tests.rs` heavily exercises malformed, underspecified, nullable, `anyOf`, enum, const, and inferred schema cases.
   - This is one of the highest-risk compatibility areas, and it has the deepest test coverage.

2. **Wire-shape serialization**
   - `tool_spec_tests.rs` and `responses_api_tests.rs` verify exact JSON serialization for function, namespace, tool-search, web-search, and deferred-loading shapes.

3. **Config-driven planning**
   - `tool_config_tests.rs` validates feature-flag and environment decisions such as unified exec restrictions, shell backend selection, image generation gating, and sub-agent job worker enablement.
   - `tool_registry_plan_tests.rs` is effectively the integration suite for the crate. It verifies tool presence/absence, handler registration, code-mode augmentation, MCP grouping, deferred tool search, dynamic tools, and suggestion tooling.

4. **Individual tool builders**
   - `agent_tool_tests.rs`, `apply_patch_tool_tests.rs`, `request_user_input_tool_tests.rs`, `tool_discovery_tests.rs`, `mcp_tool_tests.rs`, and others validate exact descriptions, parameters, required fields, and output schemas.

### Coverage strengths

- Exact-structure assertions reduce drift in externally visible tool contracts.
- Tests cover both positive and negative gating conditions.
- Code mode and deferred-loading behavior are tested explicitly.
- MCP and dynamic tool conversion paths are validated separately and inside plan-building.

### Coverage gaps or lighter areas

- There is no property-based or fuzz-style testing for schema sanitization.
- The crate tests declarative outputs, not downstream runtime execution behavior.
- `test_parallel_support_flags` exists but is ignored, so parallel-support semantics are not actively enforced in CI.

## 8. Design strengths

### Clear extraction boundary

The crate does a good job of stopping at “tool metadata + planning”. It does not pull in execution runtime state. That makes the crate easier to reuse and evolve.

### Strongly declarative style

Most modules are deterministic constructors from inputs to tool specs. That makes behavior easy to reason about and easy to snapshot-test.

### Compatibility-first normalization

`json_schema.rs` and `mcp_tool.rs` protect downstream consumers from malformed or underspecified external schemas. This is pragmatic and likely necessary given mixed tool sources.

### Good separation between “what exists” and “how it runs”

`ToolSpec` declares the user/model-facing shape, while `ToolHandlerKind` records the runtime execution route. `ToolRegistryPlan` keeps those concerns adjacent but still distinct.

### Good code-mode layering

Code mode is added as an adapter on top of base tool specs instead of duplicating all tool declarations in a separate code path.

## 9. Design tradeoffs and risks

### Description strings encode behavior policy

A large amount of behavior lives in tool descriptions, especially:

- agent delegation policy
- safety guidance for shell tools
- suggestion workflow rules

That is reasonable for LLM-facing systems, but it also means policy changes require careful text maintenance and can be harder to validate mechanically.

### `build_tool_registry_plan` is becoming a large orchestration hub

It is readable, but it already contains many branches and responsibilities:

- built-in tool inclusion
- code-mode wrapping
- MCP grouping
- deferred search registration
- dynamic tool inclusion
- collab and agent-job routing

This is the main file most likely to keep growing into a maintenance bottleneck.

### Schema sanitization is intentionally lossy

The crate normalizes rich JSON Schema down to a supported subset. That makes interoperability possible, but it also means some original schema semantics may be discarded or approximated.

### Tool families are versioned inline

The V1 and V2 agent APIs coexist in the same module. That keeps related code together, but it increases branching and may make later removal/refactoring noisier.

## 10. Open questions and follow-up items

### Questions raised by the codebase itself

1. `tool_spec.rs` contains a TODO about `web_search` errors despite API docs saying the tool is supported. The crate currently still serializes the tool, but the comment suggests unresolved compatibility uncertainty.
2. `responses_api.rs` contains a TODO about validating “strict” tool schemas. Right now `strict` is always false and the crate does not enforce the stronger schema invariants described in the comment.
3. `apply_patch_tool.rs` contains a TODO to deprecate the JSON `apply_patch` tool once the compatibility path is no longer needed.

### Architectural questions

1. Should `build_tool_registry_plan` be split into feature-specific sub-planners as more tool families are added?
2. Should tool descriptions that encode policy be generated from smaller reusable fragments or declarative policy inputs to reduce copy-heavy string maintenance?
3. Should schema normalization expose a “lossy conversion” report for debugging external tool compatibility issues?
4. Should the crate add property-based tests for `parse_tool_input_schema`, since it is effectively a schema compatibility engine?
5. Should handler registration and spec generation be made more obviously symmetric, perhaps through a shared intermediate representation for built-in tools?

## 11. Bottom line

`codex-tools` is in a good state for its intended role. It is not a generic utilities crate; it is a focused tool-contract and tool-planning crate. The strongest parts are:

- the normalized schema layer
- the Responses API conversion layer
- the config-driven registry planner
- the broad unit and integration test coverage

The main future pressure points are:

- continued growth of `build_tool_registry_plan`
- policy hidden in large description strings
- unresolved compatibility TODOs around strict schemas and web search

Overall, the crate already provides a coherent shared abstraction boundary for tool metadata outside `codex-core`, and its tests suggest that boundary is being treated as a compatibility-sensitive public surface.
