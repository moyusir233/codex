# codex-keyring-store analysis

## Scope and purpose

`codex-keyring-store` is a very small adapter crate that gives the workspace a stable, mockable API for storing secrets in the OS keyring. The crate does **not** own higher-level credential formats, fallback files, encryption policy, or persistence orchestration. It only exposes a synchronous interface for:

- loading a string secret by `(service, account)`;
- saving a string secret by `(service, account)`;
- deleting a secret by `(service, account)`.

This keeps consumers decoupled from the `keyring` crate and from OS-specific backend details.

## File map

- `Cargo.toml` — declares the crate and selects OS-specific `keyring` features.
- `src/lib.rs` — contains all production code, the trait abstraction, the default implementation, the error wrapper, and a reusable mock module.
- `BUILD.bazel` — Bazel wrapper for the Rust crate.

There are no additional modules, integration tests, benches, or examples inside this crate.

## Concrete responsibilities

### 1. Define the workspace-facing credential store abstraction

`KeyringStore` is the crate’s main contract:

- `load(&self, service, account) -> Result<Option<String>, CredentialStoreError>`
- `save(&self, service, account, value) -> Result<(), CredentialStoreError>`
- `delete(&self, service, account) -> Result<bool, CredentialStoreError>`

Important semantics:

- missing entries are treated as normal control flow;
- `load` uses `Ok(None)` for “not found”;
- `delete` uses `Ok(false)` for “nothing to delete”;
- all other backend failures are surfaced as `CredentialStoreError`.

This contract is object-safe and bounded by `Debug + Send + Sync`, so consumers can pass it around as `Arc<dyn KeyringStore>`.

### 2. Provide the default OS-backed implementation

`DefaultKeyringStore` is a zero-sized type that directly delegates to `keyring::Entry`:

- constructs an entry with `Entry::new(service, account)`;
- calls `get_password`, `set_password`, and `delete_credential`;
- translates `keyring::Error::NoEntry` into `None` / `false`;
- preserves all other backend errors.

The implementation is intentionally thin. It does not cache entries, normalize names, or add retries.

### 3. Provide a shared mock for other crates’ tests

The public `tests` module exposes `MockKeyringStore`, which uses `keyring::mock::MockCredential` behind an `Arc<Mutex<HashMap<String, Arc<MockCredential>>>>`.

That mock is used by other crates to test their own fallback and orchestration logic without talking to the real OS keychain.

## Public API surface

### `CredentialStoreError`

This is currently a wrapper enum with one variant:

- `Other(KeyringError)`

Exposed helpers:

- `new(error: KeyringError) -> Self`
- `message(&self) -> String`
- `into_error(self) -> KeyringError`

Observations:

- the enum is structurally ready for future expansion, but today it is effectively a single-case wrapper;
- `message()` exists because several downstream callers want a displayable string without taking ownership;
- `into_error()` allows consumers that use `anyhow` or `std::io::Error` to rewrap the underlying `keyring::Error`.

### `KeyringStore`

This is the main extension point. Consumers depend on the trait instead of the `keyring` crate directly.

### `DefaultKeyringStore`

This is the production implementation for all supported platforms.

### `tests::MockKeyringStore`

Although named like a test-only helper, it is public from the crate and compiled for normal builds too. The workspace uses it as shared test infrastructure.

## Dependency and platform analysis

### Direct dependencies

- `keyring` — provides the actual OS keychain integration.
- `tracing` — emits trace-level lifecycle logs for load/save/delete operations.

### Workspace dependency setup

The workspace pins `keyring = { version = "3.6", default-features = false }`, then this crate enables backend features per target OS.

### Target-specific backend selection

`Cargo.toml` selects one backend family per platform:

- Linux: `linux-native-async-persistent`
- macOS: `apple-native`
- Windows: `windows-native`
- FreeBSD/OpenBSD: `sync-secret-service`

In addition, the base dependency enables `crypto-rust`.

Implications:

- consumers do not need to care about `keyring` feature flags;
- the crate centralizes backend choice for the whole workspace;
- Linux behavior may differ more significantly from other OSes because the selected backend is the async persistent/native path exposed by `keyring`.

## Control flow

### Load flow

1. Consumer calls `KeyringStore::load(service, account)`.
2. `DefaultKeyringStore` logs a trace start event.
3. It creates `keyring::Entry`.
4. It calls `get_password()`.
5. Outcomes:
   - success => `Ok(Some(password))`
   - `NoEntry` => `Ok(None)`
   - any other error => `Err(CredentialStoreError::Other(...))`

### Save flow

1. Consumer calls `save(service, account, value)`.
2. The store logs start information, including only `value_len`, not the secret itself.
3. It creates `keyring::Entry`.
4. It calls `set_password(value)`.
5. Outcomes:
   - success => `Ok(())`
   - error => wrapped failure

