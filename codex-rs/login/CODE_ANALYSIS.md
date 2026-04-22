# codex-login crate analysis

## Overview

`codex-login` is the authentication and login support crate for Codex clients. It owns:

- Interactive browser-based ChatGPT login via a localhost OAuth callback server.
- Device-code login for environments where a browser callback is not practical.
- Persistent and ephemeral credential storage for API-key and ChatGPT auth.
- Runtime auth state management through `AuthManager`, including refresh and logout.
- Recovery behavior after `401 Unauthorized`, including guarded reload and refresh.
- Optional external auth integration for app-managed ChatGPT tokens and model-provider bearer tokens.
- Background agent task authorization support built on stored agent identity material.
- Auth-related environment telemetry and shared HTTP client defaults.

The crate is intentionally a mixed “library + auth runtime” layer: it exposes low-level building blocks such as `run_login_server`, `request_device_code`, and `save_auth`, but its main abstraction is `AuthManager`, which centralizes auth loading, caching, refresh, and change notifications.

## Module map

### Public entry points

- `src/lib.rs`
  - Re-exports the crate’s main surface: `AuthManager`, `CodexAuth`, `ServerOptions`, `run_login_server`, device-code helpers, storage helpers, and telemetry helpers.

### Core modules

- `src/auth/manager.rs`
  - Main auth model (`CodexAuth`) and control plane (`AuthManager`).
  - Refresh logic, external auth integration, guarded reload, unauthorized recovery, restriction enforcement, login/logout helpers.
- `src/auth/storage.rs`
  - `AuthDotJson` format plus storage backends: file, keyring, auto, and ephemeral.
- `src/server.rs`
  - Localhost OAuth callback server and browser-login flow.
- `src/device_code_auth.rs`
  - Device code login flow and polling.
- `src/token_data.rs`
  - JWT parsing and flat token metadata extraction.
- `src/agent_identity.rs`
  - Background-agent identity registration, stored signing keys, and `AgentAssertion` header generation.

### Supporting modules

- `src/auth/default_client.rs`
  - Shared HTTP client construction, `originator`, user-agent, residency headers.
- `src/auth/agent_assertion.rs`
  - Signs task-scoped `AgentAssertion` headers from stored agent identity records.
- `src/auth/external_bearer.rs`
  - External bearer-token provider backed by a shell command for custom model providers.
- `src/auth/revoke.rs`
  - Best-effort OAuth token revocation during logout.
- `src/auth/util.rs`
  - Small error-parsing helper for backend responses.
- `src/auth_env_telemetry.rs`
  - Presence-only auth environment telemetry without leaking secret values.

## Main responsibilities

### 1. Representing authentication state

The crate supports three main auth shapes:

- API key auth (`CodexAuth::ApiKey`)
- Managed ChatGPT OAuth auth (`CodexAuth::Chatgpt`)
- Externally supplied ChatGPT access tokens (`CodexAuth::ChatgptAuthTokens`)

These are all surfaced through `CodexAuth`, which provides convenience accessors such as:

- `auth_mode()` / `api_auth_mode()`
- `api_key()`
- `get_token()`
- `get_token_data()`
- `get_account_id()`
- `get_account_email()`
- `get_chatgpt_user_id()`
- `account_plan_type()`
- agent-identity getters/setters

The key persistence format is `AuthDotJson`, which stores:

- explicit or inferred auth mode
- optional `OPENAI_API_KEY`
- optional token bundle
- `last_refresh`
- optional `agent_identity`

`TokenData` stores the access token, refresh token, account id, and a parsed `IdTokenInfo`. The raw JWT is retained inside `IdTokenInfo.raw_jwt`, so the file format round-trips the original token string while still exposing extracted fields.

### 2. Managing cached auth state at runtime

`AuthManager` is the crate’s central abstraction. It caches the current auth snapshot in memory and hides storage details from the rest of the program.

Its key responsibilities are:

- load auth on startup
- resolve external API-key auth dynamically
- proactively refresh stale managed ChatGPT auth
- guard refreshes with a semaphore so only one refresh proceeds at a time
- reload auth from storage on demand
- expose watch-based auth-state notifications
- enforce workspace-specific and backend-specific ChatGPT auth behavior
- hold optional external auth providers
- provide `UnauthorizedRecovery` state for retry-after-401 flows

The cache stores not only the current auth snapshot but also an auth-scoped permanent refresh failure. This is an optimization: after a permanent refresh failure such as an expired/reused/revoked refresh token, repeated refresh attempts against the same credential snapshot fail fast without another network call.

