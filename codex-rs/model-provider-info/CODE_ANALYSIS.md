# codex-model-provider-info analysis

## Scope

- Crate root: [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/Cargo.toml#L1-L27)
- Main implementation: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L1-L497)
- Unit tests: [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L1-L423)
- Bazel target: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/BUILD.bazel#L1-L6)

## Responsibilities

This crate is the typed registry and normalization layer for model-provider configuration in Codex.
Its job is not to execute requests directly; instead, it defines the serialized provider model,
ships the built-in provider catalog, validates incompatible configuration combinations, and adapts
provider definitions into the lower-level API client shape used elsewhere.

Concretely, the crate owns:

1. The serialized schema for a provider entry via `ModelProviderInfo`.
2. The supported wire protocol enum via `WireApi`.
3. Built-in provider definitions for OpenAI, Amazon Bedrock, Ollama, and LM Studio.
4. Merge rules for combining built-in providers with user configuration.
5. Conversion from config-time provider metadata into `codex_api::Provider`.
6. Small convenience predicates such as OpenAI / Bedrock detection and remote-compaction support.

The top-level crate docs describe the intent clearly in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L1-L6).

## Public API surface

### Types

- `WireApi`: currently a single-variant enum containing `Responses`; custom deserialization rejects the removed `"chat"` value with a targeted migration error in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L44-L74).
- `ModelProviderInfo`: the core config model with fields for naming, endpoint selection, auth configuration, extra headers, retry knobs, stream/websocket timeouts, and capability flags in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L76-L131).
- `ModelProviderAwsAuthInfo`: minimal AWS SigV4 configuration, currently only an optional `profile`, in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L133-L139).

### Methods on `ModelProviderInfo`

- `validate`: enforces mutually exclusive auth modes and rejects unsupported AWS + websocket combinations in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L141-L200).
- `to_api_provider`: converts config metadata into `codex_api::Provider`, materializing base URL, headers, retry policy, and stream idle timeout in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L229-L257).
- `api_key`: loads the environment-backed API key and returns a structured `CodexErr::EnvVar` when required data is missing in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L259-L278).
- Effective-value helpers: `request_max_retries`, `stream_max_retries`, `stream_idle_timeout`, and `websocket_connect_timeout` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L280-L306).
- Built-in constructors: `create_openai_provider` and `create_amazon_bedrock_provider` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L308-L367).
- Capability predicates: `is_openai`, `is_amazon_bedrock`, `supports_remote_compaction`, and `has_command_auth` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L369-L384).

### Free functions and constants

- Built-in catalog: `built_in_model_providers` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L392-L419).
- Merge logic: `merge_configured_model_providers` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L421-L452).
- OSS provider factories: `create_oss_provider` and `create_oss_provider_with_base_url` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L454-L493).
- Exported identifiers and defaults: provider IDs, default ports, default Bedrock URL, and default websocket timeout in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L26-L43) and [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L386-L390).

## Data model and semantics

### Authentication modes

`ModelProviderInfo` supports several mutually exclusive ways to authenticate:

- `env_key`: resolve a bearer token from an environment variable.
- `experimental_bearer_token`: inline token string for programmatic usage.
- `auth`: command-backed token acquisition described by `ModelProviderAuthInfo`.
- `aws`: AWS SigV4 auth, currently specialized for the Bedrock provider.
- `requires_openai_auth`: delegate auth UX and token storage to the broader OpenAI / ChatGPT login flow.

The crate treats these as incompatible families and validates them explicitly in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L141-L200).
`ModelProviderAuthInfo` itself lives in another crate and provides the command, args, timeout, refresh interval, and working directory in [config_types.rs](file:///Users/bytedance/project/codex/codex-rs/protocol/src/config_types.rs#L273-L304).

### Transport and protocol

- `wire_api` is intentionally constrained: only Responses is accepted.
- `supports_websockets` is a provider capability flag, separate from `wire_api`.
- `stream_idle_timeout_ms` and `websocket_connect_timeout_ms` model transport behavior, not auth behavior.
- Retry controls are split between request retries and stream reconnect retries, each capped at 100.

This separation makes the config model expressive without requiring the runtime API layer to understand every config detail directly.

## Control flow

### 1. Build the catalog

`built_in_model_providers` constructs four built-in entries:

- OpenAI via `create_openai_provider`
- Amazon Bedrock via `create_amazon_bedrock_provider`
- Ollama via `create_oss_provider(DEFAULT_OLLAMA_PORT, WireApi::Responses)`
- LM Studio via `create_oss_provider(DEFAULT_LMSTUDIO_PORT, WireApi::Responses)`

See [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L392-L419).

### 2. Merge configured providers

`merge_configured_model_providers` overlays user config onto the built-in map with a very specific policy in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L421-L452):

