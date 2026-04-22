# codex-model-provider analysis

## Scope

This crate turns a serialized `ModelProviderInfo` configuration into a small runtime abstraction that the rest of Codex can use to:

- expose provider metadata in API-client form,
- resolve request authentication material,
- optionally create a provider-scoped external auth manager, and
- hide those details behind a shared `Arc<dyn ModelProvider>`.

It is intentionally narrow. It does not own HTTP transport, model listing, provider validation, or provider-definition parsing. Those concerns live in sibling crates such as `codex-api`, `codex-model-provider-info`, `codex-login`, and higher-level consumers like `models-manager`.

## Crate layout

- `src/lib.rs`: public surface and re-exports.
- `src/provider.rs`: core `ModelProvider` trait, default implementation, and constructor.
- `src/auth.rs`: auth-resolution helpers and provider-scoped `AuthManager` selection.
- `src/bearer_auth_provider.rs`: concrete `codex_api::AuthProvider` implementations for Authorization header attachment.

## Concrete responsibilities

### 1. Wrap provider config in a runtime trait

The central trait is:

```rust
#[async_trait::async_trait]
pub trait ModelProvider: fmt::Debug + Send + Sync {
    fn info(&self) -> &ModelProviderInfo;
    fn auth_manager(&self) -> Option<Arc<AuthManager>>;
    async fn auth(&self) -> Option<CodexAuth>;

    async fn api_provider(&self) -> codex_protocol::error::Result<Provider> {
        let auth = self.auth().await;
        self.info()
            .to_api_provider(auth.as_ref().map(CodexAuth::auth_mode))
    }

    async fn api_auth(&self) -> codex_protocol::error::Result<SharedAuthProvider> {
        let auth = self.auth().await;
        resolve_provider_auth(auth.as_ref(), self.info())
    }
}
```

What this means in practice:

- callers can treat all provider backends through one runtime interface,
- provider config remains the source of truth via `ModelProviderInfo`,
- auth is resolved lazily and asynchronously, and
- API-facing configuration and auth attachment are derived on demand.

### 2. Decide whether a provider uses the base auth manager or its own external one

`auth_manager_for_provider` chooses the auth source:

- if `ModelProviderInfo.auth` is present, the crate constructs a dedicated external-bearer `AuthManager` using `AuthManager::external_bearer_only(config)`,
- otherwise it preserves the caller-supplied base `AuthManager`.

This is the main behavioral fork in the crate. It lets command-backed providers inject their own token-refresh mechanism without changing the rest of the runtime.

### 3. Convert auth state into a request-header provider

`resolve_provider_auth` and `bearer_auth_provider_from_auth` translate provider/auth state into a `codex_api::SharedAuthProvider`.

Precedence is:

1. `provider.api_key()?` from `env_key`,
2. `provider.experimental_bearer_token`,
3. `CodexAuth` from the active `AuthManager`,
4. empty auth provider if none are available.

This precedence is important because it means provider-local static credentials override auth-manager state.

### 4. Attach request headers

Two concrete auth providers exist:

- `BearerAuthProvider`: builds `Authorization: Bearer <token>`.
- `AuthorizationHeaderAuthProvider`: accepts an already-formed `Authorization` header value, including non-Bearer schemes.

Both optionally add:

- `ChatGPT-Account-ID`
- `X-OpenAI-Fedramp: true`

The second type exists because some upstream flows already resolve the complete authorization scheme/value and must not be forced back into Bearer-only formatting.

## Public API

The crate exports:

- `create_model_provider(provider_info, auth_manager) -> SharedModelProvider`
- `ModelProvider` trait
- `SharedModelProvider = Arc<dyn ModelProvider>`
- `BearerAuthProvider`
- `AuthorizationHeaderAuthProvider`
- `CoreAuthProvider` as an alias of `BearerAuthProvider`

### `create_model_provider`

This is the normal entry point. It constructs a private `ConfiguredModelProvider`:

- stores `ModelProviderInfo`,
- resolves the provider-scoped auth manager once up front,
- returns it as `Arc<dyn ModelProvider>`.

### `ModelProvider::api_provider`

Builds a `codex_api::Provider` by:

- awaiting current auth,
- extracting `AuthMode` if present,
- delegating to `ModelProviderInfo::to_api_provider`.

