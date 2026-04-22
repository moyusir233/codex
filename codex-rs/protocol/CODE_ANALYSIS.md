# codex-protocol crate analysis

## 1. Purpose and scope

The `codex-protocol` crate is the shared schema layer for Codex. Its own README states that it defines protocol “types” used both for internal communication between `codex-core` and `codex-tui` and for external app-server communication, while ideally keeping dependencies and business logic minimal ([README.md](file:///Users/bytedance/project/codex/codex-rs/protocol/README.md#L1-L7)).

In practice, the crate does more than just passive DTO definitions:

- It defines the wire protocol for submissions and events, centered on a submission-queue / event-queue model ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L1-L5), [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L117-L127), [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L1403-L1410)).
- It models assistant responses, tool calls, multimodal inputs/outputs, sandbox policies, approval requests, MCP payloads, and session/model metadata ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L386-L420), [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L832-L1139)).
- It contains compatibility and translation logic: legacy sandbox conversion, legacy event bridging, dynamic-tool field migration, MCP wire-shape adapters, image attachment conversion, and shell-output decoding ([permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L381-L430), [items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L385-L411), [dynamic_tools.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/dynamic_tools.rs#L48-L82), [mcp.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/mcp.rs#L153-L260), [exec_output.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/exec_output.rs#L62-L165)).

## 2. High-level responsibilities

### 2.1 Session protocol and lifecycle

The main session protocol lives in [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs).

Key responsibilities:

- Define inbound client operations in `Op`, including user turns, approvals, realtime conversation control, history access, MCP refresh, skill listing, and review mode interactions ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L392-L649)).
- Define outbound agent events in `EventMsg`, covering turn lifecycle, text/reasoning deltas, tool execution, approval requests, MCP, dynamic tools, patch application, review flows, and session metadata updates ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L1412-L1625)).
- Define shared per-turn/session policy objects such as `AskForApproval`, `GranularApprovalConfig`, `NetworkAccess`, `ReadOnlyAccess`, `SandboxPolicy`, and `WritableRoot` ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L832-L1099)).
- Define operational telemetry and user-facing state like token usage, rate limits, model reroutes, review findings, exec command events, and stream error notifications ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L2051-L2309), [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L3005-L3233)).

### 2.2 Model-facing request/response types

[models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs) is the second major center of gravity.

It is responsible for:

- Modeling input/output items exchanged with model APIs, including `ResponseInputItem`, `ContentItem`, `ResponseItem`, `MessagePhase`, shell-call parameter structs, and function-call output payloads ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L386-L620), [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1337-L1571)).
- Building developer/system-style instruction payloads from sandbox and approval policy state via `DeveloperInstructions` and embedded markdown templates ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L622-L889)).
- Converting user inputs into model content, including remote image tags, local image file loading, placeholder errors, and label sequencing ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1068-L1186), [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1295-L1336)).
- Bridging MCP tool outputs into OpenAI-compatible function-call outputs, including multimodal image preservation and text fallback ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1573-L1710)).

### 2.3 Permission and sandbox modeling

