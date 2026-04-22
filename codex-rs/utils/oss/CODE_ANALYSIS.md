## Crate: `codex-utils-oss`

Path: [/Users/bytedance/project/codex/codex-rs/utils/oss](file:///Users/bytedance/project/codex/codex-rs/utils/oss)

Primary source: [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/oss/src/lib.rs)

---

## 1. Responsibilities and Role

- Provide a **thin, shared OSS helper layer** used by both:
  - the non-interactive `exec` binary ([exec/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs)), and
  - the interactive `tui` binary ([tui/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/lib.rs)),
  so they can make consistent decisions about:
  - which default model to use for a given local OSS provider ID, and
  - how to bootstrap the chosen OSS provider (ensure server is reachable, model present, and feature support).

- Encapsulate **provider-specific logic** (LM Studio vs. Ollama) behind a single API surface:
  - delegates to `codex-lmstudio` for LM Studio-specific behavior,
  - delegates to `codex-ollama` for Ollama-specific behavior.

- Act as the **glue** between:
  - higher-level configuration (`codex-core::config::Config` and provider IDs from `codex-model-provider-info`),
  - and provider-specific crates that own the real orchestration logic and HTTP/CLI integration.

This crate intentionally stays very small and stateless: it does not own configuration parsing, network behavior, or CLI invocation; it simply routes based on provider ID.

---

## 2. Public API Surface

All items live in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/oss/src/lib.rs).

### 2.1 `get_default_model_for_oss_provider`

```rust
pub fn get_default_model_for_oss_provider(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        LMSTUDIO_OSS_PROVIDER_ID => Some(codex_lmstudio::DEFAULT_OSS_MODEL),
        OLLAMA_OSS_PROVIDER_ID => Some(codex_ollama::DEFAULT_OSS_MODEL),
        _ => None,
    }
}
```

- **Purpose**
  - Map from an OSS provider identifier string (e.g. `"lmstudio"`, `"ollama"`) into that provider’s default OSS model ID.
  - Provide a single place where default model choices live, so binaries (`tui`, `exec`) do not have to reach into provider crates directly.

- **Inputs**
  - `provider_id: &str`
    - Typically one of:
      - [`LMSTUDIO_OSS_PROVIDER_ID`](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L389), value `"lmstudio"`,
      - [`OLLAMA_OSS_PROVIDER_ID`](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L390), value `"ollama"`.