### Delete flow

1. Consumer calls `delete(service, account)`.
2. The store creates `keyring::Entry`.
3. It calls `delete_credential()`.
4. Outcomes:
   - success => `Ok(true)`
   - `NoEntry` => `Ok(false)`
   - other error => wrapped failure

## Where the crate is used

The crate is intentionally low-level, and most interesting behavior lives in downstream crates.

### `codex-secrets`

`codex-secrets` injects `Arc<dyn KeyringStore>` into `LocalSecretsBackend` and uses the keyring only to hold the passphrase that encrypts a local `age` file. That means:

- `codex-keyring-store` does **not** store every secret directly;
- instead, it stores the encryption key for the file-backed secret vault.

### `codex-login`

`codex-login` uses the trait for auth storage backends and implements file/keyring/auto/ephemeral modes on top. In auto mode it falls back to `auth.json` when keyring operations fail.

### `codex-rmcp-client`

`codex-rmcp-client` uses the trait for OAuth credential persistence, again layering fallback-to-file behavior above this crate.

Design takeaway: this crate deliberately stops at “OS keyring adapter,” while policy and recovery live elsewhere.

## Testing analysis

### What this crate tests directly

As of this snapshot, `cargo test -p codex-keyring-store` runs **0 tests** and **0 doctests** for the crate itself.

So the crate currently has:

- no unit tests for `DefaultKeyringStore`;
- no unit tests for `CredentialStoreError`;
- no crate-local tests for logging behavior or `NoEntry` mapping.

### What is tested indirectly

The reusable `MockKeyringStore` is heavily exercised by downstream crates:

- `login/src/auth/storage_tests.rs`
- `secrets/src/local.rs` tests
- `rmcp-client/src/oauth.rs` tests

Those tests validate that downstream storage layers react correctly to:

- missing entries;
- successful saves;
- successful deletes;
- injected backend errors.

### Consequence

The workspace gets decent end-to-end confidence in trait semantics, but there is little direct protection against regressions inside `DefaultKeyringStore` itself.

## Design characteristics

### Strong points

- Very small API surface.
- Easy to mock through a trait object.
- Explicit handling of “not found” as non-error control flow.
- Centralized OS backend configuration in one crate.
- Trace logging avoids printing secret contents.
- Shared mock reduces duplicated test helpers across the workspace.

### Trade-offs and limitations

- Synchronous API only; no async abstraction.
- No retry, backoff, or capability probing.
- Error type adds only a thin wrapper today.
- `tests` helper module is shipped in normal builds, not only test builds.
- No direct crate-local tests.

## Notable implementation details

### Error normalization

The crate intentionally treats `NoEntry` differently from all other backend failures. That matches how callers want to model absent credentials and avoids forcing them to pattern-match on `keyring::Error`.

### Security-conscious logging

`save` logs `value_len` instead of the secret. This is a good default: useful for debugging call shape without leaking payloads.

### Mock behavior differs from production in one way

`MockKeyringStore` keys its internal map by `account` only and ignores `service`. Production behavior is scoped by both `service` and `account`.

That is fine for current tests, because callers typically control the key string tightly, but it means the mock does not fully model namespace separation across services.

## Open questions

1. Should `tests::MockKeyringStore` move behind `#[cfg(test)]` or a dedicated `test-utils` feature?
   - Current design is convenient for cross-crate tests, but it also exposes test scaffolding in normal library builds.

2. Should the mock key by `(service, account)` instead of just `account`?
   - That would align test behavior more closely with the real backend and avoid false positives if two services share an account name.

3. Should the crate add crate-local tests for the `NoEntry => None/false` contract?
   - Right now that contract is only guarded indirectly by downstream usage.

4. Does `CredentialStoreError` need to stay as a one-variant enum?
   - A newtype or `thiserror`-based transparent wrapper could be simpler unless more variants are expected soon.

5. Should there be a dedicated public test-support module name instead of `tests`?
   - `pub mod tests` suggests “internal tests,” but it is effectively part of the crate’s reusable API for the workspace.

6. Should trace logs include structured fields instead of formatted strings?
   - The current logs are readable, but structured fields would improve filtering and downstream observability.

## Bottom line

`codex-keyring-store` is a deliberately minimal boundary crate. Its value is not complex business logic; its value is architectural:

- it isolates the workspace from the raw `keyring` API;
- it standardizes missing-entry behavior;
- it centralizes target-specific backend selection;
- it provides a shared mock so higher-level crates can test fallback behavior cleanly.

The main improvement opportunities are around direct tests, naming/export strategy for test utilities, and making the mock match production namespacing more closely.
