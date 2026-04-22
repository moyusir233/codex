# app-server-protocol crate analysis

## Scope and purpose

This crate defines the typed wire contract between Codex app-server clients and the server, plus the schema-generation tooling that publishes that contract as vendored JSON Schema and TypeScript artifacts.

- Crate metadata and dependencies live in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/Cargo.toml#L1-L42).
- The public surface is assembled in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/lib.rs#L1-L49), which re-exports:
  - transport envelopes from [jsonrpc_lite.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/jsonrpc_lite.rs#L1-L88),
  - protocol types from [protocol/common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L17-L1077), [v1.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v1.rs#L26-L245), and [v2.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs),
  - schema export helpers from [export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L39-L240),
  - fixture management helpers from [schema_fixtures.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/schema_fixtures.rs#L24-L113),
  - thread-history reconstruction and synthetic item builders from [thread_history.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L73-L1187) and [item_builders.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/item_builders.rs#L1-L313).

In practice, the crate has three equally important responsibilities:

1. Define stable and experimental request/response/notification payloads.
2. Project lower-level `codex_protocol` data into app-server-friendly DTOs.
3. Generate and verify vendored schemas under `schema/json` and `schema/typescript`.

## Module map

### 1. Transport and top-level exports

- [jsonrpc_lite.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/jsonrpc_lite.rs#L11-L88) defines a lightweight JSON-RPC shape that intentionally omits the on-the-wire `"jsonrpc": "2.0"` field while still modeling request ids, requests, notifications, responses, and errors.
- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/lib.rs#L7-L49) flattens the crate into a consumer-friendly API, so downstream code can depend on one crate instead of navigating internal modules.

### 2. Protocol model definition

- [protocol/common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L71-L227) defines macros that generate the typed request/response/notification enums and their schema-export helpers.
- [protocol/v1.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v1.rs#L26-L245) contains legacy/bootstrap-compatible types.
- [protocol/v2.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs) contains the current protocol surface: config, thread lifecycle, turn lifecycle, approvals, account/auth, skills/plugins/apps, filesystem, MCP, realtime, review, and UI-facing thread item models.
- [protocol/mappers.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/mappers.rs#L1-L23) bridges a legacy one-off command payload into the newer `command/exec` model.
- [protocol/serde_helpers.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/serde_helpers.rs#L1-L23) centralizes double-`Option` serialization for fields where “unset” and “explicit null” must stay distinct.

### 3. Experimental API gating

- [experimental_api.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/experimental_api.rs#L4-L56) defines the `ExperimentalApi` trait and a registry of experimental fields.
- The macros in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L35-L69) and many types in [v2.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L762-L800) use `#[experimental(...)]` annotations to mark gated methods or fields.

### 4. Schema generation and vendoring

- [export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L76-L240) generates TypeScript and JSON schema output.
- [schema_fixtures.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/schema_fixtures.rs#L82-L113) regenerates the checked-in fixture trees and normalizes them for deterministic tests.
- The binaries [src/bin/export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/bin/export.rs#L1-L34) and [src/bin/write_schema_fixtures.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/bin/write_schema_fixtures.rs#L1-L42) expose those workflows from the CLI.

### 5. App-server presentation logic

- [item_builders.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/item_builders.rs#L1-L313) synthesizes `ThreadItem` values for approvals, command execution, patch application, and guardian review events.
- [thread_history.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L73-L1187) replays persisted rollout items and event streams into v2 `Turn` and `ThreadItem` history suitable for `thread/read`, resume, and fork responses.

## Concrete responsibilities

### Typed wire contract

The crate’s main protocol abstraction lives in the macro-generated enums in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L75-L227), [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L628-L757), and [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L762-L1077):

- `ClientRequest` and `ClientResponse` model client-to-server RPCs and typed replies.
- `ServerRequest` and `ServerResponse` model server-initiated prompts that the client must answer, such as approvals and elicitation.
- `ServerNotification` and `ClientNotification` model one-way events.

This macro approach makes the request registry the single source of truth for:

- wire method names,
- Rust payload types,
- TypeScript export traversal,
- JSON schema export traversal,
- experimental method tracking,
- helper methods such as `id()` and `method()`.

### Rich v2 domain model

The v2 layer is much more than RPC payload wrappers. It contains the UI-facing data model the app-server exposes.

Representative examples:

- Config API:
  - [Config](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L762-L800)
  - [ConfigReadResponse](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L882-L891)
  - [ConfigRequirementsReadResponse](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L958-L965)
- Thread lifecycle:
  - [ThreadStartParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L2881-L2937)
  - [ThreadStartResponse](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L2956-L2974)
  - [ThreadResumeParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L2976-L3043)
  - [ThreadForkParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L3065-L3123)
  - [Thread](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L4053-L4092)
- Turn lifecycle:
  - [TurnStartParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L4485-L4546)
  - [TurnSteerParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L4622-L4637)
  - [Turn](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L4160-L4181)
  - [TurnStatus](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L4478-L4483)
- UI/history projection:
  - [UserInput](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L4728-L4803)
  - [ThreadItem](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L4804-L4897)
- Approval and client-interaction requests:
  - [CommandExecutionRequestApprovalParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L6091-L6162)
  - [McpServerElicitationRequestParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L6226-L6244)
  - [DynamicToolCallParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L6652-L6662)
  - [ToolRequestUserInputParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L6749-L6774)

### Projection from `codex_protocol`

This crate is not the “core truth” of all domain types. Instead, it adapts lower-level core models into app-server-facing shapes:

- dozens of `From`/`TryFrom` impls in [v2.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L199-L6893) convert core config, tool-call, approval, rate-limit, and MCP types.
- [mappers.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/mappers.rs#L1-L23) translates v1 one-off command parameters into v2 `CommandExecParams`.
- [thread_history.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L162-L225) converts persisted `EventMsg` values into a richer UI history model.

That separation is important: `codex_protocol` appears to define internal/core events, while this crate defines the app-server boundary contract and presentation semantics.

## Key API surface

### Bootstrap and compatibility

- v1 still exists for initialize and older flows:
  - [InitializeParams](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v1.rs#L26-L32)
  - [InitializeCapabilities](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v1.rs#L42-L53)
  - [InitializeResponse](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v1.rs#L55-L67)
- Deprecated v1 request families remain registered in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L593-L626) and [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L898-L910).

### Main client-request families

The client request registry in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L230-L626) shows the actual public method map. Major families:

- Thread management: `thread/start`, `thread/resume`, `thread/fork`, `thread/read`, `thread/list`, `thread/rollback`, `thread/compact/start`.
- Turn execution: `turn/start`, `turn/steer`, `turn/interrupt`.
- Command execution: `command/exec`, `command/exec/write`, `command/exec/terminate`, `command/exec/resize`.
- Config and external-agent config: `config/read`, `config/value/write`, `config/batchWrite`, `configRequirements/read`, `externalAgentConfig/detect`, `externalAgentConfig/import`.
- Filesystem: `fs/readFile`, `fs/writeFile`, `fs/createDirectory`, `fs/getMetadata`, `fs/readDirectory`, `fs/remove`, `fs/copy`, `fs/watch`, `fs/unwatch`.
- Skills/plugins/apps: `skills/list`, `skills/config/write`, `plugin/list`, `plugin/read`, `plugin/install`, `plugin/uninstall`, `marketplace/add`, `marketplace/remove`, `app/list`.
- Account/auth/models: `account/login/start`, `account/login/cancel`, `account/logout`, `account/read`, `account/rateLimits/read`, `model/list`.
- MCP and review: `mcpServer/oauth/login`, `config/mcpServer/reload`, `mcpServerStatus/list`, `mcpServer/resource/read`, `mcpServer/tool/call`, `review/start`.
- Experimental realtime: `thread/realtime/*`.

### Server-initiated request families

The server request registry in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L853-L911) shows the interactive prompts the client must satisfy:

- command approval,
- file-change approval,
- generic permission approval,
- MCP elicitation,
- dynamic tool execution,
- auth token refresh,
- legacy apply-patch and exec-command approvals.

### Notifications

The server notification registry in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L995-L1073) shows the event stream shape. It includes:

- thread lifecycle notifications,
- turn lifecycle notifications,
- streaming deltas for plans, reasoning, agent messages, command output, and file-change patches,
- approval auto-review lifecycle,
- account and app/plugin updates,
- MCP status and progress events,
- warnings and deprecation notices,
- realtime experimental events.

## Data flow and execution flow

### 1. Typed RPC flow

At the boundary, the crate represents messages in two layers:

- raw envelopes via [JSONRPCMessage / JSONRPCRequest / JSONRPCNotification / JSONRPCResponse / JSONRPCError](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/jsonrpc_lite.rs#L34-L88),
- typed app-server method enums via [ClientRequest](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L87-L121), [ServerRequest](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L642-L714), and [ServerNotification](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L769-L815).

`TryFrom` conversions in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L799-L805) and [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L845-L850) let JSON-RPC notification/request envelopes be decoded into typed server-side messages.

### 2. Experimental gating flow

Experimental support is intentionally first-class:

- `InitializeCapabilities.experimental_api` in [v1.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v1.rs#L42-L53) negotiates whether experimental API surface is enabled.
- The `ExperimentalApi` trait in [experimental_api.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/experimental_api.rs#L4-L32) returns the first experimental reason used by a value.
- `client_request_definitions!` in [common.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L158-L189) automatically exposes experimental reasons for request variants and collects the method/type lists used by schema filtering.
- Schema generation then removes those methods and fields unless the caller opts in, via [filter_experimental_ts](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L243-L253), [filter_experimental_schema](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L397-L404), and [filter_experimental_json_files](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L542-L550).

This is a strong design choice: the experimental/stable split is enforced by code generation, not just by documentation.

### 3. History reconstruction flow

The thread-history subsystem is a second major execution path:

- [build_turns_from_rollout_items](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L73-L83) creates a `ThreadHistoryBuilder`.
- [handle_rollout_item](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L228-L239) dispatches each persisted `RolloutItem`.
- [handle_event](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L157-L225) reduces `EventMsg` values into updates on the active turn.
- [upsert_item_in_turn_id](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L1032-L1049) keeps late completions attached to the original turn when an event arrives after newer turns start.
- [finish_current_turn](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L988-L995) finalizes turn state while preserving explicit/compaction-only turns.

This reducer is intentionally app-server-specific. It does not mirror raw core events one-to-one. Instead, it turns them into a stable, UI-facing `Turn`/`ThreadItem` representation.

### 4. Synthetic item flow

`ThreadItem` values are created from two sources:

- directly from event replay in [thread_history.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L267-L1113),
- indirectly through shared builder helpers in [item_builders.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/item_builders.rs#L41-L313).

The builders cover cases where the app-server wants a coherent lifecycle even when the core event stream is approval-centric or incomplete:

- file-change approval requests become synthetic `ThreadItem::FileChange`,
- command approval and guardian assessment events become synthetic `ThreadItem::CommandExecution`,
- guardian review notifications are synthesized as app-server `ServerNotification` payloads.

### 5. Schema generation flow

The schema export path is another full subsystem:

- [generate_ts_with_options](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L105-L179) exports `ts-rs` types, optionally filters experimental content, generates `index.ts`, prepends standard headers, runs Prettier, and trims whitespace.
- [generate_json_with_experimental](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L192-L240) emits individual JSON Schemas and two bundles:
  - a mixed bundle at `codex_app_server_protocol.schemas.json`,
  - a flattened v2-only bundle at `codex_app_server_protocol.v2.schemas.json`.
- [build_schema_bundle](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L943-L1024) namespaces definitions and rewrites `$ref`s.
- [build_flat_v2_schema](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L1026-L1086) lifts `definitions.v2.*` into a flat root map for downstream generators that cannot traverse nested namespaces.
- [schema_fixtures.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/schema_fixtures.rs#L124-L208) canonicalizes JSON and normalizes TypeScript for stable cross-platform fixture comparisons.

## Dependencies and why they matter

From [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/Cargo.toml#L14-L42):

- `codex-protocol`: core domain types and event stream models that this crate adapts.
- `codex-shell-command`: parses shell commands into structured `CommandAction` data.
- `codex-utils-absolute-path`: provides strongly typed absolute paths for wire-safe path fields.
- `schemars`: generates JSON Schema from Rust types.
- `ts-rs`: generates TypeScript declarations from Rust types.
- `serde` / `serde_json` / `serde_with`: serialization layer, including nuanced nullable-vs-absent handling.
- `codex-experimental-api-macros` + `inventory`: derive and collect experimental field/method metadata.
- `rmcp`: models MCP elicitation and server interaction payloads.
- `clap`: powers the schema-generation binaries.
- `strum_macros`: provides display/serialization support for some enums.
- `tracing`: used in history reconstruction for drop-path warnings.
- `uuid`: generates synthetic turn ids during history reconstruction.

Dev dependencies show the maintenance strategy:

- `pretty_assertions` and `similar` improve schema and serialization diff quality.
- `tempfile` supports isolated schema-generation tests.
- `codex-utils-cargo-bin` resolves vendored fixture resources in tests.

## Testing strategy

The crate has unusually strong test coverage for a protocol crate. The tests are not just serde roundtrips; they verify contract generation and reducer semantics.

### Contract and schema tests

- [tests/schema_fixtures.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/tests/schema_fixtures.rs#L11-L143) compares checked-in schema fixtures against freshly generated output and gives diff-friendly failures.
- Export-layer tests in [export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L2052-L2833) verify:
  - experimental filtering for TS and JSON,
  - v2 bundle flattening,
  - ref rewriting to namespaced definitions,
  - stable TypeScript optional/nullability conventions.
- Fixture helpers in [schema_fixtures.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/schema_fixtures.rs#L337-L360) test JSON canonicalization stability.

### Protocol serde and gating tests

- [common.rs tests](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/common.rs#L1079-L2073) cover:
  - JSON wire shape for representative requests/responses/notifications,
  - experimental reason propagation,
  - realtime request encoding,
  - approval payloads and account/login serialization.
- [experimental_api.rs tests](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/experimental_api.rs#L58-L172) verify the derive macro over enums, nested fields, vectors, and maps.

### History and projection tests

- [thread_history.rs tests](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L1189-L3100) cover many reducer edge cases:
  - interleaved reasoning chunks,
  - turn abort/complete ordering,
  - rollback truncation,
  - late command completions routed to prior turns,
  - reconstruction of MCP, dynamic-tool, image-generation, and collaboration-agent items,
  - compaction-only turn preservation,
  - ignoring out-of-turn errors that should not fail a turn.

Overall assessment: testing is one of the strongest aspects of the crate.

## Design observations

### Strengths

- Single-source method registry: macro-generated request/notification enums keep method names, payloads, schema export, and experimental flags synchronized.
- Clear layering: v1 compatibility, v2 domain surface, transport envelopes, projection helpers, and schema tooling are cleanly separated.
- Explicit experimental contract: experimental fields are machine-detectable and removable from generated artifacts.
- UI-oriented history model: the crate does not force consumers to replay raw event streams themselves.
- Practical schema ergonomics: the flat v2 bundle exists specifically to accommodate weaker downstream code generators.

### Trade-offs

- `v2.rs` is very large and mixes DTO definitions with conversion logic, helper methods, and tests. That keeps related concepts co-located but makes the file harder to navigate.
- The macro-driven registry is powerful, but it raises the learning curve for contributors because behavior is partly generated rather than locally visible.
- Some compatibility behavior is encoded in export-time filtering and ref-rewriting logic rather than in simpler source-level type structure.

### Notable design patterns

- Declarative protocol registry via macros.
- Adapter layer via extensive `From` / `TryFrom` implementations.
- Reducer pattern for history replay in [ThreadHistoryBuilder](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/thread_history.rs#L85-L156).
- “Projection over truth” design: app-server thread items are synthesized presentation models, not direct core events.
- Generated-artifact verification as a first-class test concern.

## Open questions and follow-up points

The code itself highlights a few unresolved or still-evolving areas:

- `ThreadTokenUsage.model_context_window` is still optional with a TODO to make it required in [v2.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L4114-L4120).
- `CommandExecutionRequestApprovalParams::strip_experimental_fields` hardcodes field stripping and explicitly calls out the lack of a generic outbound compatibility design in [v2.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L6148-L6155).
- MCP elicitation currently cannot correlate back to a specific MCP tool-call item, and the code notes that as a future enhancement in [v2.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L6241-L6244).

Additional review questions suggested by the structure:

- Should `v2.rs` be split by domain (`config`, `thread`, `turn`, `mcp`, `account`, `items`) to improve maintainability without changing the public API?
- Should experimental-field stripping move to a reusable serializer/schema policy instead of bespoke post-processing in [export.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/export.rs#L243-L253) and [v2.rs](file:///Users/bytedance/project/codex/codex-rs/app-server-protocol/src/protocol/v2.rs#L6148-L6155)?
- Is there an opportunity to extract a reusable “protocol schema pipeline” crate, since export logic here is sophisticated enough to be a subsystem on its own?

## Bottom line

`codex-app-server-protocol` is not a passive type-definition crate. It is the boundary-definition and compatibility layer for the app-server ecosystem.

It owns:

- the typed app-server RPC contract,
- the stable/experimental API split,
- the transformation from core event streams into UI-facing thread history,
- the generation and verification of the published JSON/TypeScript schemas.

That makes it foundational infrastructure for every client that talks to the Codex app-server, whether through typed Rust APIs, vendored TypeScript types, or generated JSON Schema consumers.
