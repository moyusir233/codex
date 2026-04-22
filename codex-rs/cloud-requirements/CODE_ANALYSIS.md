# codex-cloud-requirements Analysis

## Scope

This crate is a focused adapter that loads managed `requirements.toml` policy from the ChatGPT backend for eligible workspace accounts, caches it locally, and exposes the result through `CloudRequirementsLoader` so the rest of Codex can merge those requirements into normal config loading.

- Crate manifest: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/Cargo.toml)
- Main implementation: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs)
- Bazel target: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/BUILD.bazel)

The crate has a single source module, so most design decisions, implementation details, and tests live together in one file.

## Responsibilities

The crate owns six concrete responsibilities:

1. Determine whether cloud-managed requirements apply to the current auth/session.
2. Fetch `requirements.toml` contents from the backend with retry and timeout behavior.
3. Recover from `401 Unauthorized` responses via `AuthManager::unauthorized_recovery()`.
4. Parse backend TOML into `ConfigRequirementsToml`.
5. Persist a signed local cache scoped to ChatGPT user/account identity.
6. Expose a shared async loader that other crates can await exactly once.

These responsibilities are implemented primarily in [CloudRequirementsService](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L268-L708), with backend transport in [BackendRequirementsFetcher](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L196-L266) and public construction functions in [cloud_requirements_loader](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L710-L759).

## Public API

The crate exposes two public constructors and otherwise keeps its operational machinery private:

- [cloud_requirements_loader](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L710-L745)
  - Accepts an existing `Arc<AuthManager>`, backend base URL, and `codex_home`.
  - Starts the initial fetch in a spawned task.
  - Starts a background cache refresher loop in a separate spawned task.
  - Returns a `CloudRequirementsLoader`.

- [cloud_requirements_loader_for_storage](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L747-L759)
  - Builds a shared `AuthManager` from storage settings.
  - Delegates to `cloud_requirements_loader`.