Sandboxing is split across the legacy user-facing `SandboxPolicy` in [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L994-L1139), the richer runtime filesystem/network policy types in [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L23-L258), and request/grant wrappers in [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L147-L385) and [request_permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/request_permissions.rs#L1-L73).

The crate is responsible for:

- Representing filesystem access as explicit entries over paths, glob patterns, and special roots (`root`, `cwd`, `tmpdir`, project roots, etc.) ([permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L40-L147), [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L243-L258)).
- Resolving effective read/write/deny access against a cwd, including carve-outs and precedence handling ([permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L432-L733)).
- Converting between legacy sandbox settings and richer runtime policies, and detecting when direct runtime enforcement is required because the legacy representation is lossy ([permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L319-L430), [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L538-L555), [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L734-L779)).
- Modeling per-command permission requests and profile overrides for shell-like tools (`SandboxPermissions`, `PermissionProfile`, `RequestPermissionProfile`) ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L112-L145), [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L296-L347), [request_permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/request_permissions.rs#L17-L73)).

### 2.4 Approval and review workflows

[approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs) defines the review/approval vocabulary that complements `Op::ExecApproval`, `Op::PatchApproval`, and approval-related events.

Concrete responsibilities:

- Capture exec approval payloads, including parsed command metadata, proposed execpolicy/network amendments, optional extra permissions, and allowed decision sets ([approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L212-L258)).
- Derive backward-compatible default decisions when older senders omit `available_decisions` ([approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L260-L315)).
- Represent guardian-review status and reviewed actions such as shell commands, execve calls, network access, MCP tool calls, permission requests, and patch application ([approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L104-L210)).
- Represent elicitation requests from MCP servers, patch approval requests, and review actions around user-provided structured forms or URLs ([approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L317-L381)).

### 2.5 Turn history and legacy compatibility

[items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs) adds a structured turn-item layer above raw events.

It is responsible for:

- Representing replayable/typed turn content (`TurnItem`) for user messages, agent messages, plans, reasoning, web search, image generation, hook prompts, and context compaction ([items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L25-L37)).
- Converting structured items back to legacy `EventMsg` values for older consumers ([items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L316-L411)).
- Encoding hook prompt fragments as XML-ish inline content inside `ResponseItem::Message`, then parsing them back into structured fragments ([items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L251-L314)).
- Rebasing `TextElement` byte ranges when flattening segmented `UserInput::Text` into legacy text-only message strings ([items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L153-L229)).

Related lightweight persistence/history types live in [message_history.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/message_history.rs#L1-L11) and [memory_citation.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/memory_citation.rs#L1-L20).

### 2.6 Tool schemas and integrations

The crate also owns tool-adjacent protocol types:

- Dynamic tool registration/calls/results in [dynamic_tools.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/dynamic_tools.rs#L8-L82).
- MCP types and lossy wire-shape adapters in [mcp.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/mcp.rs#L11-L151), [mcp.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/mcp.rs#L153-L260).
- `request_user_input` and `request_permissions` tool contracts in [request_user_input.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/request_user_input.rs#L8-L55) and [request_permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/request_permissions.rs#L9-L73).
- Plan/todo tool arguments in [plan_tool.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/plan_tool.rs#L6-L29).
- Shell command parsing summaries for approvals and UI in [parse_command.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/parse_command.rs#L7-L31).

### 2.7 Account, auth, model, and output metadata

Other supporting modules cover:

- Account plan labels and grouping helpers in [account.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/account.rs#L6-L38).
- Auth-facing plan parsing and refresh-token failure modeling in [auth.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/auth.rs#L5-L120).
- Shared model catalog/config metadata in [openai_models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/openai_models.rs#L25-L152), [openai_models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/openai_models.rs#L246-L349), [openai_models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/openai_models.rs#L430-L495).
- Shell output decoding and execution result structs in [exec_output.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/exec_output.rs#L15-L165).
- Error taxonomy and mapping to protocol-facing error codes/events in [error.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/error.rs#L26-L263).
- Network-policy decision payloads imported from the proxy subsystem in [network_policy.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/network_policy.rs#L1-L22).

## 3. Key public APIs and data models

### 3.1 Protocol entrypoints

- `Submission { id, op, trace }` is the root inbound message type ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L117-L137)).
- `Op` is the main request enum. The most important variants for standard agent work are `UserTurn`, `ExecApproval`, `PatchApproval`, `RequestPermissionsResponse`, `DynamicToolResponse`, `AddToHistory`, `ListMcpTools`, and realtime control variants ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L397-L649)).
- `Event { id, msg }` and `EventMsg` are the outbound equivalents ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L1403-L1418)).

### 3.2 Model-facing items

- `UserInput` models segmented text, remote images, local image references, skills, and mentions ([user_input.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/user_input.rs#L9-L40)).
- `ResponseInputItem` is the model request-side abstraction for messages and tool outputs ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L386-L419)).
- `ResponseItem` is the model response-side abstraction, including assistant messages, reasoning, shell calls, function calls, web search calls, image generation calls, compaction notices, and more ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L465-L620)).
- `FunctionCallOutputPayload` is a notable compatibility type: it serializes as either a string or a content-item array, while carrying internal `success` metadata that is intentionally not sent on the wire ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1463-L1571)).

### 3.3 Policy and approval surfaces

- `SandboxPolicy` is the legacy/high-level execution policy exposed to callers ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L994-L1060)).
- `FileSystemSandboxPolicy` / `NetworkSandboxPolicy` are the richer runtime-level permissions used for direct access checks and fine-grained permission profiles ([permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L23-L38), [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L139-L147), [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L275-L733)).
- `ExecApprovalRequestEvent`, `ApplyPatchApprovalRequestEvent`, `GuardianAssessmentEvent`, and `ElicitationRequestEvent` model the major approval/review callbacks ([approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L179-L258), [approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L346-L381)).

### 3.4 Turn-item and replay surfaces

- `TurnItem` is the higher-level structured history item used for replay and modern UI composition ([items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L25-L37)).
- Legacy surfaces remain first-class through `as_legacy_event()` / `as_legacy_events()` helpers on several item types ([items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L135-L145), [items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L161-L170), [items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L326-L411)).

## 4. End-to-end flow encoded by the crate

The crate does not execute turns itself, but it clearly defines the data flow that `codex-core` and clients follow.

### 4.1 Standard user turn flow

1. A client submits `Submission { op: Op::UserTurn { ... } }`, including user items, cwd, model, sandbox policy, approval policy, and optional collaboration/personality overrides ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L436-L491)).
2. `UserInput` values are converted into a `ResponseInputItem::Message`. Remote images are wrapped with image tags; local image paths are loaded, converted into `data:` URLs when possible, or replaced with explanatory text placeholders when reading/decoding fails ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1081-L1186), [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1295-L1336)).
3. Developer guidance is synthesized from sandbox mode, network access, approval policy, collaboration mode, and optional approved prefix rules using `DeveloperInstructions` plus embedded markdown templates ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L667-L889)).
4. Model output arrives as `ResponseItem` values such as assistant messages, reasoning, function calls, web searches, and image generation calls ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L465-L620)).
5. The runtime can project those into `EventMsg` and/or `TurnItem` values for UIs and persistence ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L1418-L1625), [items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L385-L411)).