### 3. Running login flows

The crate supports two sign-in paths:

- browser/OAuth authorization-code flow
- device-code flow

Both paths end by persisting an `AuthDotJson` record via the configured storage backend.

### 4. Storing credentials

The storage layer abstracts four backends:

- `File`
  - writes `$CODEX_HOME/auth.json`
- `Keyring`
  - stores serialized auth in OS keyring and deletes fallback file if present
- `Auto`
  - prefers keyring, falls back to file on keyring errors
- `Ephemeral`
  - process-local in-memory store keyed by `codex_home`

Ephemeral storage is important for externally managed ChatGPT auth tokens. Those tokens intentionally override persistent credentials for the current process without writing long-lived state to disk.

### 5. Supporting background agent task auth

For supported ChatGPT backends, the crate can derive an `AgentAssertion` authorization header instead of using a raw bearer token. This path:

- derives a binding from ChatGPT account/user identity
- generates or loads an Ed25519 keypair
- registers the agent runtime against the backend
- registers a background task
- stores the resulting identity material in auth storage
- signs task-scoped authorization envelopes for future requests

If this path is disabled, unsupported, or fails, the crate falls back to ordinary bearer auth.

## Public API surface

### High-level auth management

- `AuthManager`
- `AuthManagerConfig`
- `CodexAuth`
- `UnauthorizedRecovery`
- `AuthConfig`
- `enforce_login_restrictions`

### Login and logout helpers

- `run_login_server`
- `LoginServer`
- `ServerOptions`
- `request_device_code`
- `complete_device_code_login`
- `run_device_code_login`
- `login_with_api_key`
- `logout`
- `logout_with_revoke`
- `save_auth`
- `load_auth_dot_json`

### External auth and tokens

- `ExternalAuth`
- `ExternalAuthTokens`
- `ExternalAuthChatgptMetadata`
- `ExternalAuthRefreshContext`
- `ExternalAuthRefreshReason`

### Agent and token helpers

- `AgentIdentityAuthRecord`
- `AgentTaskAuthorizationTarget`
- `BackgroundAgentTaskAuthMode`
- `cached_background_agent_task_authorization_header_value`
- `TokenData`

### Client and telemetry helpers

- `default_client`
- `AuthEnvTelemetry`
- `collect_auth_env_telemetry`

## Key runtime flows

### Browser login flow

1. `run_login_server` binds a local `tiny_http` server on localhost.
2. It generates PKCE verifier/challenge and a CSRF `state`.
3. It builds an authorization URL with:
   - response type `code`
   - PKCE challenge
   - Codex scopes
   - `originator`
   - optional `allowed_workspace_id`
4. The browser redirects back to `/auth/callback`.
5. `process_request` validates:
   - callback path
   - `state`
   - OAuth error parameters
   - presence of authorization code
6. `exchange_code_for_tokens` posts to `/oauth/token`.
7. `ensure_workspace_allowed` enforces optional workspace restriction using the ID token claims.
8. `obtain_api_key` optionally performs a legacy token exchange for an API-key style token.
9. `persist_tokens_async` saves the final `AuthDotJson`.
10. The callback redirects to `/success`, then the server exits.

Notable behavior:

- Structured logs redact sensitive URLs and query parameters.
- User-visible error messages preserve more detail than logs.
- A custom response writer is used for terminal responses so the connection is always closed and old keep-alive sockets do not wedge future login attempts.

### Device-code login flow

1. `request_device_code` calls `/api/accounts/deviceauth/usercode`.
2. The CLI prints the verification URL and one-time code.
3. `complete_device_code_login` polls `/api/accounts/deviceauth/token`.
4. Once polling succeeds, it receives an authorization code plus PKCE material.
5. It reuses `server::exchange_code_for_tokens`.
6. It enforces workspace restrictions.
7. It persists tokens with `persist_tokens_async`.

Differences from browser login:

- No localhost callback server is needed.
- It does not call `obtain_api_key`; tests explicitly expect successful persistence without an API-key exchange.

### Auth loading precedence

When `load_auth` is called, precedence is:

1. `CODEX_API_KEY` environment variable, if env-based API key auth is enabled
2. ephemeral storage (`ChatgptAuthTokens`)
3. configured persistent storage backend

This means externally managed, process-local ChatGPT auth overrides persisted credentials, while env-based API key auth overrides everything when enabled.

### Proactive refresh flow

`AuthManager::auth()` behaves roughly as follows:

1. resolve external API-key auth if configured
2. read cached auth snapshot
3. if managed ChatGPT auth looks stale:
   - expired access token, or
   - `last_refresh` older than 8 days
4. call `refresh_token()`
5. if refresh fails, return the old cached snapshot
6. otherwise return refreshed cached auth

### Guarded refresh flow

`refresh_token()` is careful about multi-process races:

1. acquire the refresh semaphore
2. read current cached auth
3. if API-key auth, do nothing
4. compute expected account id
5. reload auth from storage only if the reloaded account id matches the current account
6. if reloaded auth changed, skip network refresh because another actor already updated credentials
7. if unchanged, refresh against the token authority
8. persist returned tokens
9. reload cache

This is a good example of the crate’s bias toward “consistent auth snapshot” semantics rather than fully live reactivity.

### Unauthorized recovery flow

`UnauthorizedRecovery` is an explicit state machine used after a `401`.

Managed ChatGPT auth:

1. `reload`
2. `refresh_token`
3. `done`

External auth:

1. `external_refresh`
2. `done`

The manager exposes mode and step names for diagnostics, and tests cover both managed and external variants.

### Logout flow

- `logout` removes stored auth from the configured store, and from ephemeral too when needed.
- `logout_with_revoke` first attempts OAuth revocation, then removes local state regardless of revocation success.
- Revocation uses refresh token first, then falls back to access token.

## Storage design

### File storage

`FileAuthStorage` writes pretty-printed JSON with mode `0600` on Unix. This backend is simple and test-friendly.

### Keyring storage

`KeyringAuthStorage` computes a short stable key from the canonicalized `codex_home` path. It stores serialized `AuthDotJson` in the OS keyring and removes any stale fallback file after a successful save.

### Auto storage

`AutoAuthStorage` prefers keyring for both load and save, but degrades to file storage when keyring access fails. This makes auth resilient on machines where keyring integration is partially broken.

### Ephemeral storage

`EphemeralAuthStorage` is a process-global `HashMap` protected by a mutex. It is intentionally not persisted and is used for externally managed auth snapshots.

## Dependency analysis

### Async/runtime and networking

- `tokio`
  - async runtime, task spawning, watch channels, semaphore, process execution.
- `reqwest`
  - OAuth, revocation, device-code, and background-agent HTTP traffic.
- `tiny_http`
  - embedded localhost login callback server.
- `webbrowser`
  - opens the login URL for browser flow.
- `url`, `urlencoding`
  - callback parsing and safe URL construction.

### Serialization and parsing

- `serde`, `serde_json`
  - auth persistence and backend payloads.
- `base64`, `sha2`
  - JWT decoding, PKCE, and signed envelope encoding.
- `chrono`
  - timestamps, refresh age, token expiration handling.

### Workspace-specific crates

- `codex-client`
  - shared HTTP client wrapper and custom CA support.
- `codex-config`
  - auth storage mode and residency configuration.
- `codex-app-server-protocol`
  - shared auth-mode enum.
- `codex-protocol`
  - plan types, refresh errors, provider auth config, and session source types.
- `codex-keyring-store`
  - OS keyring abstraction.
- `codex-model-provider-info`
  - provider env-key metadata for telemetry.
- `codex-otel`
  - telemetry shape for auth environment reporting.
- `codex-terminal-detection`
  - user-agent construction.
- `codex-utils-template`
  - branded HTML error page rendering.

### Crypto and identity

- `ed25519-dalek`
  - agent identity signing keys and signatures.
- `crypto_box`
  - decryption of registered background task IDs.
- `rand`
  - PKCE, OAuth state, request IDs, and key generation.

## Testing analysis

The crate has both unit tests and consolidated integration tests.

### Unit coverage highlights

- `src/auth/auth_tests.rs`
  - auth loading
  - login restriction enforcement
  - plan-type mapping
  - permanent refresh-failure scoping
  - external bearer provider behavior
  - auth state watch notifications
- `src/auth/storage_tests.rs`
  - all storage backends
  - keyring/file fallback behavior
  - delete semantics
- `src/token_data_tests.rs`
  - JWT parsing, plan mapping, workspace/fedramp detection, expiration parsing
- `src/auth/default_client_tests.rs`
  - user-agent and default header behavior
- `src/server.rs` inline tests
  - error-page rendering
  - token-endpoint error parsing
  - URL redaction behavior
- `src/agent_identity.rs` and `src/auth/agent_assertion.rs` inline tests
  - signed `AgentAssertion` generation and validation

### Integration coverage highlights