- custom IDs are added only if absent;
- built-in providers are not generally overwritten;
- `amazon-bedrock` is a special case where only `aws.profile` may be customized.

This is narrower than a full “last writer wins” merge.

### 3. Validate provider semantics

This crate exposes `ModelProviderInfo::validate`, while the broader config crate adds higher-level policy in [config_toml.rs](file:///Users/bytedance/project/codex/codex-rs/config/src/config_toml.rs#L803-L826):

- only `amazon-bedrock` may use `aws`;
- provider names must be non-empty;
- per-provider validation errors are prefixed with `model_providers.<id>`.

So validation is intentionally split between local structural checks and config-level policy checks.

### 4. Resolve runtime API shape

When runtime code needs an actual API provider, the `model-provider` crate asks `ModelProviderInfo` to translate itself:

- runtime trait call path in [provider.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider/src/provider.rs#L19-L45)
- translation logic in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L229-L257)
- resulting target struct in [provider.rs](file:///Users/bytedance/project/codex/codex-rs/codex-api/src/provider.rs#L43-L50)

`to_api_provider` performs the following steps:

1. Picks a default base URL.
2. Switches OpenAI default base URL to `https://chatgpt.com/backend-api/codex` when `AuthMode::Chatgpt` is active.
3. Builds extra headers from static config and environment-backed config.
4. Produces a retry config with a fixed base delay of 200 ms.
5. Emits the final `codex_api::Provider`.

## Built-in providers

### OpenAI

`create_openai_provider` creates the richest built-in definition in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L308-L343):

- default base URL is runtime-dependent if no explicit URL is given;
- `requires_openai_auth = true`;
- `supports_websockets = true`;
- static `version` header is injected from `CARGO_PKG_VERSION`;
- `OpenAI-Organization` and `OpenAI-Project` can be sourced from environment variables.

This provider is the default “first-party” experience and integrates most tightly with the broader auth flow.

### Amazon Bedrock

`create_amazon_bedrock_provider` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L345-L367) sets:

- fixed default base URL;
- `aws` auth enabled by default;
- no websocket support;
- no OpenAI-managed auth.

The provider is intentionally constrained, and user overrides are limited to `aws.profile`.

### OSS providers: Ollama and LM Studio

The two OSS provider IDs both reuse `create_oss_provider`, which reads experimental environment variables in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L454-L471):

- `CODEX_OSS_PORT`
- `CODEX_OSS_BASE_URL`

The final provider is produced by `create_oss_provider_with_base_url` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L473-L493), which hard-codes a generic provider name of `"gpt-oss"` and disables auth and websocket support.

An important subtlety is that both Ollama and LM Studio share the same environment-variable override path, so the provider ID differentiates them more than the `name` field does.

## Dependency analysis

### Direct dependencies from `Cargo.toml`

- `codex-api`: target runtime provider shape, retry config, and Azure detection helpers.
- `codex-app-server-protocol`: provides `AuthMode`.
- `codex-protocol`: provides `ModelProviderAuthInfo` and the crate-wide error types.
- `http`: used for validated `HeaderMap`, `HeaderName`, and `HeaderValue`.
- `schemars`: derives JSON schema for config/schema export.
- `serde`: derives serialization and deserialization for TOML/JSON config loading.