### 4.2 Tool-output flow

1. Tool results can be represented as `ResponseInputItem::FunctionCallOutput`, `CustomToolCallOutput`, `McpToolCallOutput`, or `ToolSearchOutput` ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L386-L419)).
2. MCP results are normalized through `CallToolResult::as_function_call_output_payload()`, preferring structured content when images are present, otherwise falling back to serialized text/JSON ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1573-L1638)).
3. Function/custom-tool outputs then serialize with wire-compatible shape rules: plain string for text-only output, array for multimodal content ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1545-L1571)).

### 4.3 Approval flow

1. A blocked command or sensitive action is surfaced through `ExecApprovalRequestEvent`, `ApplyPatchApprovalRequestEvent`, `RequestPermissionsEvent`, `RequestUserInputEvent`, or `ElicitationRequestEvent` ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L1522-L1537), [approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L212-L381)).
2. The client answers with `Op::ExecApproval`, `Op::PatchApproval`, `Op::RequestPermissionsResponse`, `Op::UserInputAnswer`, or `Op::ResolveElicitation` ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L559-L617)).
3. `ExecApprovalRequestEvent::effective_available_decisions()` preserves compatibility with older senders by synthesizing default options when the richer decision list is absent ([approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L267-L315)).

### 4.4 History/replay flow