- `tests/suite/login_server_e2e.rs`
  - end-to-end OAuth callback flow
  - missing Codex home directory creation
  - workspace mismatch behavior
  - entitlement-specific error messaging
  - canceling an older login server on the same port
- `tests/suite/device_code_login.rs`
  - successful device-code login
  - polling retries
  - workspace mismatch rejection
  - failure propagation
- `tests/suite/auth_refresh.rs`
  - happy-path refresh
  - guarded reload vs refresh
  - proactive refresh based on token expiry / stale age
  - permanent vs transient error handling
  - unauthorized recovery
- `tests/suite/logout.rs`
  - revoke payload shape
  - best-effort revoke semantics
  - logout using cached auth snapshot

### Test strategy observations

- The test suite is behavior-oriented rather than purely structural.
- `wiremock` is used heavily to simulate auth backends.
- Some tests use serial execution because they mutate process environment variables.
- Coverage is strongest around refresh/recovery behavior and storage semantics, which are the highest-risk parts of the crate.

## Design observations

### Strong points

- `AuthManager` gives the rest of the workspace a single auth abstraction instead of exposing storage details everywhere.
- The guarded reload-before-refresh logic is thoughtful and reduces multi-process token refresh races.
- Permanent refresh failures are scoped to a specific auth snapshot, preventing noisy repeated network attempts.
- The storage abstraction is pragmatic and supports secure-enough defaults while keeping file-based testing easy.
- Browser login separates user-facing error detail from structured logging and redacts sensitive URL content.
- External auth support is flexible enough to cover both app-managed ChatGPT tokens and provider-shell-command bearer tokens.
- Background agent task auth is layered so bearer auth remains a reliable fallback.

### Tradeoffs / non-obvious choices

- `AuthManager::reload()` reports change using top-level auth equality (`ApiKey` vs `Chatgpt`) rather than credential equality, so a credential rotation inside the same auth mode can return `false` while still notifying watchers and updating the cache.
- Ephemeral auth is process-global and keyed by `codex_home`, which is simple but means it is not isolated per logical client instance inside the same process.
- The browser flow still optionally exchanges the ID token for an API-key style token and stores it in `OPENAI_API_KEY`, which mixes legacy and newer auth representations.
- JWT parsing logic exists in both `token_data.rs` and `server.rs` (`jwt_auth_claims`), which duplicates claim extraction logic.
- The localhost login server uses a blocking `tiny_http` thread plus async glue instead of a fully async server; this is simpler, but more specialized.

## Open questions

1. Should the browser login flow still call `obtain_api_key` and persist `OPENAI_API_KEY`?
   - `tests/suite/login_server_e2e.rs` explicitly notes this as legacy behavior that may be removed later.
   - Device-code login already succeeds without this exchange, so the two login flows are not fully aligned.

2. Should JWT claim parsing be consolidated?
   - `token_data.rs` has the main JWT parsing path, while `server.rs` has a separate `jwt_auth_claims` helper.
   - A shared parser would reduce drift risk and improve consistency of workspace/plan extraction.

3. Is `reload()`’s boolean return value too weakly defined?
   - Tests intentionally document that credential changes within the same auth mode can yield `false`.
   - If callers interpret `false` as “no meaningful auth change”, they may be surprised.

4. Should ephemeral auth storage have more explicit lifecycle management?
   - The current in-memory global store is convenient, but long-lived multi-client processes may want stronger namespacing or cleanup guarantees.

5. How should background agent task auth evolve for non-first-party ChatGPT hosts?
   - Support is currently gated by a hardcoded host allowlist.
   - That is safe, but it may become a maintenance point as staging/custom hosts evolve.

6. Should external API-key auth participate in more cache visibility?
   - External API-key auth is resolved dynamically and does not update `inner.auth`.
   - That is reasonable, but it means some state-observation paths behave differently for external API-key auth vs stored auth.

## Practical summary

If another crate only needs “what auth should I use right now?”, it should depend on `AuthManager`.

If a caller needs to:

- start login: use `run_login_server` or `run_device_code_login`
- inspect token/account details: use `CodexAuth`
- persist or inspect auth in tests: use `save_auth` / `load_auth_dot_json`
- recover from `401`: use `AuthManager::unauthorized_recovery()`
- enforce workspace/method restrictions: use `enforce_login_restrictions`

Overall, `codex-login` is not just a login helper. It is the crate that defines how Codex auth is represented, loaded, refreshed, restricted, recovered, and persisted across the rest of the workspace.