- **Outputs**
  - `Option<&'static str>`:
    - `Some(DEFAULT_OSS_MODEL)` for supported providers:
      - LM Studio → [`codex_lmstudio::DEFAULT_OSS_MODEL`](file:///Users/bytedance/project/codex/codex-rs/lmstudio/src/lib.rs#L5-L7), currently `"openai/gpt-oss-20b"`.
      - Ollama → [`codex_ollama::DEFAULT_OSS_MODEL`](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L11-L13), currently `"gpt-oss:20b"`.
    - `None` for unknown provider IDs.

- **Key call sites**
  - `exec` bootstrap:
    - [exec/src/lib.rs#L372-L380](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L372-L380):
      - If `--oss` is enabled and the user did not pass `-m/--model`, `exec` uses `get_default_model_for_oss_provider` along with the resolved provider ID to set `ConfigOverrides::model`.
  - `tui` bootstrap:
    - [tui/src/lib.rs#L808-L816](file:///Users/bytedance/project/codex/codex-rs/tui/src/lib.rs#L808-L816):
      - Same behavior, but the provider might be selected interactively by the TUI if not preconfigured.

### 2.2 `ensure_oss_provider_ready`

```rust
pub async fn ensure_oss_provider_ready(
    provider_id: &str,
    config: &Config,
) -> Result<(), std::io::Error> {
    match provider_id {
        LMSTUDIO_OSS_PROVIDER_ID => {
            codex_lmstudio::ensure_oss_ready(config)
                .await
                .map_err(|e| std::io::Error::other(format!("OSS setup failed: {e}")))?;
        }
        OLLAMA_OSS_PROVIDER_ID => {
            codex_ollama::ensure_responses_supported(&config.model_provider).await?;
            codex_ollama::ensure_oss_ready(config)
                .await
                .map_err(|e| std::io::Error::other(format!("OSS setup failed: {e}")))?;
        }
        _ => {
            // Unknown provider, skip setup
        }
    }
    Ok(())
}
```

- **Purpose**
  - Provide a **unified readiness hook** for local OSS providers:
    - For LM Studio: ensure server reachability, model presence, and best-effort warm-up.
    - For Ollama: additionally enforce a minimum Ollama version that supports the Responses API before bootstrapping.

- **Inputs**
  - `provider_id: &str`
    - Same expected values as for `get_default_model_for_oss_provider`.
  - `config: &Config` (`codex_core::config::Config`)
    - Must contain:
      - provider catalog including built-in OSS providers, as configured in `config.toml` / profiles,
      - the chosen `model` (may be `None` – provider crates have their own defaulting),
      - `config.model_provider: Option<String>` (used by `codex_ollama::ensure_responses_supported`).

- **Outputs**
  - `Result<(), std::io::Error>`:
    - `Ok(())` if:
      - the selected provider is unknown (setup is skipped), or
      - the underlying provider crate completed its readiness checks and model preparation without returning an error (modulo their own non-fatal logging policies).
    - `Err(std::io::Error)` if:
      - provider-specific `ensure_oss_ready` functions return an error, which is wrapped as `std::io::Error::other("OSS setup failed: {…}")`.
      - for Ollama, if `ensure_responses_supported` fails with an IO error (e.g. version too old).

- **Provider-specific behavior**
  - **LM Studio path**
    - Delegates to [`codex_lmstudio::ensure_oss_ready`](file:///Users/bytedance/project/codex/codex-rs/lmstudio/src/lib.rs#L11-L45):
      - Picks model = `config.model.unwrap_or(DEFAULT_OSS_MODEL)`.
      - Builds an `LMStudioClient` and checks server reachability.
      - Calls `fetch_models`; if the model is missing, runs a CLI download.
      - Spawns a background task to `load_model` for warm-up.
      - Logs warnings on some non-fatal failures (e.g. model listing or warm-up), but still returns `Ok(())`.
  - **Ollama path**
    - Calls [`codex_ollama::ensure_responses_supported`](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L59-L76) first:
      - Ensures the local Ollama version is compatible with the Responses API (soft behavior: missing/unparseable version often returns `Ok(())`).
    - Then delegates to [`codex_ollama::ensure_oss_ready`](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L15-L47):
      - Picks model = `config.model.unwrap_or(DEFAULT_OSS_MODEL)`.
      - Builds an `OllamaClient`, checks server reachability.
      - Uses `fetch_models` and pulls the model via CLI if missing.
      - Logs warnings on non-fatal failures fetching the model list.

- **Unknown provider behavior**
  - For any `provider_id` not equal to the known constants:
    - The function performs **no setup** and returns `Ok(())`.
    - This relies on earlier configuration validation (e.g. [`validate_oss_provider`](file:///Users/bytedance/project/codex/codex-rs/config/src/config_toml.rs#L838-L853)) to prevent unsupported IDs from being chosen in normal flows.

---

## 3. Control Flow and Integration

### 3.1 OSS provider resolution (outside this crate)

Provider selection is handled upstream, primarily in `codex-core` and the binaries:

- [`resolve_oss_provider`](file:///Users/bytedance/project/codex/codex-rs/core/src/config/mod.rs#L1392-L1416):
  - Accepts an explicit provider (e.g. CLI `--local-provider`).
  - Otherwise walks through:
    - selected config profile’s `oss_provider` field (if any),
    - global `ConfigToml::oss_provider`,
  - Returns `Option<String>` for the chosen OSS provider ID string.

- `config` layer validation:
  - [`validate_oss_provider`](file:///Users/bytedance/project/codex/codex-rs/config/src/config_toml.rs#L838-L853) ensures that only:
    - `LMSTUDIO_OSS_PROVIDER_ID` (`"lmstudio"`) or
    - `OLLAMA_OSS_PROVIDER_ID` (`"ollama"`)
    are accepted in TOML, with a special error path for a removed legacy Ollama chat provider.

### 3.2 Usage in `exec`

In [exec/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/exec/src/lib.rs#L354-L395):

- `model_provider` is resolved when `--oss` is passed:
  - `resolve_oss_provider(oss_provider.as_deref(), &config_toml, config_profile.clone())`.
  - If `None`, `exec` errors out with a message listing valid providers and suggesting config changes.
- Model selection:
  - If `-m/--model` was provided, that wins.
  - Else, when `oss == true`, it calls:
    - `model_provider.as_ref().and_then(|provider_id| get_default_model_for_oss_provider(provider_id))`.
  - The resulting model ID is stored into `ConfigOverrides::model`.
- The resulting `Config` (merged with overrides) is later passed to provider crates and, indirectly, to `ensure_oss_provider_ready` when `exec` runs OSS bootstrap logic.

### 3.3 Usage in `tui`

In [tui/src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/tui/src/lib.rs#L788-L837):

- `model_provider_override` is resolved similarly:
  - `resolve_oss_provider(…, &config_toml, cli.config_profile.clone())`.
  - If no provider is configured:
    - The TUI opens an OSS provider selection UI (`oss_selection::select_oss_provider`), writing the choice back to config.
    - If the user cancels, it returns an IO error.
- Model selection:
  - If CLI model is set, use it.
  - Else if `cli.oss`:
    - Use `model_provider_override.as_ref().and_then(get_default_model_for_oss_provider)` to set `ConfigOverrides::model`.

`tui` then proceeds to launch the app with this configuration. When starting in OSS mode, the app can use `ensure_oss_provider_ready` to prepare the local environment before initiating sessions.

---

## 4. Dependencies

### 4.1 Direct crate dependencies

Declared in [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/utils/oss/Cargo.toml#L10-L14):

- `codex-core`
  - Used for the shared `Config` type ([core/src/config](file:///Users/bytedance/project/codex/codex-rs/core/src/config/mod.rs)).
  - `ensure_oss_provider_ready` takes `&Config` to align with the rest of the system config model.
- `codex-lmstudio`
  - Provides:
    - `DEFAULT_OSS_MODEL` ([lmstudio/src/lib.rs#L5-L7](file:///Users/bytedance/project/codex/codex-rs/lmstudio/src/lib.rs#L5-L7)).
    - `async fn ensure_oss_ready(config: &Config) -> io::Result<()>` ([lmstudio/src/lib.rs#L11-L45](file:///Users/bytedance/project/codex/codex-rs/lmstudio/src/lib.rs#L11-L45)).
  - Encapsulates the LM Studio-specific HTTP and CLI behavior.
- `codex-model-provider-info`
  - Provides the canonical provider IDs:
    - [`LMSTUDIO_OSS_PROVIDER_ID`](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L389) = `"lmstudio"`.
    - [`OLLAMA_OSS_PROVIDER_ID`](file:///Users/bytedance/project/codex/codex-rs/model-provider-info/src/lib.rs#L390) = `"ollama"`.
  - Centralizes provider metadata so this crate does not hard-code string literals directly.
- `codex-ollama`
  - Provides:
    - `DEFAULT_OSS_MODEL` ([ollama/src/lib.rs#L11-L13](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L11-L13)).
    - `async fn ensure_oss_ready(config: &Config) -> io::Result<()>` ([ollama/src/lib.rs#L15-L47](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L15-L47)).
    - `async fn ensure_responses_supported(model_provider: &Option<String>) -> io::Result<()>` ([ollama/src/lib.rs#L59-L76](file:///Users/bytedance/project/codex/codex-rs/ollama/src/lib.rs#L59-L76)).
  - Encapsulates the Ollama-specific HTTP and CLI behavior and version gating for Responses support.

### 4.2 Indirect ecosystem dependencies

All heavy lifting (HTTP clients, async runtimes, CLI spawning, semver parsing, logging) lives in the provider crates (`lmstudio`, `ollama`) and core config. `codex-utils-oss` itself has:

- no direct external dependencies,
- no async runtime coupling beyond returning `Future<Output = io::Result<()>>` from `ensure_oss_provider_ready`.

---

## 5. Testing

All tests are currently unit tests embedded in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/utils/oss/src/lib.rs#L40-L61).

### 5.1 Unit tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_default_model_for_provider_lmstudio() {
        let result = get_default_model_for_oss_provider(LMSTUDIO_OSS_PROVIDER_ID);
        assert_eq!(result, Some(codex_lmstudio::DEFAULT_OSS_MODEL));
    }

    #[test]
    fn test_get_default_model_for_provider_ollama() {
        let result = get_default_model_for_oss_provider(OLLAMA_OSS_PROVIDER_ID);
        assert_eq!(result, Some(codex_ollama::DEFAULT_OSS_MODEL));
    }

    #[test]
    fn test_get_default_model_for_provider_unknown() {
        let result = get_default_model_for_oss_provider("unknown-provider");
        assert_eq!(result, None);
    }
}
```

- **Coverage**
  - Verifies the mapping from provider ID → default model for both supported providers and an unknown provider.
  - Does **not** test:
    - `ensure_oss_provider_ready`.
    - error propagation semantics.
    - interaction with config validation or provider crates.

- **Testing style**
  - Pure synchronous unit tests without mocking or test doubles.
  - Relies only on constants and static defaults, so tests are stable and cheap.

### 5.2 Higher-level tests (outside this crate)

Behavior of the overall OSS flow (provider resolution + readiness + model selection) is exercised more comprehensively in:

- `codex-lmstudio` tests and analysis ([CODE_ANALYSIS.md](file:///Users/bytedance/project/codex/codex-rs/lmstudio/CODE_ANALYSIS.md)).
- `codex-ollama` tests and analysis ([CODE_ANALYSIS.md](file:///Users/bytedance/project/codex/codex-rs/ollama/CODE_ANALYSIS.md)).
- Configuration validation in `codex-config` ([config/src/config_toml.rs](file:///Users/bytedance/project/codex/codex-rs/config/src/config_toml.rs#L838-L853)).
- Binary integration tests in `exec`/`tui` crates (not enumerated here).

---

## 6. Design Notes and Rationale

- **Thin facade, not a new abstraction layer**
  - The crate keeps logic minimal:
    - no new types,
    - no own configuration structs,
    - no additional logging or telemetry.
  - The main goal is to avoid code duplication between `exec` and `tui` when dealing with OSS providers.

- **Stringly-typed provider IDs**
  - Provider IDs are plain strings throughout (`&str`, `String`), but sourced exclusively from `codex-model-provider-info`.
  - `match` statements in this crate depend on those constants to keep comparisons centralized and typo-safe.
  - Config validation (`validate_oss_provider`) catches invalid IDs early; `ensure_oss_provider_ready` treats unknown IDs as a no-op.

- **Error type choice (`std::io::Error`)**
  - `ensure_oss_provider_ready` returns `io::Result<()>` instead of `anyhow::Result` or a custom error enum.
  - This aligns with many other utilities in the workspace that represent user/environmental failures as IO errors.
  - Provider crate errors are wrapped into `ErrorKind::Other` with a formatted message when appropriate.

- **Provider-specific responsibilities**
  - This crate deliberately **does not**:
    - know which ports services listen on,
    - know which HTTP endpoints are used,
    - run or manage the provider daemons.
  - All such behavior belongs to:
    - `codex-lmstudio` ([analysis](file:///Users/bytedance/project/codex/codex-rs/lmstudio/CODE_ANALYSIS.md)),
    - `codex-ollama` ([analysis](file:///Users/bytedance/project/codex/codex-rs/ollama/CODE_ANALYSIS.md)).

- **Version gating for Ollama only**
  - The extra check via `ensure_responses_supported` exists only for Ollama:
    - LM Studio presumably always provides the Responses-like API used elsewhere, or uses a different stability story;
    - Ollama required a minimum version before the Responses API became available, so the guard prevents subtle runtime failures.

---

## 7. Open Questions and Potential Improvements

These are not current bugs, but questions or design opportunities observed from the crate’s current shape and its integration points.

1. **Should unknown providers return an error instead of a no-op?**
   - Today, `ensure_oss_provider_ready` silently does nothing for unknown provider IDs and returns `Ok(())`.
   - Upstream validation *should* prevent this scenario, but if a new provider ID is introduced without updating this crate, readiness would be silently skipped.
   - An alternative design is to return an `io::Error` for unknown IDs, or to expose an explicit enum of supported OSS providers and avoid arbitrary strings at this boundary.

2. **Stronger typing for provider IDs**
   - The workspace already centralizes provider metadata in `codex-model-provider-info`.
   - This crate could introduce a small enum (e.g. `enum OssProvider { LmStudio, Ollama }`) with a conversion from string, allowing compiler-enforced exhaustiveness when new providers are added.

3. **Expose richer readiness outcomes**
   - Provider crates already differentiate between fatal and non-fatal failures internally (e.g. connectivity vs. model listing vs. warm-up).
   - `ensure_oss_provider_ready` currently collapses everything into `io::Result<()>`.
   - A more descriptive return type (e.g. a `ReadyStatus` enum and/or structured error type) could let callers distinguish:
     - “server unreachable,”
     - “model download failed,”
     - “model warm-up scheduled but not confirmed,”
     - “Ollama version does not support Responses API.”

4. **Testing `ensure_oss_provider_ready` itself**
   - The crate does not contain tests for the readiness function.
   - Lightweight tests using mocked versions of `codex_lmstudio::ensure_oss_ready` / `codex_ollama::*` (or a feature-gated test module) could:
     - verify that the correct branch is taken for each provider ID,
     - assert that errors are wrapped as `io::Error::other("OSS setup failed: …")`,
     - confirm that `ensure_responses_supported` is invoked before `ensure_oss_ready` for the Ollama branch.

5. **Future provider extensibility**
   - Adding a new OSS provider today requires touching:
     - `codex-model-provider-info` (new ID and metadata),
     - `codex-config` validation (`validate_oss_provider`),
     - this crate’s `get_default_model_for_oss_provider` and `ensure_oss_provider_ready`,
     - upstream consumers that present provider choices (e.g. TUI’s OSS selection UI).
   - If the number of providers grows, this crate could evolve towards a registry or trait-based dispatch to reduce the number of coordinated edits required.

6. **Consistency between `tui` and `exec` flows**
   - Both binaries use this crate for mapping provider → default model, but their handling of “no provider configured” differs:
     - `exec` errors out immediately with a message.
     - `tui` falls back to an interactive picker.
   - The crate itself is agnostic of this difference, but any future changes to defaulting or provider semantics should ensure that both call sites remain consistent with their UX goals.

---

## 8. Summary

`codex-utils-oss` is a deliberately small, focused utility crate that:

- centralizes mapping from OSS provider IDs to default models, and
- provides a unified readiness hook that delegates to provider-specific crates.

It avoids duplicating provider-specific logic across binaries, keeps the OSS integration points consistent, and defers complex behavior to dedicated crates (`lmstudio`, `ollama`). Future evolution may introduce stronger typing for providers and richer readiness reporting, but the current design is simple, pragmatic, and idiomatic for a workspace-level utility.
