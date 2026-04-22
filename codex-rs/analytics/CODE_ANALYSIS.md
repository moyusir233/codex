# codex-analytics crate analysis

## Scope and purpose

`codex-analytics` is a telemetry translation and delivery crate for the Codex Rust workspace. It accepts a mix of:

- app-server protocol lifecycle facts (`Initialize`, request/response pairs, notifications),
- higher-level runtime facts emitted directly by core/session code,
- plugin/app/hook/subagent usage events,
- and turn/guardian/compaction analytics data that do not map 1:1 to JSON-RPC.

Its main job is to turn those inputs into stable analytics event payloads and send them to the remote analytics endpoint.

At a high level, the crate splits into three layers:

- `client.rs`: public ingestion API and async delivery queue.
- `reducer.rs`: stateful reducer that correlates low-level protocol events into final analytics events.
- `events.rs` / `facts.rs`: schema and domain-model layer that defines inputs and outbound event payloads.

Primary entrypoint: [AnalyticsEventsClient](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L49-L297)

Manifest: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/analytics/Cargo.toml#L1-L33)

## Crate layout

- [lib.rs](file:///Users/bytedance/project/codex/codex-rs/analytics/src/lib.rs#L1-L51)
  - Re-exports the public client plus most public analytics fact/event types.
  - Keeps `reducer` internal.
  - Exposes `now_unix_seconds()` as a small shared utility.
- [client.rs](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L42-L350)
  - Owns the bounded async queue.
  - Provides the user-facing tracking methods.
  - Starts a background task that reduces facts and POSTs event batches.
- [facts.rs](file:///Users/bytedance/project/codex/codex-rs/analytics/src/facts.rs#L29-L351)
  - Defines input-side domain facts and enums.
  - Models the semantic events the rest of the system reports to analytics.
- [events.rs](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L35-L616)
  - Defines serialized outbound analytics payloads.
  - Contains mapping helpers from internal facts to wire schema.
- [reducer.rs](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L72-L1052)
  - Correlates request/response/notification streams.
  - Builds final event payloads only when enough lifecycle state exists.
- [analytics_client_tests.rs](file:///Users/bytedance/project/codex/codex-rs/analytics/src/analytics_client_tests.rs#L1-L2411)
  - Comprehensive unit/integration-style test file covering serialization and reducer behavior.
- [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/analytics/BUILD.bazel#L1-L6)
  - Minimal Bazel wrapper; the Rust crate name remains `codex_analytics`.

## Concrete responsibilities

### 1. Provide a single analytics ingestion façade

[AnalyticsEventsClient](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L49-L297) is the public API the rest of the workspace uses. It exposes coarse tracking methods such as:

- `track_initialize`
- `track_request`
- `track_response`
- `track_error_response`
- `track_notification`
- `track_turn_resolved_config`
- `track_turn_token_usage`
- `track_compaction`
- `track_guardian_review`
- `track_skill_invocations`
- `track_app_mentioned`
- `track_app_used`
- `track_hook_run`
- `track_plugin_used`
- plugin state methods (`track_plugin_installed`, `track_plugin_uninstalled`, `track_plugin_enabled`, `track_plugin_disabled`)

These methods do not construct HTTP requests directly. They convert call-site input into `AnalyticsFact` values and enqueue them.

### 2. Correlate multi-step protocol flows into one event

The reducer is where most of the crate’s domain logic lives. A turn analytics event is only emitted after enough state has been collected from different sources:

- `TurnStart` request gives `thread_id` and image count.
- `TurnStart` response gives `turn_id` and connection binding.
- custom `TurnResolvedConfigFact` provides resolved model/sandbox/policy/config state.
- `TurnStarted` notification may provide `started_at`.
- `TurnCompleted` notification provides terminal status/error/timing.
- custom `TurnTokenUsageFact` optionally augments usage counters.

That logic is implemented in [AnalyticsReducer::maybe_emit_turn_event](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L805-L859).

### 3. Normalize internal domain models into wire payloads

`events.rs` defines the serialized outbound request types such as:

- [TrackEventRequest](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L48-L65)
- [ThreadInitializedEvent](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L102-L120)
- [GuardianReviewEventRequest](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L122-L244)
- [CodexCompactionEventRequest](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L285-L312)
- [CodexTurnEventRequest](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L314-L368)
- [CodexTurnSteerEventRequest](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L370-L390)

Helper functions such as [codex_app_metadata](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L433-L446), [codex_plugin_metadata](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L448-L469), and [codex_compaction_event_params](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L471-L500) isolate schema-shaping logic from reducer state transitions.

### 4. Deliver events asynchronously with best-effort semantics

[AnalyticsEventsQueue::new](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L55-L71) creates a Tokio channel and spawns a background task. Each received fact is reduced into zero or more `TrackEventRequest` values and then sent with [send_track_events](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L299-L350).

Operational behavior:

- queue is bounded to 256 facts,
- backpressure is handled by drop-on-full with a warning,
- analytics can be globally disabled by `analytics_enabled == Some(false)`,
- delivery only proceeds for ChatGPT auth,
- failures are logged but never surfaced back to the caller.

### 5. Deduplicate some high-volume usage events

[AnalyticsEventsQueue](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L42-L47) stores two `HashSet`s guarded by `Mutex`:

- app usage dedupe key: `(turn_id, connector_id)` via [should_enqueue_app_used](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L80-L97)
- plugin usage dedupe key: `(turn_id, plugin_id)` via [should_enqueue_plugin_used](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L98-L111)

Both sets are hard-capped at 4096 entries and clear wholesale when the cap is reached.

## Public API surface

The library re-exports the client and a large set of enums/structs from [lib.rs](file:///Users/bytedance/project/codex/codex-rs/analytics/src/lib.rs#L9-L42). From an external crate’s perspective, the public contract is:

- `AnalyticsEventsClient` for reporting analytics facts.
- strongly typed event/fact input structs such as `TrackEventsContext`, `TurnResolvedConfigFact`, `TurnTokenUsageFact`, `CodexCompactionEvent`, `SubAgentThreadStartedInput`, `SkillInvocation`, and `GuardianReviewEventParams`.
- enums for lifecycle/telemetry classification, such as `TurnStatus`, `InvocationType`, `CompactionStatus`, `CompactionReason`, `ThreadInitializationMode`, and guardian-related enums.

Important non-public boundary:

- `AnalyticsFact`, `CustomAnalyticsFact`, `TrackEventRequest`, and the reducer are crate-private, which keeps external call sites from depending on reducer internals or wire-shape details directly.

## End-to-end flow

### Typical app-server turn flow

1. App-server creates an [AnalyticsEventsClient](file:///Users/bytedance/project/codex/codex-rs/app-server/src/message_processor.rs#L252-L302).
2. App-server forwards low-level protocol lifecycle events using:
   - [track_initialize](file:///Users/bytedance/project/codex/codex-rs/app-server/src/message_processor.rs#L685-L685)
   - [track_request](file:///Users/bytedance/project/codex/codex-rs/app-server/src/message_processor.rs#L763-L763)
   - `track_response` in `codex_message_processor.rs`
   - `track_notification` in `bespoke_event_handling.rs`
3. Core/session code reports higher-level turn metadata through [track_turn_resolved_config](file:///Users/bytedance/project/codex/codex-rs/core/src/session/turn.rs#L711-L737).
4. Core/task code reports token usage via [track_turn_token_usage](file:///Users/bytedance/project/codex/codex-rs/core/src/tasks/mod.rs#L516-L522).
5. The reducer merges those fragments into a single `codex_turn_event`.
6. The background sender POSTs the batched event list to `/codex/analytics-events/events`.

### Other event flows

- Subagent thread start bypasses normal initialize/response correlation and emits directly through [subagent_thread_started_event_request](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L562-L589).
- Compaction is emitted from core through [track_compaction](file:///Users/bytedance/project/codex/codex-rs/core/src/compact.rs#L334-L364), but reducer enrichment still depends on cached thread/connection metadata.
- Skill invocation is reduced asynchronously because repo URL detection uses async git helpers in [ingest_skill_invoked](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L383-L429).
- Guardian review events are custom facts that require prior thread/connection metadata to attach app-server and runtime info.

## Key module analysis

### client.rs

Key types:

- [AnalyticsEventsQueue](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L42-L47)
- [AnalyticsEventsClient](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L49-L53)

Notable behavior:

- [record_fact](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L265-L270) is the universal gate for all track methods.
- `analytics_enabled: Option<bool>` treats `Some(false)` as hard opt-out and any other value as enabled.
- [send_track_events](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L299-L350) authenticates through `AuthManager`, requires ChatGPT auth, and adds FedRAMP header when appropriate.
- Network errors and non-2xx responses only produce `tracing::warn!`; there is no retry, persistence, or acknowledgement path.

Design implication:

- Callers get a very low-friction telemetry API, but also no delivery guarantees and no visibility into dropped or rejected events.

### facts.rs

This file defines the semantic input model. The important design choice is that analytics facts are more expressive than raw protocol messages.

Examples:

- [TurnResolvedConfigFact](file:///Users/bytedance/project/codex/codex-rs/analytics/src/facts.rs#L55-L75) captures resolved config that the app-server protocol does not provide in one place.
- [TurnTokenUsageFact](file:///Users/bytedance/project/codex/codex-rs/analytics/src/facts.rs#L85-L90) carries per-turn usage deltas from core task accounting.
- [AnalyticsFact](file:///Users/bytedance/project/codex/codex-rs/analytics/src/facts.rs#L265-L293) separates protocol-surface facts from crate-specific custom facts.
- [CustomAnalyticsFact](file:///Users/bytedance/project/codex/codex-rs/analytics/src/facts.rs#L295-L307) is the escape hatch for telemetry that has no natural app-server protocol counterpart.

This is a strong design choice: it prevents forcing analytics requirements back into protocol definitions prematurely.

### events.rs

This file is effectively the crate’s analytics schema adapter.

Highlights:

- All outbound payloads derive `Serialize`.
- Enum shapes are carefully controlled with `rename_all`, `untagged`, and `tag = "type"` where needed.
- Runtime metadata is produced centrally by [current_runtime_metadata](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L552-L560).
- Hook source/status normalization is localized in [analytics_hook_source](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L539-L550) and [analytics_hook_status](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L610-L616).
- Subagent metadata derivation is localized in [subagent_source_name](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L591-L599) and [subagent_parent_thread_id](file:///Users/bytedance/project/codex/codex-rs/analytics/src/events.rs#L601-L608).

Two embedded schema gaps are explicitly documented:

- turn submission type is not fully plumbed yet,
- tool-call accounting fields in `CodexTurnEventParams` are reserved but currently always `None`.

### reducer.rs

This is the core of the crate.

Internal state:

- `requests`: pending `TurnStart` and `TurnSteer` requests keyed by `(connection_id, request_id)`.
- `turns`: partially assembled turn lifecycle state keyed by `turn_id`.
- `connections`: per-connection client/runtime metadata.
- `thread_connections`: mapping from `thread_id` to owning `connection_id`.
- `thread_metadata`: thread lifecycle metadata derived from session source and initialization mode.

The reducer is intentionally tolerant of partial or reordered inputs. Instead of assuming a strict stream order, it accumulates enough information and emits only when prerequisites exist.

Important functions:

- [ingest](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L156-L234): top-level dispatcher
- [emit_thread_initialized](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L663-L699): caches thread/connection linkage and emits thread initialization
- [maybe_emit_turn_event](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L805-L859): final turn correlation gate
- [emit_turn_steer_event](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L767-L803): accepted/rejected turn-steer emission
- [skill_id_for_local_skill](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L1012-L1028): stable hashed identifier for local skills
- [normalize_path_for_skill_id](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L1030-L1052): path normalization rules for repo-scoped vs personal/system/admin skills

One especially useful detail: the turn state is deleted after event emission in [maybe_emit_turn_event](file:///Users/bytedance/project/codex/codex-rs/analytics/src/reducer.rs#L846-L858), which prevents duplicate turn events and bounds memory for completed turns.

## Dependency analysis

Direct dependencies from [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/analytics/Cargo.toml#L15-L33):

- `codex-app-server-protocol`
  - Supplies the protocol request/response/notification types that seed reducer state.
- `codex-git-utils`
  - Used by skill analytics to resolve repo root and repository URL.
- `codex-login`
  - Provides `AuthManager`, HTTP client creation, originator metadata, and auth header generation.
- `codex-plugin`
  - Supplies plugin telemetry metadata and connector capability summaries.
- `codex-protocol`
  - Provides shared config/approval/sandbox/session/domain enums used in analytics facts.
- `os_info`
  - Used for runtime OS version in event metadata.
- `serde` / `serde_json`
  - Event serialization and test assertions.
- `sha1`
  - Stable skill ID hashing.
- `tokio`
  - Async queue and spawned worker.
- `tracing`
  - Drop/failure logging.

Dev-only:

- `codex-utils-absolute-path`
  - Test path fixtures.
- `pretty_assertions`
  - Readable equality diffs.

Architecturally, the dependency direction makes sense: this crate sits near the boundary between protocol/core state and remote telemetry transport, so it depends broadly on shared domain crates but exposes a narrow API upward.

## Testing coverage

The test strategy is concentrated in [analytics_client_tests.rs](file:///Users/bytedance/project/codex/codex-rs/analytics/src/analytics_client_tests.rs#L1-L2411). The file mixes pure serialization tests with reducer state-machine tests.

Covered areas:

- schema serialization for app, plugin, hook, thread, compaction, guardian, and turn events
- dedupe rules for app-used and plugin-used events
- path normalization and skill ID generation inputs
- thread initialization behavior with and without prior `Initialize`
- full turn lifecycle emission
- turn failure/interruption variants
- accepted and rejected turn-steer handling
- steer count accumulation across multiple accepted/rejected steer attempts
- subagent thread-start variants and parent-thread derivation
- plugin state change event mapping

What the tests validate well:

- outbound JSON shape is stable,
- reducer correlation rules work across multi-step lifecycles,
- missing prerequisites suppress emission,
- error-type mapping for turn steer rejection is correct.

What is notably not covered here:

- real HTTP delivery behavior in [send_track_events](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L299-L350),
- queue overflow behavior beyond the single warning path,
- auth-mode permutations outside simple guard logic,
- memory growth/lifetime behavior of long-lived `thread_connections` and `thread_metadata` caches.

## Design assessment

### Strengths

- Clear separation of concerns between ingestion, reduction, schema shaping, and transport.
- Strong typed boundaries for both inputs and serialized outputs.
- Reducer design handles fragmented lifecycle inputs without leaking protocol complexity to call sites.
- Public API is ergonomic: callers report intent-level facts rather than raw JSON blobs.
- Tests are extensive and focus on externally meaningful behavior.

### Trade-offs and constraints

- Delivery is explicitly best-effort; dropped queue entries and network failures are not recoverable.
- Event emission often depends on prior metadata caches, so events like compaction and guardian review are dropped if thread/connection lifecycle was not observed first.
- Reducer stores thread-level metadata indefinitely; unlike `turns`, these maps are not obviously pruned.
- Some analytics schema fields are placeholders and intentionally remain unset today.
- Skill invocation enrichment performs filesystem and git work during reduction, which is more expensive than the other mostly in-memory paths.

## Notable design patterns

- State machine reducer: protocol fragments are assembled over time before final event emission.
- Internal DTO split: `facts.rs` models producer-facing semantics, `events.rs` models consumer-facing wire schema.
- Lossy async buffer: callers never block on telemetry I/O.
- Stable identifier hashing: skill IDs are generated from normalized path/repo/name inputs rather than transient runtime object identity.
- Defensive normalization: unknown or invalid lifecycle combinations mostly degrade to omission or logging rather than panicking.

## Open questions

- Should `thread_connections` and `thread_metadata` be pruned when threads end, or are they intentionally process-lifetime caches?
- Should queue overflow in [try_send](file:///Users/bytedance/project/codex/codex-rs/analytics/src/client.rs#L73-L78) emit an internal metric in addition to a warning? The code already has a TODO for this.
- Should `send_track_events` retry transient failures or batch multiple reducer outputs together instead of sending once per received fact?
- Should non-ChatGPT auth modes explicitly record that analytics was skipped, or is silent dropping the intended product behavior?
- Are compaction and guardian-review events guaranteed to observe thread initialization first in all real runtimes, or can valid events be lost due to startup/order races?
- When will `submission_type` and tool-call accounting fields in `CodexTurnEventParams` be populated?
- Should plugin/app dedupe eviction use LRU behavior rather than whole-set clearing at 4096 keys?
- Is SHA-1 acceptable for skill identity long term, or should a stronger hash be used even if collision risk is already negligible for this use case?

## Bottom line

`codex-analytics` is a small but high-leverage crate. It is not just a transport client; it is the telemetry correlation layer for the whole workspace. The most important code is the reducer, because that is where raw protocol traffic and high-level session facts are combined into meaningful analytics events. The implementation is pragmatic and intentionally lossy, favoring low overhead and caller isolation over guaranteed delivery.
