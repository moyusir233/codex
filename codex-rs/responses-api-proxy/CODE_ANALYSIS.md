# CODE_ANALYSIS: codex-responses-api-proxy

## Overview

`codex-responses-api-proxy` is a small, security-oriented HTTP proxy crate that exposes a single local endpoint, accepts only `POST /v1/responses`, injects a privileged `Authorization: Bearer <key>` header read from `stdin`, and forwards the call to a configured upstream Responses API URL. The implementation is intentionally narrow: it is designed to let a more privileged user hold the OpenAI-compatible API key while less-privileged local processes can talk only to a constrained loopback proxy.

The crate is split into:

- a library crate exposing [Args](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L37-L59) and [run_main](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L72-L136)
- a binary entry point in [main.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/main.rs#L1-L12)
- two internal modules:
  - [read_api_key.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L1-L342) for protected API-key ingestion
  - [dump.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L1-L378) for redacted request/response dumps

The crate README in [README.md](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/README.md#L1-L110) is accurate and useful because it documents both the threat model and the intended multi-user workflow.

## Manifest and packaging

The manifest is [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/Cargo.toml#L1-L31).

- Package name: `codex-responses-api-proxy`
- Library target: `codex_responses_api_proxy`
- Binary target: `codex-responses-api-proxy`
- Workspace-inherited metadata: version, edition, license, lints
- Bazel exposure: [BUILD.bazel](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/BUILD.bazel#L1-L6)

The crate also has an npm wrapper directory under [npm](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/npm), but the Rust crate itself does not depend on it at runtime.

## Core responsibilities

The crate has a deliberately small responsibility surface:

1. Read an API key from `stdin` and retain it with extra care in memory.
2. Bind a loopback HTTP listener on a fixed or ephemeral port.
3. Accept only `POST /v1/responses` and reject everything else with `403`.
4. Forward accepted requests to a configured upstream URL with caller headers preserved except for `Authorization` and `Host`.
5. Mirror the upstream response back to the local caller, including streaming bodies.
6. Optionally expose `GET /shutdown` as a cooperative local termination hook.
7. Optionally emit startup metadata and redacted request/response dumps.

This is not a general reverse proxy. It does not do routing, retries, authentication negotiation, multi-tenant policy, or request transformation beyond strict header replacement.

## Crate structure

- [src/main.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/main.rs#L1-L12)
  - Installs pre-main process hardening with `#[ctor::ctor]`.
  - Parses CLI args and delegates into the library.
- [src/lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L1-L275)
  - Owns the main server lifecycle, request validation, forwarding logic, and server-info writing.
- [src/read_api_key.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L1-L342)
  - Owns API-key ingestion, validation, zeroization, and `mlock` behavior.
- [src/dump.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L1-L378)
  - Owns JSON dump creation, response-body teeing, and header redaction.

## Public API

The public Rust API is intentionally tiny.

### `Args`

Defined in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L37-L59).

Fields:

- `port: Option<u16>`: bind `127.0.0.1:<port>` or use an ephemeral port
- `server_info: Option<PathBuf>`: write a one-line JSON startup file
- `http_shutdown: bool`: enable `GET /shutdown`
- `upstream_url: String`: absolute upstream URL, defaulting to OpenAI Responses
- `dump_dir: Option<PathBuf>`: write redacted request/response dumps

This type is primarily a CLI surface model rather than a reusable configuration API.

### `run_main(args: Args) -> anyhow::Result<()>`

Defined in [run_main](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L72-L136).

Responsibilities:

- read and protect the auth header from `stdin`
- parse and normalize the upstream URL into [ForwardConfig](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L67-L70)
- create the optional dumper
- bind the listener and optionally write startup info
- construct a `reqwest::blocking::Client` with timeout disabled
- accept requests from `tiny_http` and spawn one worker thread per request

Although `run_main` is public, the crate still behaves like an executable-focused component rather than a reusable library.

## Entry and startup flow

Startup spans [main.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/main.rs#L1-L12), [run_main](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L72-L136), and [read_auth_header_from_stdin](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L11-L18).

Execution sequence:

1. `#[ctor::ctor]` runs [pre_main](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/main.rs#L4-L7), which calls `codex_process_hardening::pre_main_hardening()`.
2. The binary parses CLI flags with `clap` and calls [run_main](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/main.rs#L9-L11).
3. [run_main](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L73-L95) reads the bearer token from `stdin`, parses the upstream URL, computes the `Host` header, and creates optional dumping state.
4. It binds a local TCP listener via [bind_listener](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L138-L143) and optionally writes `{port, pid}` via [write_server_info](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L145-L160).
5. It creates a blocking `reqwest` client with timeout disabled so streaming responses are not cut off.
6. For each incoming `tiny_http` request, it spawns a dedicated OS thread and either:
   - terminates the process on `GET /shutdown` when enabled, or
   - forwards the request through [forward_request](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L163-L275).

The flow is intentionally simple and synchronous. There is no async runtime.

## Request/response flow

The critical logic lives in [forward_request](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L163-L275).

### Request admission

The admission rule is extremely strict:

- only `Method::Post`
- only exact URL path `"/v1/responses"`
- no query string or alternate path accepted

Anything else gets an immediate empty `403` response in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L170-L179).

### Request body handling

Accepted requests are fully buffered into memory before forwarding in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L181-L194).

Implications:

- request uploads are not streamed upstream
- dump generation can inspect the entire request body
- large request bodies increase memory usage linearly per request

### Header forwarding

Header propagation happens in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L196-L221).

Behavior:

- all incoming headers are iterated
- `Authorization` and `Host` are removed
- other headers are copied into a `reqwest::header::HeaderMap` if they parse cleanly
- the proxy inserts:
  - a sensitive `Authorization` header built from the protected static string
  - a `Host` header matching the configured upstream URL

This gives the caller influence over most metadata while keeping credential control centralized.

### Upstream forwarding

The actual upstream call is a simple blocking `POST` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L223-L228):

- URL: configured `upstream_url`
- method: always `POST`
- body: buffered request bytes
- headers: forwarded caller headers plus injected auth/host

There is no retry logic, timeout policy, or backoff. Upstream failures surface as an `anyhow` error and are only logged with `eprintln!`.

### Response bridging

The proxy converts a `reqwest::blocking::Response` into a `tiny_http::Response` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L230-L274).

Key details:

- it preserves the upstream status code
- it forwards most upstream headers except transport-managed headers such as:
  - `content-length`
  - `transfer-encoding`
  - `connection`
  - `trailer`
  - `upgrade`
- it uses the upstream body directly as a `Read` implementation
- when dumping is enabled, it wraps the body in a tee adapter so the same bytes are streamed to the client and captured to disk

This is a good fit for Server-Sent Events and long-lived Responses API streams because the response body is not eagerly buffered.

## API-key handling and security model

The most security-sensitive code is in [read_api_key.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L1-L342).

### What the module does

- prepends `Bearer ` directly into a fixed stack buffer
- reads the API key from `stdin` into the remaining bytes
- trims trailing newline characters
- validates that the key contains only ASCII alphanumeric, `-`, and `_`
- copies the final header string once into heap storage
- zeroizes the original stack buffer with `zeroize`
- leaks the string to obtain a process-lifetime `&'static str`
- `mlock`s the backing memory on Unix

The main implementation is [read_auth_header_with](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L72-L162), with low-level Unix reading in [read_from_unix_stdin](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L32-L70) and page-aligned locking in [mlock_str](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L164-L204).

### Why the `&'static str` design exists

`forward_request` receives `auth_header: &'static str` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L163-L169). That allows each worker thread to cheaply copy the reference by value without cloning the secret contents. The code then constructs `HeaderValue::from_static(auth_header)` and marks it sensitive in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L215-L219).

