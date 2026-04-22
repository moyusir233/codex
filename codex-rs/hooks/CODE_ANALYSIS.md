# codex-hooks crate analysis

## Scope and purpose

The `codex-hooks` crate implements two related hook systems:

- A current Claude/Codex command-hook engine that discovers `hooks.json` files from config layers, previews matching handlers, executes external commands, parses their outputs, and converts them into protocol-facing hook run events and control decisions. The public crate surface is re-exported from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/lib.rs#L1-L37).
- A smaller legacy hook path that still supports fire-and-forget notification commands for historical `after_agent` payloads via [registry.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/registry.rs#L23-L92) and [legacy_notify.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/legacy_notify.rs#L28-L73).

At a high level, the modern path is:

1. Build `Hooks` with `HooksConfig`.
2. Preload generated JSON schemas and discover handlers from config-layer `hooks.json` files.
3. On each event, preview matching handlers for UI rendering.
4. Serialize event-specific JSON input to the hook process `stdin`.
5. Run matching commands, parse `stdout`/`stderr` plus exit codes, and return both protocol-visible hook events and event-specific control outcomes.

## Module responsibilities

### Public surface and orchestration

- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/lib.rs#L1-L37) is mostly a re-export layer. It exposes event request/outcome types, `Hooks`, legacy notify helpers, schema fixture generation, and legacy `Hook`/`HookPayload` types.
- [registry.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/registry.rs#L23-L169) is the main public entry point. `Hooks::new` wires legacy hooks plus the modern engine, `startup_warnings` exposes discovery-time warnings, and `preview_*` / `run_*` forward into the engine.
- [engine/mod.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/mod.rs#L26-L172) defines `CommandShell`, the normalized `ConfiguredHandler` type, and `ClaudeHooksEngine`, which owns discovered handlers, warnings, and shell configuration.

### Discovery and configuration

- [engine/config.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/config.rs#L1-L50) defines the JSON shape of `hooks.json`: event buckets, matcher groups, and handler variants.
- [engine/discovery.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/discovery.rs#L20-L180) walks config layers from lowest precedence first, reads `hooks.json`, validates matchers for supported events, normalizes timeouts, maps config-layer source metadata into protocol `HookSource`, and emits warnings instead of hard errors for bad configs or unsupported handler types.

### Execution pipeline

- [engine/dispatcher.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/dispatcher.rs#L25-L119) provides reusable pieces:
  - `select_handlers` filters by event and matcher.
  - `running_summary` and `completed_summary` build protocol summaries.
  - `execute_handlers` runs all selected handlers and feeds their results through an event-specific parser callback.
- [engine/command_runner.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/command_runner.rs#L24-L135) runs each hook command in a shell, writes JSON input to `stdin`, captures `stdout`/`stderr`, enforces per-handler timeouts, and records timestamps and duration.
- [engine/output_parser.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/output_parser.rs#L75-L233) parses successful JSON outputs into event-specific internal structs. It also rejects unsupported or reserved fields such as `updatedInput`, `updatedPermissions`, `updatedMCPToolOutput`, and invalid block decisions via validations in [output_parser.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/output_parser.rs#L261-L417).

### Event handlers

- [events/pre_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/pre_tool_use.rs#L20-L239) decides whether a tool call should be blocked before execution.
- [events/permission_request.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/permission_request.rs#L1-L286) participates in the approval path and resolves multiple allow/deny votes conservatively.
- [events/post_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/post_tool_use.rs#L22-L301) can add model context, produce feedback, block normal processing, or stop the turn after tool execution.
- [events/session_start.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/session_start.rs#L20-L220) runs on startup/resume/clear and can add context or stop further processing.
- [events/user_prompt_submit.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/user_prompt_submit.rs#L21-L240) runs when a user submits a prompt and can add context or stop/block the turn.
- [events/stop.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/stop.rs#L22-L309) runs in the stop path and can either stop cleanly or block with continuation prompt fragments.
- [events/common.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/common.rs#L11-L134) centralizes matcher semantics, context aggregation, tool-use run-id rewriting, and synthetic failure-event generation.

### Schema generation and loading

- [schema.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/schema.rs#L29-L615) defines the wire-format input/output structs and generates canonical JSON Schema fixtures for all hook events.
- [engine/schema_loader.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/schema_loader.rs#L21-L78) embeds the generated schema JSON with `include_str!` and validates it on startup through a `OnceLock`.
- [bin/write_hooks_schema_fixtures.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/bin/write_hooks_schema_fixtures.rs#L1-L9) is the small utility binary that regenerates schema fixtures under `schema/generated/`.

### Legacy hook model

- [types.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/types.rs#L13-L158) defines the older generic `Hook`, `HookPayload`, `HookEvent`, and `HookResult` model still used by the legacy dispatch path.
- [legacy_notify.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/legacy_notify.rs#L12-L73) converts `AfterAgent` payloads into the historical JSON shape and spawns a fire-and-forget process.

## Public APIs and important types

### Main entry points

- `HooksConfig` in [registry.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/registry.rs#L23-L30) controls legacy notify argv, feature enablement, config-layer stack, and shell configuration.
- `Hooks::new`, `startup_warnings`, `dispatch`, `preview_*`, and `run_*` in [registry.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/registry.rs#L45-L169) form the primary API surface for consumers.

### Modern event request/outcome types

- `PreToolUseRequest` / `PreToolUseOutcome` in [pre_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/pre_tool_use.rs#L20-L38)
- `PermissionRequestRequest` / `PermissionRequestOutcome` / `PermissionRequestDecision` in [permission_request.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/permission_request.rs#L34-L58)
- `PostToolUseRequest` / `PostToolUseOutcome` in [post_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/post_tool_use.rs#L22-L43)
- `SessionStartRequest` / `SessionStartOutcome` / `SessionStartSource` in [session_start.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/session_start.rs#L20-L53)
- `UserPromptSubmitRequest` / `UserPromptSubmitOutcome` in [user_prompt_submit.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/user_prompt_submit.rs#L21-L38)
- `StopRequest` / `StopOutcome` in [stop.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/stop.rs#L22-L42)

### Legacy APIs

- `Hook`, `HookPayload`, `HookEvent`, and `HookResult` in [types.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/types.rs#L13-L158)
- `notify_hook` and `legacy_notify_json` in [legacy_notify.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/legacy_notify.rs#L28-L73)

## End-to-end flow

### 1. Engine construction

- `Hooks::new` creates a `ClaudeHooksEngine` only when `feature_enabled` is true; otherwise the engine contains no handlers and no warnings ([registry.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/registry.rs#L45-L65), [engine/mod.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/mod.rs#L73-L94)).
- During construction, the engine eagerly loads generated schemas with `schema_loader::generated_hook_schemas()` and discovers handlers from the config-layer stack ([engine/mod.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/mod.rs#L87-L93)).

### 2. Handler discovery

- Discovery scans each enabled config layer’s `config_folder()/hooks.json`, parses it into `HooksFile`, and converts each supported command hook into a `ConfiguredHandler` carrying event name, matcher, source path, source kind, timeout, status message, and global display order ([discovery.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/discovery.rs#L20-L110), [engine/mod.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/mod.rs#L32-L64)).
- Unsupported hook kinds (`prompt`, `agent`, async command hooks) are skipped with warnings rather than failing discovery ([discovery.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/discovery.rs#L134-L177)).

### 3. Preview stage

- Every event module exposes a `preview` function that calls `dispatcher::select_handlers` and converts the matches to `HookRunSummary` rows for the UI ([session_start.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/session_start.rs#L62-L74), [pre_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/pre_tool_use.rs#L46-L60)).
- Tool-scoped events append the tool-use id or approval-path suffix to the run id using `common::hook_run_for_tool_use` so preview ids match final completion ids ([events/common.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/common.rs#L73-L96)).

### 4. Request serialization

- Each event builds an event-specific command input struct from [schema.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/schema.rs#L223-L430), serializes it to JSON, and treats serialization failures as synthetic failed hook events instead of process execution failures.
- `session_start`, `user_prompt_submit`, and `stop` serialize the natural event payload directly.
- `permission_request` preserves the actual tool name via `build_command_input` ([permission_request.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/permission_request.rs#L168-L183)).
- `pre_tool_use` and `post_tool_use` currently serialize `tool_name: "Bash"` regardless of `request.tool_name` ([pre_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/pre_tool_use.rs#L80-L93), [post_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/post_tool_use.rs#L89-L103)).

### 5. Command execution

- `dispatcher::execute_handlers` starts all selected handlers with `join_all`, so matching hooks run concurrently, not sequentially ([dispatcher.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/dispatcher.rs#L65-L85)).
- `run_command` invokes either a custom shell (`shell_program` + args) or the platform default shell (`$SHELL -lc` on Unix), writes the JSON payload to `stdin`, captures both streams, and reports timeout/process errors as structured `CommandRunResult` values ([command_runner.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/command_runner.rs#L24-L135)).

### 6. Output parsing and aggregation

- Each event module uses an event-specific `parse_completed` function to translate `CommandRunResult` into:
  - protocol-visible `HookCompletedEvent`
  - event-specific internal data used to produce final outcomes
- Exit code semantics are event-specific:
  - `0` means inspect `stdout`; structured JSON may create stop/block/context/decision data.
  - `2` has special meaning for several events and usually interprets `stderr` as a block/deny/feedback reason.
  - any other code becomes a failed hook event.
- Aggregation differs by event:
  - `PreToolUse`: any blocking hook blocks the tool call; first block reason wins ([pre_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/pre_tool_use.rs#L116-L130)).
  - `PermissionRequest`: any deny wins immediately, otherwise the last allow wins ([permission_request.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/permission_request.rs#L127-L166)).
  - `PostToolUse`, `SessionStart`, `UserPromptSubmit`: flatten additional contexts and stop if any handler returns `continue:false` ([post_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/post_tool_use.rs#L126-L153), [session_start.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/session_start.rs#L124-L139), [user_prompt_submit.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/user_prompt_submit.rs#L110-L125)).
  - `Stop`: `continue:false` overrides block behavior, otherwise multiple block prompts are joined and converted into `HookPromptFragment`s ([stop.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/stop.rs#L265-L299)).

## Event-by-event behavior

### SessionStart

- Matchers apply to the source string: `startup`, `resume`, or `clear` ([session_start.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/session_start.rs#L20-L45)).
- Plain non-JSON `stdout` is accepted as additional context, which is more permissive than most other events ([session_start.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/session_start.rs#L192-L206)).

### PreToolUse

- Intended to gate tool execution before the command runs.
- Supports structured block decisions from JSON output and a legacy exit-code-2-plus-stderr path ([pre_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/pre_tool_use.rs#L133-L239)).
- Rejects several reserved or unsupported fields through the output parser instead of silently ignoring them ([output_parser.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/output_parser.rs#L86-L132), [output_parser.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/output_parser.rs#L337-L407)).

### PermissionRequest

- Runs later in the approval path and focuses on allow/deny verdicts, not input rewriting ([permission_request.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/permission_request.rs#L1-L15)).
- Rejects reserved mutation fields such as `updatedInput` and `updatedPermissions` ([output_parser.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/output_parser.rs#L134-L155), [output_parser.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/output_parser.rs#L297-L325)).

### PostToolUse

- Can add context, emit feedback, stop processing, or return a block-style feedback result after tool execution ([post_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/post_tool_use.rs#L156-L291)).
- Plain text `stdout` is ignored rather than treated as context; only valid JSON has semantics ([post_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/post_tool_use.rs#L177-L245)).

### UserPromptSubmit

- Matchers are ignored for this event by design; all configured handlers run ([events/common.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/common.rs#L98-L108), [dispatcher.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/dispatcher.rs#L25-L44)).
- Plain text `stdout` becomes additional context, while `decision:block` or exit code `2` stop the turn ([user_prompt_submit.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/user_prompt_submit.rs#L128-L240)).

### Stop

- Stop hooks can either stop cleanly (`continue:false`) or block completion with continuation prompt fragments ([stop.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/stop.rs#L124-L299)).
- Multiple blocking hooks are merged into a joined `block_reason` and a list of `HookPromptFragment`s, unless a stop decision exists, which takes precedence ([stop.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/stop.rs#L265-L299)).

## Matcher and precedence rules

- Only `PreToolUse`, `PermissionRequest`, `PostToolUse`, and `SessionStart` honor `matcher`; `UserPromptSubmit` and `Stop` ignore it ([events/common.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/common.rs#L98-L108)).
- Matchers are regexes except that `""` and `"*"` both mean “match all” ([events/common.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/common.rs#L111-L134)).
- Discovery preserves a global `display_order` across config layers and file order, which becomes the protocol-visible run order and the fold order for decisions ([discovery.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/discovery.rs#L28-L30), [discovery.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/discovery.rs#L156-L167)).

## Wire format and schemas

- The wire schema is deliberately explicit and narrow. The input/output structs deny unknown fields and encode event names as constants or enums where possible ([schema.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/schema.rs#L56-L125), [schema.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/schema.rs#L223-L430)).
- `write_schema_fixtures` rewrites the generated schema directory from scratch and canonicalizes JSON key ordering to keep fixtures stable ([schema.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/schema.rs#L433-L538)).
- The engine does not generate schemas lazily at runtime; instead it embeds checked-in schema JSON and validates that the embedded fixtures still parse ([schema_loader.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/schema_loader.rs#L21-L78)).

## Dependencies

`Cargo.toml` shows a focused dependency set ([Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/hooks/Cargo.toml#L1-L30)):

- `codex-config`: supplies config-layer stacking and source metadata used by discovery.
- `codex-protocol`: supplies hook protocol enums, run summaries, completed events, and continuation fragments.
- `codex-utils-absolute-path`: gives strongly typed absolute paths for source and cwd fields.
- `tokio`: provides async process spawning, I/O, timers, and the async test runtime.
- `futures`: used for `join_all` in concurrent hook execution.
- `serde`, `serde_json`, `schemars`: power wire structs, JSON parsing, and JSON Schema generation.
- `regex`: validates and evaluates matchers.
- `chrono`: timestamps hook run summaries.
- `anyhow`: used mostly for schema-fixture generation utilities.

There are no crate-local feature flags in this `Cargo.toml`.

## Testing coverage

The crate relies heavily on inline unit tests across modules:

- Discovery tests verify matcher validation, match-all behavior, precedence metadata, and source mapping ([discovery.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/discovery.rs#L196-L385)).
- Dispatcher tests verify event filtering, matcher semantics, duplicate preservation, and declaration order ([dispatcher.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/dispatcher.rs#L122-L343)).
- Output parser tests check rejection of reserved permission-request fields ([output_parser.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/output_parser.rs#L419-L493)).
- Event tests cover block/stop/context semantics, run-id rewriting, and aggregation behavior:
  - [pre_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/pre_tool_use.rs#L241-L547)
  - [post_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/post_tool_use.rs#L304-L559)
  - [permission_request.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/permission_request.rs#L288-L331)
  - [session_start.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/session_start.rs#L249-L380)
  - [stop.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/stop.rs#L312-L547)
  - [user_prompt_submit.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/user_prompt_submit.rs#L270-L419)
- Schema tests regenerate fixtures into a temp directory and compare them with checked-in output ([schema.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/schema.rs#L617-L716)).
- Legacy serialization tests keep the historical payload shape stable ([legacy_notify.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/legacy_notify.rs#L75-L149), [types.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/types.rs#L160-L292)).

I also ran `cargo test -p codex-hooks --quiet` from the workspace root; the crate currently passes all 67 tests.

## Design observations

- The crate is intentionally split into small event modules but shares a strong common pipeline: discovery -> preview -> serialize -> execute -> parse -> aggregate.
- Protocol output is treated as a first-class product. Every hook run becomes a `HookRunSummary` / `HookCompletedEvent`, even for serialization failures and invalid hook output.
- The parser is conservative about unsupported fields. Instead of silently accepting forward-looking fields, it marks the hook run as failed and discards the behavioral effect.
- Discovery is tolerant of bad configuration: invalid configs generate warnings, while the rest of the system still starts.
- The legacy hook model and modern command-hook model coexist in the same crate, which eases migration but also leaves some surface area that is no longer central.

## Open questions

1. Why do `PreToolUse` and `PostToolUse` serialize `tool_name: "Bash"` instead of `request.tool_name`? Matcher selection uses the real tool name, but the hook input payload and schema are Bash-only today ([pre_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/pre_tool_use.rs#L80-L93), [post_tool_use.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/post_tool_use.rs#L89-L103), [schema.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/schema.rs#L548-L566)).
2. `execute_handlers` runs matching hooks concurrently with `join_all`, while comments in some event modules talk about precedence order. Aggregation still preserves logical precedence because results are zipped back in handler order, but side effects and wall-clock execution order are concurrent ([dispatcher.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/dispatcher.rs#L65-L85), [permission_request.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/events/permission_request.rs#L8-L15)).
3. `prompt`, `agent`, and async command hooks are parsed from config but explicitly skipped as unsupported. Is support planned, or should the config schema be narrowed to reduce user confusion? ([engine/config.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/config.rs#L33-L50), [discovery.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/engine/discovery.rs#L134-L177)).
4. `src/user_notification.rs` appears to be an unused near-duplicate of [legacy_notify.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/legacy_notify.rs#L1-L153). It is not referenced from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/lib.rs#L1-L37) or [registry.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/registry.rs#L1-L169).
5. `Hooks` still carries an `after_tool_use` legacy hook vector, but `Hooks::new` never populates it, so only `after_agent` legacy hooks are actually wired today ([registry.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/registry.rs#L33-L65), [registry.rs](file:///Users/bytedance/project/codex/codex-rs/hooks/src/registry.rs#L72-L92)).