This allows `codex-model-provider-info` to choose details like the ChatGPT backend base URL versus the standard OpenAI base URL based on auth mode.

### `ModelProvider::api_auth`

Builds the request auth adapter by:

- awaiting current auth,
- feeding both config and auth state into `resolve_provider_auth`.

The result is a synchronous `AuthProvider` object suitable for API clients.

## Runtime flow

### Construction flow

1. Higher-level code passes `ModelProviderInfo` and an optional base `AuthManager` into `create_model_provider`.
2. `auth_manager_for_provider` checks whether the provider defines command-backed auth.
3. If yes, it creates a dedicated external auth manager; otherwise it reuses the incoming manager.
4. The crate returns `Arc<ConfiguredModelProvider>` erased to `Arc<dyn ModelProvider>`.

### Request flow

1. Consumer calls `provider.auth().await`.
2. The default implementation asks the selected `AuthManager` for current `CodexAuth`.
3. Consumer calls either:
   - `provider.api_provider().await` for transport configuration, or
   - `provider.api_auth().await` for request headers.
4. `api_provider` uses `ModelProviderInfo::to_api_provider(auth_mode)`.
5. `api_auth` uses provider config plus resolved auth to produce an `AuthProvider`.
6. Downstream clients call `AuthProvider::add_auth_headers(&mut HeaderMap)` during request construction.

### Important downstream interaction

`models-manager` is a representative consumer. It uses this crate to:

- create its runtime provider from config,
- fetch current auth and auth mode,
- build API provider config and header attachment,
- optionally replace `api_auth` with `AuthorizationHeaderAuthProvider` when ChatGPT auth resolves to a non-trivial full authorization header.

That last point matters: this crate provides the reusable header-attacher, but some richer auth-shaping still happens in higher layers.

## Dependency roles

### Direct dependencies

- `async-trait`: enables async methods on the object-safe-ish `ModelProvider` trait.
- `codex-api`: supplies `Provider`, `AuthProvider`, and `SharedAuthProvider`.
- `codex-login`: supplies `AuthManager` and `CodexAuth`.
- `codex-model-provider-info`: supplies the serialized provider definition and conversion logic.
- `codex-protocol`: supplies shared error/result types and auth config types used in tests and helper calls.
- `http`: provides `HeaderMap` and `HeaderValue` for header insertion.

### Why the separation matters

- `codex-model-provider-info` owns declarative provider data and validation.
- `codex-model-provider` owns runtime adaptation of that data.
- `codex-login` owns auth acquisition and refresh.
- `codex-api` owns transport-facing abstractions.

This keeps the crate small and makes it mostly an integration layer rather than a business-logic-heavy crate.

## Design observations

### Thin adapter by design

This crate intentionally does very little computation itself. Most logic is orchestration:

- select the right auth manager,
- select the right credential source,
- wrap it in the right request-header type,
- forward config translation to `ModelProviderInfo`.

That is a good fit for a boundary crate.

### Trait object for runtime polymorphism

Using `SharedModelProvider = Arc<dyn ModelProvider>` suggests the authors want:

- cheap sharing across async tasks,
- easy substitution of alternate provider implementations later,
- minimal exposure of the default implementation type.

At the moment only one concrete implementation exists: `ConfiguredModelProvider`.

### Synchronous header insertion after async auth resolution

`codex_api::AuthProvider` is synchronous by contract. This crate preserves that by resolving async auth first and then returning a cheap synchronous header-adder object. That separation is clean and prevents request-building code from performing I/O.

### Explicit support for precomputed Authorization values

`AuthorizationHeaderAuthProvider` is a subtle but important design point. It avoids assuming that all auth can be represented as `Bearer <token>`. This supports cases where an upstream component resolves a different scheme or a provider-specific header format while still reusing the common `AuthProvider` interface.

### Provider-local auth manager override

Creating a dedicated external auth manager when `provider.auth` exists is the crate's strongest ownership decision. It makes provider auth behavior largely self-contained, but it also creates a split between base auth state and provider-local auth state.

## Testing coverage

The crate has focused unit tests inside the source files.

### `src/bearer_auth_provider.rs`

Covered behaviors:

- telemetry detects presence of an Authorization header,
- Bearer auth inserts `Authorization: Bearer ...`,
- account ID header is added when present,
- FedRAMP routing header is added when enabled,
- non-Bearer authorization schemes are supported by `AuthorizationHeaderAuthProvider`.

What these tests validate well:

- header names,
- exact header values,
- compatibility with `codex_api::auth_header_telemetry`,
- parity between the two auth provider types for shared behavior.

### `src/provider.rs`

Covered behavior:

- a provider with command-backed auth gets a provider-scoped external auth manager even when no base manager is supplied.

What is not directly tested here:

- precedence of `env_key` over `experimental_bearer_token` over `CodexAuth`,
- behavior when `provider.api_key()` returns an error,
- `api_provider()` auth-mode-dependent conversion,
- `auth()` delegation semantics when no auth manager exists,
- cloning/sharing behavior of `SharedModelProvider`.

The current tests are good smoke tests for the most important surface behavior, but they do not comprehensively lock down the auth resolution matrix.

## Notable code paths and behavior details

### Auth precedence

`bearer_auth_provider_from_auth` uses the following precedence:

```text
env_key API key
-> experimental_bearer_token
-> auth manager / CodexAuth token
-> no token
```

This has a few consequences:

- environment-backed provider credentials trump user login state,
- provider-configured experimental tokens trump login state,
- `account_id` and FedRAMP information are only carried through from `CodexAuth`,
- static provider credentials do not currently carry account metadata.

### Silent header dropping on invalid values

Both auth provider implementations insert headers only if `HeaderValue::from_str(...)` succeeds. Invalid header content is silently skipped.

That avoids panics, but it also means malformed credentials can degrade into missing auth headers without structured diagnostics at this layer.

### Error behavior

`api_auth()` can fail even though the returned auth provider is simple, because `provider.api_key()?` can fail when `env_key` is configured but absent or empty. So auth failure is represented during provider-auth construction, not during header insertion.

## Consumers and integration points

Known consumers include:

- `models-manager`
- `core`
- `cli`

Most call sites use `create_model_provider(...)` and then treat the result as a shared runtime handle. This indicates the crate is the canonical bridge from config-time provider definitions to execution-time provider behavior.

## Design trade-offs

### Strengths

- small and easy to reason about,
- clean separation between async auth lookup and sync header insertion,
- minimal public API,
- flexible enough to support both Bearer tokens and precomposed Authorization headers,
- defers complex provider-definition logic to the crate that owns it.

### Limitations

- only one built-in runtime provider implementation exists,
- auth precedence is hard-coded rather than policy-driven,
- invalid header values are silently ignored,
- provider-specific auth-manager exposure leaks through the public trait,
- some richer auth shaping still happens in downstream crates rather than here.

## Open questions

### 1. Should `auth_manager()` remain public on `ModelProvider`?

`provider.rs` already contains a TODO noting that auth-manager access should maybe become internal to this crate. That would make `ModelProvider` a cleaner abstraction, but it likely requires a broader refactor because other crates currently inspect the auth manager directly.

### 2. Should all provider-specific auth shaping move into this crate?

`models-manager` sometimes replaces `api_auth()` output with `AuthorizationHeaderAuthProvider` after additional ChatGPT-specific logic. That suggests the auth boundary is not fully centralized yet.

### 3. Should auth resolution expose structured diagnostics?

Today malformed header values are silently omitted. It may be useful to surface warning telemetry or typed errors for invalid configured credentials.

### 4. Should the auth precedence be configurable?

The current order is sensible, but it is policy. If providers need different precedence between env keys, command-backed auth, and login auth, the current design will need extension.

### 5. Should there be stronger test coverage for the auth matrix?

The most valuable missing tests appear to be:

- `env_key` success and failure paths,
- precedence between all credential sources,
- preservation or omission of account metadata across credential types,
- `api_provider()` behavior under different `AuthMode` values.

## Bottom line

`codex-model-provider` is a narrow adapter crate that converts provider configuration plus login state into two runtime artifacts:

- a transport-facing `codex_api::Provider`, and
- a request-header `codex_api::AuthProvider`.

Its key value is not algorithmic complexity, but architectural placement. It centralizes the boundary between provider configuration, auth management, and API-client consumption, while keeping the public surface small and async-safe.