See [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/Cargo.toml#L15-L27).

### How those dependencies shape the design

- `serde` + `schemars` make `ModelProviderInfo` both a runtime type and a schema source.
- `codex-protocol` centralizes shared config subtypes and error plumbing, preventing this crate from redefining auth-command semantics.
- `codex-api` keeps the adapter boundary crisp: this crate owns config semantics, while `codex-api` owns HTTP request construction.
- `http` lets the crate validate and normalize headers before the client layer sees them.

## Error handling and validation behavior

### Positive traits

- Removed config paths get explicit migration messages, especially for `wire_api = "chat"`.
- Missing environment-backed API keys return a structured error with optional setup instructions.
- Retry knobs are bounded to prevent pathological values.
- Auth conflicts are reported as user-readable strings.

### Notable behavior

- `build_header_map` silently skips invalid header names or values rather than returning an error in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L202-L227).
- `api_key` only considers `env_key`; other auth modes are intentionally handled elsewhere.
- `supports_remote_compaction` defers Azure detection to `codex_api::is_azure_responses_provider` in [provider.rs](file:///Users/bytedance/project/codex/codex-rs/codex-api/src/provider.rs#L106-L127).

## Testing coverage

The crate keeps all unit tests in a sibling file referenced through `#[path = "model_provider_info_tests.rs"]` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L495-L497).

### Covered areas

- TOML deserialization for simple custom providers, Azure, and header-heavy examples in [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L8-L107).
- Migration error for deprecated `wire_api = "chat"` in [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L109-L120).
- Websocket timeout deserialization in [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L122-L133).
- `supports_remote_compaction` behavior for OpenAI, Azure-like, and non-Azure providers in [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L135-L190).
- Command-auth deserialization defaults and zero refresh interval behavior in [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L192-L218) and [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L404-L423).
- AWS config deserialization and built-in Bedrock creation in [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L220-L276).
- Merge rules around custom providers and Bedrock-only profile overrides in [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L278-L372).
- Validation errors for AWS auth conflicts and websocket incompatibility in [model_provider_info_tests.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/model_provider_info_tests.rs#L374-L402).

### Gaps

- No direct unit tests for `to_api_provider`, including the ChatGPT-specific default base URL branch.
- No direct unit tests for `build_header_map`, especially invalid header handling and env-header omission.
- No direct unit tests for `api_key`.
- No direct unit tests for retry-cap clamping.
- No direct unit tests for `create_oss_provider` environment-variable precedence.

## Design observations

### Strengths

- The crate has a clean boundary: config modeling and normalization live here, transport execution does not.
- It uses small, stable value types and pure functions, which keeps most behavior deterministic and testable.
- Schema derivation and serde derivation on the same types reduce drift between implementation and config documentation.
- Auth concerns are separated into config-time description here and runtime resolution elsewhere, which avoids overloading this crate.

### Trade-offs

- Validation is intentionally distributed across crates, which keeps responsibilities separated but makes the full rule set harder to discover from this crate alone.
- Provider identity is split across provider ID keys and `name` values; for OSS providers the `name` is shared (`"gpt-oss"`), so callers must treat the map key as the real identity.
- The merge model favors stability of built-in providers over user override flexibility.

## Open questions

1. The crate docs say configured providers “override or extend” built-ins, but `merge_configured_model_providers` mostly prevents overriding and only inserts missing keys. Is the top-level documentation outdated, or is the merge policy intentionally stricter now?
2. Should invalid `http_headers` / `env_http_headers` be surfaced as configuration errors instead of being silently dropped?
3. Should `to_api_provider` and `build_header_map` be tested directly, especially for the ChatGPT base URL branch and env-header behavior?
4. Should OSS provider factories expose provider-specific names instead of using the shared `"gpt-oss"` display name for both Ollama and LM Studio?
5. Should `CODEX_OSS_BASE_URL` and `CODEX_OSS_PORT` apply globally to both OSS built-ins, or should each provider have independent overrides?
6. AWS websocket support is explicitly blocked with a TODO in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L143-L149). Is supporting SigV4-signed websocket upgrades on the roadmap, and would that require extending the API/provider abstraction?

## Short conclusion

`codex-model-provider-info` is a compact but important configuration boundary crate. It defines the provider schema, owns built-in provider defaults, enforces key compatibility rules, and adapts provider definitions into the lower-level API client representation. The implementation is straightforward and mostly side-effect free, with the main complexity concentrated in auth-mode compatibility, merge policy, and the distinction between config-time provider metadata and runtime request construction.