The returned loader type is defined in another crate at [config/src/cloud_requirements.rs](file:///Users/bytedance/project/codex/codex-rs/config/src/cloud_requirements.rs#L48-L82):

- `CloudRequirementsLoader` wraps a shared boxed future.
- `get()` memoizes the result so concurrent callers observe the same resolved value.
- Failure is typed as `CloudRequirementsLoadError`, with explicit codes:
  - `Auth`
  - `Timeout`
  - `Parse`
  - `RequestFailed`
  - `Internal`

That separation is important: this crate performs the fetch/caching work, but the stable abstraction consumed elsewhere lives in `codex-config`/`codex-core`.

## Internal Structure

The implementation is organized around a few internal types:

- [RequirementsFetcher](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L185-L194)
  - Trait for fetching remote raw contents.
  - Exists mainly to make the service testable.

- [BackendRequirementsFetcher](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L196-L266)
  - Real transport adapter using `codex_backend_client::Client`.
  - Converts backend/network failures into crate-specific retry/auth categories.

- [CloudRequirementsService](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L268-L708)
  - Central orchestration type.
  - Owns auth access, fetcher, cache path, timeout, retry logic, cache IO, and background refresh.

- [CloudRequirementsCacheFile](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L118-L131)
  - On-disk JSON envelope.
  - Contains a signed payload plus base64-encoded HMAC signature.

- [FetchAttemptError](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L83-L90)
  - Distinguishes retryable failures from unauthorized failures.
  - Keeps retry policy separate from parsing and cache behavior.

- [CacheLoadStatus](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L92-L110)
  - Encodes all cache miss/ignore reasons.
  - Lets the service log degraded cache cases without turning every miss into a hard error.

## End-to-End Flow

### Startup path

The startup path begins in [cloud_requirements_loader](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L710-L745):

1. Construct `CloudRequirementsService`.
2. Spawn `fetch_with_timeout()` for the initial fetch.
3. Spawn `refresh_cache_in_background()` for periodic refresh.
4. Store the refresher task in a process-global `OnceLock<Mutex<Option<JoinHandle<()>>>>` so the newest loader replaces any previous refresher.
5. Return `CloudRequirementsLoader::new(async move { ... })`, which resolves from the startup task result.

The shared-future semantics of `CloudRequirementsLoader` mean the initial remote/cache lookup is effectively "single flight" for consumers.

### Eligibility gate

The first gate is in [CloudRequirementsService::fetch](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L347-L378):

- If there is no auth, return `Ok(None)`.
- If the auth has no plan type, return `Ok(None)`.
- If auth is not ChatGPT auth, return `Ok(None)`.
- If the plan is not business-like and not `Enterprise`, return `Ok(None)`.

The plan helpers come from [protocol/src/account.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/account.rs#L30-L38):

- `is_team_like()` covers `Team` and `SelfServeBusinessUsageBased`.
- `is_business_like()` covers `Business` and `EnterpriseCbpUsageBased`.

This crate intentionally allows:

- `Business`
- `EnterpriseCbpUsageBased`
- `Enterprise`
- `hc` aliases indirectly, because auth parsing maps `hc` to `Enterprise` in the auth stack

It intentionally does not allow team-like plans.

### Cache-first lookup

Still inside [fetch](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L359-L377), the service derives identity with [auth_identity](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L171-L179) and then calls [load_cache](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L594-L647).

`load_cache()` requires:

- current `chatgpt_user_id`
- current `account_id`
- readable cache file
- valid JSON structure
- valid HMAC signature
- cached identity exactly matching current identity
- unexpired TTL

If all checks pass, the cached payload is converted back into requirements via [CloudRequirementsCacheSignedPayload::requirements](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L133-L139).

If any check fails, the service logs the reason and falls through to remote fetch. Notably, a missing cache file is treated as a silent cold start.

### Remote fetch path

The remote path is [fetch_with_retries](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L380-L549):

- Max attempts: `5`
- Timeout for outer startup fetch: `15s`
- Backoff source: `codex_core::util::backoff`
- Retryable categories:
  - backend client construction failure
  - request failure except unauthorized
- Non-retryable categories:
  - parse error
  - unauthorized with exhausted/unavailable recovery

On success:

- Missing backend `contents` becomes `Ok(None)`.
- Present contents are parsed by [parse_cloud_requirements](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L761-L774).
- Parsed requirements are saved to cache via [save_cache](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L668-L707).
- The function returns `Option<ConfigRequirementsToml>`.

On parse failure:

- The function returns a `CloudRequirementsLoadErrorCode::Parse`.
- The message is user-facing and intentionally admin-oriented: [format_cloud_requirements_parse_failed_message](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L776-L781).
- No retry is attempted because bad TOML is treated as a persistent configuration error, not a transient transport problem.

### Unauthorized recovery path

If the backend returns `401`, [fetch_with_retries](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L413-L497) switches to `AuthManager::unauthorized_recovery()`.

The recovery state machine is documented in [login/src/auth/manager.rs](file:///Users/bytedance/project/codex/codex-rs/login/src/auth/manager.rs#L1001-L1080):

- Managed ChatGPT auth:
  - Step 1: reload auth from disk, but only when account identity still matches.
  - Step 2: refresh OAuth tokens.
- External auth:
  - single external refresh step.
- API key auth:
  - effectively no recovery.

This crate does not implement refresh logic itself; it delegates fully to `AuthManager`, then retries with the refreshed auth snapshot.

### Background refresh

[refresh_cache_in_background](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L551-L565) runs forever until the account is no longer eligible:

- sleep for 5 minutes
- call `refresh_cache()` under the same timeout
- stop looping if auth disappears or the plan becomes ineligible
- keep the existing cache on refresh failure

[refresh_cache](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L567-L592) repeats the same eligibility gate as startup, then calls `fetch_with_retries(auth, "refresh")`.

The design here is subtle: startup is allowed to fail closed for eligible accounts, while background refresh is best-effort and preserves the previously cached state.

## Backend/API Behavior

The backend request is implemented in [BackendRequirementsFetcher::fetch_requirements](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L210-L266).

Key request behavior:

- Builds `BackendClient` from the configured base URL.
- Adds Codex user-agent.
- Adds ChatGPT authorization header when available.
- Adds ChatGPT account ID header when available.
- Adds FedRAMP routing header for FedRAMP accounts.
- Calls `get_config_requirements_file()`.

Response handling:

- `response.contents == None` means "no cloud requirements configured".
- Unauthorized backend errors become `FetchAttemptError::Unauthorized`.
- All other request errors become retryable.

This design keeps policy semantics separate from transport semantics: the fetcher only produces raw TOML or fetch-classified errors.

## Parsing and Semantics

[parse_cloud_requirements](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L761-L774) has intentionally narrow behavior:

- Trimmed empty string => `Ok(None)`
- TOML that parses to an empty `ConfigRequirementsToml` => `Ok(None)`
- Valid non-empty TOML => `Ok(Some(...))`
- Invalid TOML => `Err(toml::de::Error)`

The parsed type `ConfigRequirementsToml` is defined elsewhere in the config stack and represents administrative constraints such as:

- approval policies
- sandbox modes
- web search modes
- app/connector enablement
- rules, network, permissions, residency, and feature requirements

Tests in this crate confirm at least two concrete TOML shapes:

- top-level policy arrays such as `allowed_approval_policies`
- nested `[apps.<id>]` sections

## Cache Design

The cache is JSON stored at:

- `${codex_home}/cloud-requirements-cache.json`

The schema is implemented in [CloudRequirementsCacheFile](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L118-L131):

- `signed_payload`
  - `cached_at`
  - `expires_at`
  - `chatgpt_user_id`
  - `account_id`
  - `contents`
- `signature`

Integrity model:

- Payload bytes are signed with HMAC-SHA256 in [sign_cache_payload](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L140-L145).
- Verification happens in [verify_cache_signature](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L160-L169).
- Read keys are an array, which gives room for future key rotation.

Lifecycle parameters:

- Refresh interval: 5 minutes
- TTL: 30 minutes

Identity scoping:

- Cache use requires both `chatgpt_user_id` and `account_id`.
- Cache writes still happen even when identity is incomplete.
- Cache reads are skipped when current identity is incomplete.

That asymmetry is intentional and visible in tests: the crate prefers to preserve fetched data for a later complete identity, but refuses to trust it unless identity checks can be enforced.

## Metrics and Observability

The crate emits OpenTelemetry counters and a startup timer:

- fetch duration timer in [fetch_with_timeout](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L291-L345)
- attempt counter in [emit_fetch_attempt_metric](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L783-L800)
- final fetch counter in [emit_fetch_final_metric](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L802-L821)
- load counter in [emit_load_metric](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L823-L831)

Metric dimensions include:

- `trigger`: `startup` or `refresh`
- `attempt`
- `attempt_count`
- `outcome`
- `reason`
- `status_code`

Tracing is also used heavily for:

- client construction failures
- backend request failures
- cache accept/reject paths
- parse errors
- refresh failures

## Integration With the Rest of Codex

### Config loading

The real consumer of this crate is the core config loader in [core/src/config_loader/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/config_loader/mod.rs#L94-L166).

The precedence order there is explicit:

- cloud requirements
- managed admin preferences on macOS
- system `requirements.toml`

Cloud requirements are applied first via `merge_requirements_with_remote_sandbox_config(...)`, which means later layers only fill unset fields. This gives cloud-managed policy the strongest precedence among requirement sources.

Relevant merge logic:

- cloud application: [config_loader/mod.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/config_loader/mod.rs#L133-L166)
- remote sandbox expansion + merge: [merge_requirements_with_remote_sandbox_config](file:///Users/bytedance/project/codex/codex-rs/core/src/config_loader/mod.rs#L618-L631)

### CLI startup

The CLI constructs the loader in [exec/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L339-L352), using the configured ChatGPT base URL and auth credential storage mode.

This shows the intended lifecycle:

- create loader early during process startup
- pass loader into config assembly
- let config loading block on the shared future when requirements are needed

## Dependencies

### First-party crates

- `codex-backend-client`
  - creates backend client and performs `get_config_requirements_file()`
- `codex-config`
  - provides `AuthCredentialsStoreMode`
- `codex-core`
  - provides `CloudRequirementsLoader`, error types, `ConfigRequirementsToml`, and retry backoff
- `codex-login`
  - provides `AuthManager`, `CodexAuth`, and unauthorized/token recovery
- `codex-otel`
  - emits metrics and timers
- `codex-protocol`
  - provides `PlanType`

### Third-party crates

- `async-trait`
  - async trait abstraction for `RequirementsFetcher`
- `base64`
  - signature encoding/decoding
- `chrono`
  - cache timestamps and TTL calculation
- `hmac` + `sha2`
  - cache payload signing/verification
- `serde` + `serde_json`
  - cache serialization
- `thiserror`
  - internal error enums
- `tokio`
  - async FS, sleep, timeout, spawn, mutexes used in tests
- `toml`
  - requirements parsing
- `tracing`
  - structured logs

## Testing Strategy

This crate has extensive in-file unit tests in [tests](file:///Users/bytedance/project/codex/codex-rs/cloud-requirements/src/lib.rs#L849-L2177). The tests are strong for behavior and edge cases because the `RequirementsFetcher` trait allows deterministic fakes.

### What is well covered

- Eligibility gating
  - non-ChatGPT auth skipped
  - non-business/enterprise plans skipped
  - business/business-like/enterprise plans accepted

- Parsing behavior
  - missing contents
  - empty contents
  - invalid TOML
  - empty requirements
  - valid approval-policy TOML
  - nested app requirements TOML

- Timeout and retry
  - startup timeout fails closed
  - transient request retry until success
  - retry exhaustion returns `RequestFailed`
  - `None` is treated as success without retry

- Unauthorized recovery
  - reload-based recovery after stale token
  - cache identity updated after recovery changes identity
  - permanent recovery errors surface user-facing auth messages
  - external token mode without recovery uses generic auth failure message

- Cache behavior
  - valid cache is used
  - incomplete auth identity prevents cache read
  - identity mismatch prevents cache read
  - tampered cache is ignored
  - expired cache is ignored
  - signed cache is written
  - refresh updates cached requirements

### Gaps / lighter coverage

- No direct tests for emitted metrics/tags.
- No direct tests for tracing content.
- No direct tests for FedRAMP routing header application.
- No direct tests for process-global refresher replacement behavior.
- No direct tests for backend client construction failure path.

## Design Observations

### Strong choices

- Fail-closed semantics for eligible workspace accounts are explicit and consistently enforced.
- The `RequirementsFetcher` trait cleanly decouples transport from orchestration and makes tests easy.
- Cache identity binding prevents requirements leakage across users/accounts on shared storage.
- Shared-future loader avoids duplicate startup fetches.
- Parse errors are surfaced immediately rather than hidden behind retries, which is the correct operational model for admin-authored policy.

### Notable tradeoffs

- The cache signature protects integrity, but the HMAC key is compiled into the binary, so this is tamper detection against casual/local corruption, not a strong secret-rooted authenticity guarantee against a determined reverse engineer.
- The refresher task is global process state. This prevents duplicate refreshers, but it also introduces hidden coupling across loader instances.
- Cache validity is keyed to identity and TTL, but not to backend base URL or environment. If the same user/account talks to a different backend with the same `codex_home`, the cache could be reused.
- Background refresh failures preserve the existing cache, which favors availability over strict freshness.

## Relationship to Higher-Level Config Semantics

The config loader treats cloud requirements as the highest-priority requirement source. Tests in [core/src/config_loader/tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/config_loader/tests.rs#L753-L1164) confirm:

- cloud requirements override MDM requirements
- system requirements cannot overwrite cloud requirements
- config loading includes cloud requirements in the assembled layers
- cloud loader failures bubble up as config-load failures

Connector-focused tests in [core/src/connectors_tests.rs](file:///Users/bytedance/project/codex/codex-rs/core/src/connectors_tests.rs#L552-L644) also show that cloud-managed app requirements can disable connectors even when user config enables them. That demonstrates the practical enforcement intent of this crate.

## Open Questions

1. Should cache validity also include `chatgpt_base_url` or another environment discriminator to avoid cross-environment cache reuse?
2. Should the HMAC read-key array be used for planned key rotation, and if so, what migration policy is expected?
3. Is the global refresher task intentionally process-wide forever, or should shutdown / explicit cancellation be exposed?
4. Should startup and refresh share identical failure semantics, or is best-effort refresh with stale-cache retention the intended long-term behavior?
5. Should metrics be promoted to tested behavior, at least for critical tags like `trigger`, `reason`, and `status_code`?
6. Should backend client construction failures include more user-facing detail, or is the current generic retry behavior preferred?
7. The CLI call site still carries a TODO comment about making cloud requirement failures blocking once fail-closed is possible; does that comment now lag the current behavior?

## Validation Performed

I ran the crate's test target successfully:

```bash
cargo test -p codex-cloud-requirements
```

Result:

- exit code `0`
- unit tests and doc tests passed

## Bottom Line

`codex-cloud-requirements` is a small but security-relevant crate. Its job is not just to fetch remote TOML; it acts as the policy bridge between ChatGPT workspace administration and local Codex config enforcement. The implementation is intentionally conservative:

- only eligible workspace accounts participate
- cache reuse is identity-bound and signed
- startup failures for eligible accounts fail closed
- auth recovery is delegated to the central auth manager
- parsed requirements enter the config system at the highest requirement precedence

For such a small crate, the test coverage is strong and the design is coherent. The main areas worth revisiting are cache/environment scoping, the process-global refresher lifecycle, and the exact security expectations around the embedded HMAC key.