This is a pragmatic design: it intentionally leaks one heap allocation so the secret has stable process-lifetime storage and avoids repeated reallocation or copying.

### Unsafe code and safety assumptions

Unsafe behavior is limited but important:

- `libc::read` in [read_from_unix_stdin](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L47-L54)
- `libc::mlock` and `libc::sysconf` in [mlock_str](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L175-L200)

Safety assumptions:

- the raw `read(2)` call is given a valid writable buffer
- the computed page-aligned region passed to `mlock` correctly covers the string allocation
- one intentionally leaked string for the process lifetime is acceptable

The implementation is careful and the comments are strong, but this is the highest-risk code in the crate.

## Dumping and observability

The optional dumping subsystem is implemented in [dump.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L1-L378).

### ExchangeDumper

[ExchangeDumper](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L19-L61) owns:

- a dump directory path
- an atomic sequence counter
- request dump creation with a sequence-and-timestamp prefix

For each accepted request it writes:

- `<prefix>-request.json`
- `<prefix>-response.json`

The request file is written immediately; the response file is written after the response body is consumed or dropped.

### Response teeing

[ExchangeDump::tee_response_body](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L67-L83) wraps the upstream body into [ResponseBodyDump](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L85-L134), which:

- implements `Read`
- appends streamed bytes into an in-memory `Vec<u8>`
- writes the response dump once at EOF or on `Drop`