1. Conversation content may be persisted either as lightweight `HistoryEntry` rows or richer `TurnItem` values ([message_history.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/message_history.rs#L6-L11), [items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L25-L37)).
2. `TurnItem` can be lossily projected back into legacy `EventMsg` events for older clients ([items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L399-L410)).
3. Error events explicitly expose whether they should mark a replayed turn as failed via `ErrorEvent::affects_turn_status()` ([protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L2058-L2065)).

## 5. Module-by-module notes

### 5.1 Core protocol surface

- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/lib.rs#L1-L28) exposes `protocol`, `models`, `permissions`, approvals, dynamic tools, MCP, config types, and error/output modules as the crate’s main public API.
- `protocol.rs` is the broadest module and effectively acts as the canonical “wire vocabulary” for Codex sessions.

### 5.2 Policy and permissions

- `protocol.rs` exposes user-facing policy enums.
- `permissions.rs` owns semantic resolution, access checks, deny-glob matching, special-path handling, and legacy conversion.
- `models.rs` wraps those runtime policies into tool-call-facing `PermissionProfile` types.
- `request_permissions.rs` defines the interactive request/response tool surface.

This is one of the crate’s most logic-heavy areas.

### 5.3 Message and multimodal handling

- `user_input.rs` keeps user input segmented and preserves `TextElement` byte ranges.
- `models.rs` handles model content items, local image loading, and multimodal tool output encoding.
- `items.rs` adds replayable turn structure and legacy event adapters.

### 5.4 Tooling integrations

- `dynamic_tools.rs` handles server-defined tools and a legacy field migration from `exposeToContext` to `deferLoading`.
- `mcp.rs` deliberately duplicates MCP-friendly schemas locally so other crates do not need direct dependency on external MCP type crates.
- `parse_command.rs` provides only a lightweight parsed-command summary, suggesting the crate prefers summarized command metadata rather than full shell AST ownership.

### 5.5 Errors and execution output

- `exec_output.rs` is focused and pragmatic: it stores command output and adds robust byte-decoding logic for real-world terminal encodings.
- `error.rs` is large and operationally important; it maps low-level/runtime errors into protocol-visible categories (`CodexErrorInfo`, `ErrorEvent`) and retry behavior.

## 6. Dependency analysis

`Cargo.toml` confirms the crate tries to stay shared and portable but still pulls in a fairly broad set of support crates ([Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/protocol/Cargo.toml#L14-L61)).

### 6.1 Serialization and schema generation

- `serde`, `serde_json`, `serde_with`: wire serialization, aliases, base64 encoding, and compatibility defaults.
- `schemars`, `ts-rs`: JSON Schema and TypeScript type generation across most public types.

These are foundational to the crate’s role as a cross-process and cross-language contract package.

### 6.2 Codex-local shared infrastructure

- `codex-execpolicy`: used while building approval/developer-instruction guidance around approved prefixes ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L38-L39), [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L893-L1032)).
- `codex-network-proxy`: imported for network policy decision payloads ([network_policy.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/network_policy.rs#L1-L22)).
- `codex-utils-absolute-path`: absolute-path normalization and sandbox path resolution appear throughout permissions and approvals.
- `codex-utils-image`: local image attachment loading and validation ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L9-L10), [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1152-L1186)).
- `codex-utils-template`: sandbox/developer instruction prompt rendering ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L45-L56), [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L878-L889)).

### 6.3 Parsing and content handling

- `globset`: deny-glob matching in filesystem sandbox policies ([permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L9-L10), [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L173-L240)).
- `quick-xml`: hook prompt fragment serialization/parsing ([items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L18-L19), [items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs#L251-L314)).
- `chardetng` + `encoding_rs`: smart shell-output decoding ([exec_output.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/exec_output.rs#L1-L12), [exec_output.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/exec_output.rs#L62-L165)).

### 6.4 Runtime-oriented dependencies

- `reqwest`, `tokio`, `chrono`, `uuid`, `tracing`, `thiserror` are used in error modeling, IDs, timestamps, logging, and async/runtime error translation.
- Linux-only `landlock` and `seccompiler` are pulled behind `cfg(target_os = "linux")`, indicating the crate must be able to describe Linux sandbox failures without forcing those dependencies on macOS/Windows builds ([Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/protocol/Cargo.toml#L48-L50)).

## 7. Testing coverage

The crate has meaningful unit-test coverage distributed across inline module tests plus dedicated sidecar test files.

### 7.1 Major covered behaviors

- `models.rs` has extensive tests for:
  - sandbox-permission helpers,
  - MCP image conversion,
  - function-call output serialization shape,
  - image generation parsing,
  - permission-profile round trips,
  - developer instruction composition,
  - granular approval messaging,
  - tool-search items,
  - web search action round trips,
  - local-image placeholder behavior ([models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs#L1730-L3213)).
- `error_tests.rs` exercises protocol error mapping, user-facing sandbox messages, plan-specific usage-limit strings, and retry/rate-limit formatting ([error_tests.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/error_tests.rs#L1-L220)).
- `exec_output_tests.rs` is a focused regression suite for encoding detection across UTF-8, CP1251, CP866, Windows-1252 smart punctuation, Latin-1, and invalid byte fallbacks ([exec_output_tests.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/exec_output_tests.rs#L1-L77)).
- `dynamic_tools.rs` tests legacy `exposeToContext` migration to `deferLoading` ([dynamic_tools.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/dynamic_tools.rs#L84-L139)).
- `approvals.rs` tests guardian action serialization/deserialization shapes ([approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L383-L442)).
- `account.rs` tests wire-name compatibility for usage-based plan types ([account.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/account.rs#L40-L87)).

### 7.2 What the tests suggest about design priorities

The test suite strongly suggests these priorities:

- Backward compatibility matters a lot.
- Wire-format exactness matters a lot.
- Multimodal behavior is first-class, not an afterthought.
- Sandbox/approval prompting is a high-risk contract area.
- Real-world terminal behavior, especially encoding edge cases, is important enough to justify specialized decoding logic and regressions.

## 8. Design observations

### 8.1 Compatibility-first protocol evolution

This crate consistently favors additive evolution over breaking changes:

- many fields use `#[serde(default)]`,
- old and new names are supported with aliases,
- optional fields are used to preserve older payloads,
- there are explicit legacy conversion helpers for events, MCP fields, and sandbox policies.

Examples include `DynamicToolSpec` legacy field inversion, `Op::UserInputAnswer` aliases, MCP camelCase/snake_case adapters, and `ExecApprovalRequestEvent` fallback decisions ([dynamic_tools.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/dynamic_tools.rs#L48-L82), [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs#L594-L609), [mcp.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/mcp.rs#L177-L260), [approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs#L267-L315)).

### 8.2 The crate is not “just types”

Even though the README says to avoid material business logic, several modules clearly contain meaningful behavior:

- permission resolution and sandbox conversion,
- developer-instruction rendering,
- image attachment processing,
- shell-output decoding,
- token-usage aggregation and context-window calculations.

This logic is reasonable for a shared contract crate because it keeps serialization-adjacent semantics centralized, but it is a notable architectural drift from the README ideal.

### 8.3 Strong cross-language contract posture

The pervasive use of `schemars` and `ts-rs` shows the crate is intended to serve Rust, frontend, and server consumers simultaneously. The code is written to make contract generation practical, not just Rust ergonomics.

### 8.4 Layered but partially overlapping abstractions

The crate has three related but distinct representation layers:

- protocol/session layer in `protocol.rs`,
- model/provider layer in `models.rs`,
- replay/history layer in `items.rs`.

That split is helpful, but there is some unavoidable overlap. For example, both `EventMsg::AgentMessage` and `TurnItem::AgentMessage` exist, and both `SandboxPolicy` and `FileSystemSandboxPolicy`/`NetworkSandboxPolicy` exist because of legacy vs runtime needs.

## 9. Open questions

These are the main questions I would raise after reading the crate:

1. **Should the crate still be considered “minimal-business-logic”?**  
   The README says yes, but `permissions.rs`, `models.rs`, and `exec_output.rs` now contain substantial operational logic. If this is intentional, the README may be underselling the crate’s architectural role.

2. **What is the long-term deprecation plan for legacy event and sandbox shapes?**  
   There is active support for legacy `SandboxPolicy`, legacy event projection, alias-heavy request variants, and fallback decision synthesis. That is useful today, but the crate would benefit from a clearly documented retirement path.

3. **Why are there two plan-type models?**  
   [account.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/account.rs#L6-L38) and [auth.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/auth.rs#L5-L50) both model plan-like concepts with overlapping value sets. They appear to serve different consumers, but the duplication increases cognitive load.

4. **Is legacy sandbox bridging still a permanent requirement?**  
   `FileSystemSandboxPolicy::needs_direct_runtime_enforcement()` explicitly detects when the richer runtime representation cannot be faithfully represented as the legacy sandbox model ([permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs#L538-L555)). That suggests the runtime model is already more expressive than the historic contract.

5. **Should `protocol.rs` be split further?**  
   It currently owns session ops, event vocabulary, sandbox policy, token/rate-limit types, review types, realtime types, and skill metadata. It works, but it is large enough that discoverability is becoming difficult.

## 10. Bottom line

`codex-protocol` is the contract backbone of the Codex system. It is not merely a bag of structs: it centralizes wire formats, compatibility strategy, multimodal content representation, sandbox/approval semantics, shared model metadata, and replay/history bridges. The crate is carefully engineered around stability and interoperability, with the heaviest complexity concentrated in:

- [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs),
- [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs),
- [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs),
- [approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs),
- [openai_models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/openai_models.rs),
- [items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs).

If another engineer needs to understand the crate quickly, I would start in this order:

1. [README.md](file:///Users/bytedance/project/codex/codex-rs/protocol/README.md)
2. [lib.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/lib.rs)
3. [protocol.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/protocol.rs)
4. [models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/models.rs)
5. [permissions.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/permissions.rs)
6. [items.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/items.rs)
7. [approvals.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/approvals.rs)
8. [openai_models.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/openai_models.rs)