This is a solid design because it preserves streaming semantics while still capturing the full downstream body for diagnostics.

### Redaction policy

Header redaction is centralized in [should_redact_header](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L186-L189).

It redacts:

- `Authorization`
- any header whose lowercased name contains `cookie`

Bodies are serialized by [dump_body](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L191-L194):

- parse as JSON when possible
- otherwise store as lossy UTF-8 text

This is useful operationally, but it is intentionally not a full secret scanner. Sensitive data inside JSON bodies is not redacted.

## Dependencies

Direct dependencies from [Cargo.toml](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/Cargo.toml#L18-L31):

- `anyhow`
  - error propagation and context
- `clap` with `derive`
  - CLI argument parsing for `Args`
- `codex-process-hardening`
  - pre-main process hardening in the binary entry point
- `ctor`
  - early constructor hook for hardening
- `libc`
  - raw Unix `read` and `mlock`
- `reqwest` with `blocking`, `json`, `rustls-tls`
  - blocking HTTP client and header types
- `serde` and `serde_json`
  - server-info serialization and dump files
- `tiny_http`
  - small blocking HTTP server for loopback proxying
- `zeroize`
  - secure stack-buffer clearing

Dev dependency:

- `pretty_assertions`
  - clearer diffs in unit tests

### Dependency design assessment

The dependency set fits the crate’s goals well:

- `tiny_http` keeps the local server implementation small and blocking
- `reqwest::blocking` avoids an async runtime and pairs naturally with a thread-per-request model
- `zeroize` and `libc` directly support the secret-handling requirements
- `codex-process-hardening` is a good architectural separation: pre-main hardening remains reusable and independent

The main trade-off is that `reqwest` is a large dependency graph for such a small crate, but it buys mature TLS and header handling.

## Testing

The crate has 10 unit tests across two modules, and `cargo test -p codex-responses-api-proxy` passes.

### `read_api_key.rs` tests

Located in [read_api_key.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/read_api_key.rs#L221-L342).

Covered behaviors:

- read without newline
- read across short/chunked reads
- trim `\r\n`
- error on empty input
- error on oversized input
- propagate I/O errors
- reject invalid UTF-8 and invalid characters

These tests do a good job of covering the pure logic in the most security-sensitive helper.

### `dump.rs` tests

Located in [dump.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L203-L378).

Covered behaviors:

- request dumps contain JSON bodies and redact auth/cookie headers
- response teeing preserves body streaming and writes a redacted response file

These tests validate the core observability path and the most important redaction behavior.

### Testing gaps

Not currently covered:

- end-to-end proxy forwarding through the local HTTP server
- `GET /shutdown` behavior
- `write_server_info` output
- rejection behavior for disallowed routes and methods
- header forwarding edge cases
- streaming behavior against a real upstream server
- Unix `mlock` success or failure behavior
- constructor-based hardening integration

Subprocess or socket-level integration tests would increase confidence in the actual server behavior.

## Design assessment

### Strengths

- Narrow attack surface
  - only one allowed local API route
- Clear privilege boundary
  - the proxy holds the upstream credential, not the caller
- Simple runtime model
  - easy to understand and debug
- Thoughtful secret handling
  - direct read, validation, zeroization, `mlock`, sensitive headers
- Streaming-friendly response path
  - no eager buffering of upstream responses
- Useful diagnostics
  - redacted, correlated request/response dumps

### Trade-offs and limitations

- Thread-per-request model
  - simple, but unbounded under load
- Fully buffered request bodies
  - request streaming is not supported
- Minimal error handling
  - failures are logged to stderr but not surfaced to the client in a structured way
- Hard exit shutdown path
  - `GET /shutdown` calls `std::process::exit(0)` from a worker thread
- Intentional memory leak
  - acceptable here, but still a deliberate lifetime trade-off
- Local-only policy
  - binding is fixed to `127.0.0.1`; broader deployment modes are out of scope

## Notable implementation details

### Host-header normalization

`run_main` derives the `Host` header from the configured upstream URL in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L76-L88). This is important because the proxy permits alternate upstreams such as Azure endpoints while still forcing a consistent host header for the final request.

### Disabled reqwest timeout

The `reqwest` client is created with `.timeout(None::<Duration>)` in [lib.rs](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/lib.rs#L102-L108). That is a meaningful design choice because Responses API streams may stay open for a long time, and reqwest’s default timeout would otherwise terminate them.

### Response dump finalization on `Drop`

[ResponseBodyDump](file:///Users/bytedance/project/codex/codex-rs/responses-api-proxy/src/dump.rs#L130-L134) writes the response dump in `Drop` if EOF was not observed. That improves the chance of capturing partial responses even if the consumer stops early.

## Open questions

1. Should there be integration tests with a fake upstream server?
   - Most important runtime behavior lives at the socket boundary, not in isolated helpers.
2. Should request bodies also be streamed instead of fully buffered?
   - Current behavior is simpler, but it limits scalability and changes latency characteristics.
3. Should worker concurrency be bounded?
   - One thread per request is fine for low local traffic, but it has no backpressure.
4. Should dump redaction expand beyond auth/cookie headers?
   - Request or response JSON bodies may still contain secrets or PII.
5. Should `GET /shutdown` be authenticated or scoped more tightly?
   - The README explicitly accepts this risk for multi-user hosts, but it is still a sharp edge.
6. Should disallowed requests return richer error bodies instead of an empty `403`?
   - Current behavior is strict and simple, but less diagnosable.
7. Should upstream errors be mapped into HTTP responses when possible?
   - Right now a forwarding failure is logged, but the downstream client may only observe connection failure semantics.
8. Should the API-key validation remain restricted to `[A-Za-z0-9_-]+`?
   - It fits current token formats, but it may reject future provider-specific token shapes.

## Suggested improvements

- Add end-to-end tests that stand up the proxy and a dummy upstream server.
- Add focused tests for `forward_request` rejection and header-copying behavior.
- Consider a bounded worker pool or connection cap if local load can be non-trivial.
- Consider optional structured error responses for upstream failures.
- Document the intentional request-body buffering trade-off more explicitly.
- Consider optional body redaction hooks for dump files if dumps are used in less trusted environments.

## Bottom line

`codex-responses-api-proxy` is a focused, well-scoped crate that does one job well: safely mediate local access to an upstream Responses API using a privileged bearer token. Its strongest design qualities are the strict route policy, the careful secret-handling path, and the streaming-friendly response bridge. The main gaps are test depth at the integration boundary, limited backpressure/concurrency controls, and a deliberately minimal operational error model.
