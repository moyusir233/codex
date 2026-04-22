# codex-network-proxy: Code Analysis

## Overview

`codex-network-proxy` is a Rust library crate that implements Codex's local network enforcement proxy. It combines:

- HTTP proxying, including plain HTTP forwarding and HTTPS `CONNECT` tunneling
- SOCKS5 TCP and optional SOCKS5 UDP proxying
- Policy enforcement over destination hosts, local/private network access, and limited-vs-full network mode
- Optional managed MITM support for HTTPS so limited-mode method restrictions can still be enforced after `CONNECT`
- Runtime reloadable policy state, blocked-request telemetry, and OTEL-style audit events
- Environment-variable injection helpers so child processes are forced through the managed proxy

At a high level, the crate is both:

- an embeddable library that exposes `NetworkProxy`, `NetworkProxyState`, `NetworkPolicyDecider`, and config types
- a policy engine that sits between clients and the network and decides whether requests are allowed, denied, or escalated to an external decider

## Top-Level Responsibilities

### 1. Parse and normalize network policy configuration

The crate models proxy configuration in `NetworkProxyConfig` / `NetworkProxySettings`, including:

- enable/disable flag
- HTTP and SOCKS bind addresses
- allowlist and denylist domain patterns
- unix socket allowlist
- local/private network access policy
- upstream proxy behavior
- limited vs full network mode
- optional MITM enablement

Notable behavior:

- domain permission precedence is encoded in `NetworkDomainPermission` ordering: `None < Allow < Deny`
- duplicate domain entries collapse to the strongest effective permission
- non-loopback bind addresses are clamped back to loopback unless explicitly allowed
- if unix socket proxying is enabled, bind addresses are forced to loopback even when non-loopback was requested

## 2. Compile policy state for fast runtime checks

`build_config_state` transforms config into a runtime-ready `ConfigState`:

- compiles allowlist and denylist globsets
- validates disallowed global wildcards for deny rules and managed constraints
- optionally constructs `MitmState`
- initializes blocked-request buffers and managed constraints

This means most request-time checks operate over precompiled state rather than reparsing config.

## 3. Enforce host and mode policy at request time

Policy enforcement happens in layers:

1. `NetworkProxyState::host_blocked` checks denylist, allowlist, local/private network restrictions, and DNS-based SSRF safeguards.
2. `evaluate_host_policy` optionally consults a `NetworkPolicyDecider`, but only for `not_allowed` allowlist misses.
3. Protocol-specific handlers in HTTP/SOCKS/MITM apply additional rules like method restrictions or proxy-disabled behavior.

Important rule ordering:

1. explicit deny wins
2. local/private network restrictions apply before general allowlist approval
3. empty allowlist means "deny by default"
4. decider override can only rescue `not_allowed`, not explicit deny or local/private blocks

## 4. Run proxy listeners and manage process-facing integration

`NetworkProxyBuilder` and `NetworkProxy` handle:

- constructing a proxy from shared state
- reserving managed listeners on loopback or platform-specific managed ports
- starting HTTP and SOCKS services as Tokio tasks
- applying proxy environment variables to subprocess env maps
- replacing a subset of runtime config safely without restarting listeners

This makes the crate usable as a managed component inside a larger Codex runtime.

## Module Map

### Public surface / crate root

- `src/lib.rs`: re-exports config, state, proxy builder/handle, policy decider, and runtime telemetry types

### Configuration and derived runtime state

- `src/config.rs`: user-facing config model, address resolution, default values, bind clamping, unix socket path validation
- `src/policy.rs`: host normalization, glob compilation, wildcard semantics, IP classification helpers
- `src/state.rs`: managed constraints, config-state construction, constraint validation
- `src/runtime.rs`: live `NetworkProxyState`, reload hooks, host checks, blocked-request recording, runtime updates

### Request policy and audit layer

- `src/network_policy.rs`: protocol-neutral policy request/decision types, decider trait, audit event emission, host policy evaluation
- `src/reasons.rs`: canonical machine-readable block reasons
- `src/responses.rs`: reusable blocked/plain/json response helpers

### Transport handlers

- `src/http_proxy.rs`: HTTP proxy server, HTTP request filtering, `CONNECT` acceptance, tunnel forwarding, unix socket proxying
- `src/socks5.rs`: SOCKS5 TCP connector and UDP inspector with the same policy gatekeeping
- `src/upstream.rs`: outgoing HTTP client construction, including optional upstream HTTP proxy support and unix socket connector
- `src/mitm.rs`: HTTPS interception path used to enforce limited-mode policy after `CONNECT`
- `src/certs.rs`: managed CA generation/loading and per-host certificate issuance for MITM

## Primary Public APIs

### Configuration model

- `NetworkProxyConfig`
- `NetworkProxySettings`
- `NetworkMode`
- `NetworkDomainPermissions`
- `NetworkUnixSocketPermissions`
- `resolve_runtime`
- `host_and_port_from_network_addr`

These types are the main embedding/configuration entry points.

### Runtime state and policy

- `NetworkProxyState`
- `ConfigState`
- `ConfigReloader`
- `BlockedRequest`
- `BlockedRequestObserver`
- `build_config_state`
- `validate_policy_against_constraints`

`NetworkProxyState` is the central shared object used by all protocol handlers.

### Policy extension hooks

- `NetworkPolicyDecider`
- `NetworkPolicyRequest`
- `NetworkDecision`
- `NetworkPolicyDecision`
- `NetworkDecisionSource`

This allows the embedding application to inject a second-stage policy decision, usually to map higher-level approvals to network access.

### Proxy lifecycle

- `NetworkProxy::builder()`
- `NetworkProxyBuilder`
- `NetworkProxy::run()`
- `NetworkProxy::apply_to_env()`
- `NetworkProxy::replace_config_state()`
- `NetworkProxyHandle::{wait, shutdown}`

The crate is designed so callers build shared state first, then construct and run proxy listeners from it.

## Configuration Semantics

### Domain patterns

The crate supports several pattern styles:

- `example.com`: exact host
- `*.example.com`: subdomains only, not the apex
- `**.example.com`: apex plus subdomains
- `*`: accepted only for allowlist compilation, rejected for denylist and many managed-constraint cases

Pattern handling is intentionally normalized:

- lowercase matching
- trailing-dot trimming
- bracket stripping for IPv6 literals
- duplicate pattern deduplication

### Local/private network protection

When `allow_local_binding = false`, the crate blocks:

- loopback literals like `127.0.0.1`, `::1`, `localhost`
- private IPv4 ranges
- link-local and unspecified addresses
- unique-local/link-local IPv6
- hosts that resolve via DNS to non-public IPs

There is an explicit defense-in-depth decision here: even an allowlisted hostname is blocked if DNS resolution indicates a local/private destination. Only explicit local literal allowlisting can permit local/private literal access.

### Unix socket proxying

Unix socket support is intentionally narrow:

- only supported on macOS
- path must be absolute
- path must be allowlisted unless `dangerously_allow_all_unix_sockets = true`
- proxy listeners are forced to loopback when unix socket proxying is enabled

This is treated as a local capability escape hatch, not a general network feature.

## Request Flow

## HTTP plain request flow

For a normal HTTP request handled by `http_plain_proxy`:

1. extract `NetworkProxyState` from request extensions
2. check current mode's method policy
3. if `x-unix-socket` is present, run the unix-socket-specific branch
4. parse target authority using `RequestContext`
5. validate absolute-form request URI against `Host` header to prevent mismatches
6. reject immediately if proxy is disabled
7. evaluate host policy via `evaluate_host_policy`
8. if method is blocked in limited mode, return a blocked response
9. create `UpstreamClient`, optionally using env upstream proxies
10. strip hop-by-hop headers and forward upstream

Error/telemetry behavior:

- structured blocked-response JSON for domain/method/policy blocks
- `x-proxy-error` header for clients
- blocked requests recorded in memory and optionally sent to observers
- audit events emitted for allow/deny decisions

## HTTPS CONNECT flow

`http_connect_accept` and `http_connect_proxy` split CONNECT handling into an accept phase and an upgraded tunnel phase.

### Accept phase

1. parse authority from request
2. normalize host
3. verify proxy is enabled
4. evaluate host policy
5. read current network mode
6. read optional MITM state
7. if mode is `Limited` and MITM is unavailable, reject with `mitm_required`
8. store `ProxyTarget`, `NetworkMode`, and optional `MitmState` into request extensions
9. return `200 OK` to establish the tunnel

### Tunnel phase

- if limited mode and MITM state exists, terminate TLS locally and inspect inner requests
- otherwise, establish a TCP/TLS tunnel to the destination, optionally through an upstream HTTP proxy discovered from env

This split is a clean design choice: policy is decided before upgrade, while actual transport behavior is deferred until after the client receives tunnel acceptance.

## MITM flow

MITM exists specifically to make limited-mode HTTPS enforceable.

Flow:

1. `MitmState::new` loads or creates a local CA and chooses direct vs proxied upstream behavior
2. `mitm_tunnel` reads `ProxyTarget`, target host/port, mode, and shared state from the upgraded tunnel
3. it generates a per-host leaf certificate from the managed CA
4. it terminates client TLS with `TlsAcceptorLayer`
5. inner requests are routed through `handle_mitm_request` / `forward_request`
6. `mitm_blocking_response` re-checks host safety and method policy before forwarding
7. allowed requests are rewritten to `https://authority/path` and forwarded via `UpstreamClient`

Key security properties:

- host mismatch inside the intercepted request is rejected
- local/private host checks are repeated after CONNECT to reduce DNS rebinding risk
- limited-mode method checks are applied to inner HTTPS methods

Current implementation note:

- body inspection plumbing exists, but `MITM_INSPECT_BODIES` is currently `false`
- the inspection path logs body lengths and truncation status, not full bodies

## SOCKS5 flow

The SOCKS5 server is implemented in `socks5.rs` using Rama's acceptor stack.

### TCP flow

1. extract state and target host/port from `TcpRequest`
2. reject if proxy disabled
3. reject if mode is `Limited`
4. evaluate host policy using `evaluate_host_policy`
5. on allow, delegate to `TcpConnector`

### UDP flow

1. inspect relay destination IP/port before relaying
2. reject if proxy disabled
3. reject if mode is `Limited`
4. evaluate host policy
5. on allow, return relay payload unchanged

This crate treats SOCKS5 as full-access only. Unlike HTTP, limited mode never tries to partially restrict SOCKS semantics.

## State, Reloading, and Constraints

`NetworkProxyState` is a live, reloadable wrapper around `ConfigState`.

Responsibilities:

- lazy reload via `ConfigReloader::maybe_reload`
- forced reload via `reload_now`
- persistent blocked-request buffer across reloads
- mutation helpers such as `add_allowed_domain`, `add_denied_domain`, `set_network_mode`
- unix socket permission checks
- blocked telemetry buffering and observer notification

Managed constraints are enforced via `NetworkProxyConstraints` and `validate_policy_against_constraints`.

This allows an embedding system to lock down:

- whether the proxy may be enabled
- max-permitted network mode
- upstream proxy usage
- local binding
- allowlist/denylist contents or expansion behavior
- unix socket access

The mutation methods are carefully written as compare-and-retry loops:

- snapshot current config and constraints
- build a candidate config
- validate against constraints
- compile a new `ConfigState`
- acquire write lock
- retry if state changed concurrently

That is a solid design for low-frequency config mutation under async concurrency.

## Environment Management

`NetworkProxy::apply_to_env` forcibly rewrites proxy-related environment variables so child processes route through the managed proxy.

Behavior includes:

- setting `HTTP_PROXY` / `HTTPS_PROXY` / websocket proxy vars to the HTTP proxy
- setting `ALL_PROXY` to SOCKS when SOCKS is enabled, otherwise to HTTP
- setting `NO_PROXY` to common loopback and RFC1918/private ranges
- setting `CODEX_NETWORK_PROXY_ACTIVE`
- setting `CODEX_NETWORK_ALLOW_LOCAL_BINDING`
- on macOS, injecting or refreshing a `GIT_SSH_COMMAND` SOCKS wrapper unless an unrelated user-provided wrapper already exists

This is operationally important because it prevents command-specific env overrides from bypassing the managed proxy endpoint.

## Dependency Analysis

## Core runtime and async

- `tokio`: task spawning, async runtime, locks, TCP utilities, DNS timeout handling
- `async-trait`: async trait objects for `ConfigReloader`, `BlockedRequestObserver`, and `NetworkPolicyDecider`
- `tracing`: audit/event logging and debug/warn/info logs

## Configuration and serialization

- `serde`, `serde_json`: config model and blocked-response/event serialization
- `url`: address parsing and host normalization support
- `globset`: compiled host-pattern matching
- `thiserror`, `anyhow`: error modeling and error context

## Proxying / transport stack

- `rama-core`, `rama-http`, `rama-http-backend`, `rama-net`, `rama-socks5`, `rama-tcp`: main proxy server/client stack
- `rama-tls-rustls`: TLS accept/connect layers for CONNECT and MITM
- `rama-unix` (unix family only): unix socket connector

The crate is deeply coupled to Rama for server/client plumbing. That is the most important external architectural dependency.

## Internal workspace utilities

- `codex-utils-absolute-path`: validated absolute paths for unix socket allowlists
- `codex-utils-home-dir`: resolve Codex home directory for managed MITM CA storage
- `codex-utils-rustls-provider`: ensure Rustls crypto provider is initialized

## Time / audit support

- `chrono`, `time`: RFC3339 audit timestamps and unix timestamps

## Security Design Notes

### Strong choices

- deny-first evaluation order
- default-deny when allowlist is empty
- local/private host protection with DNS resolution fallback
- explicit refusal to allow global wildcard deny patterns
- loopback-only enforcement when unix socket proxying is enabled
- secure CA key creation with restrictive mode checks and anti-overwrite behavior
- CONNECT split so policy decision happens before tunnel creation
- decider override limited to allowlist misses only

### Tradeoffs / caveats

- DNS rebinding is only partially addressed because the final transport path does not pin the resolved IP from the policy check
- MITM support generates a local CA but trust installation/distribution is outside this crate
- blocked response messages are intentionally coarse and do not expose much policy detail
- unix socket support is intentionally macOS-specific, which simplifies threat modeling but narrows portability

## Testing Coverage

The crate has strong unit/integration-style coverage concentrated inside module-local tests.

Major areas covered:

- config defaults, host/port parsing, bind clamping, unix socket path validation
- host normalization and domain glob semantics
- managed constraint validation and dynamic allow/deny mutations
- blocked-request buffer behavior
- audit event emission and metadata fields
- HTTP CONNECT behavior, absolute-form `Host` validation, hop-by-hop header stripping
- unix socket request gating
- SOCKS5 block-path auditing
- MITM policy behavior, including host mismatch and local/private re-checks
- CA file permission and symlink checks

Local verification result during analysis:

- `cargo test -p codex-network-proxy` passed
- 133 tests passed, 0 failed

## Design Assessment

### What is well designed

- Clear layering: config -> compiled state -> policy engine -> protocol handlers
- Shared policy logic: `evaluate_host_policy` centralizes domain-policy decisions across HTTP, CONNECT, and SOCKS
- Good separation of transport vs policy concerns
- Defensive normalization and validation around hosts, wildcards, and socket paths
- Thoughtful operational hooks: reloaders, observers, env injection, managed constraints
- Security-oriented defaults across binding, local/private access, and unix socket routing

### Areas that feel intentionally conservative

- SOCKS5 is all-or-nothing by mode
- unix socket support is macOS-only
- MITM body inspection exists but is effectively disabled
- policy response detail exposed to clients is minimal

These choices look deliberate rather than unfinished, except where noted below.

## Open Questions

1. Why are `chrono` and `time` both needed long-term?
   - `network_policy.rs` uses `chrono` for RFC3339 OTEL timestamps.
   - `runtime.rs` uses `time` for unix timestamps.
   - This may be acceptable, but it adds duplicate date/time dependencies for a small benefit.

2. Should hostname-to-IP resolution be pinned after policy approval?
   - The crate performs DNS-based SSRF checks, and MITM re-checks local/private resolution after CONNECT.
   - But the final upstream connector still resolves independently, which leaves a residual rebinding window.

3. Should blocked client responses expose richer structured policy data?
   - `PolicyDecisionDetails` carries decision/source/protocol/host/port.
   - `blocked_message_with_policy` currently ignores most of it and returns only a generic human message.
   - The JSON blocked response exposes some detail, but plain-text paths remain intentionally sparse.

4. Should MITM leaf cert issuance be cached?
   - `tls_acceptor_data_for_host` generates a new leaf cert on demand.
   - That keeps the implementation simple, but repeated CONNECTs to the same host may pay repeated certificate-generation cost.

5. Should unix socket support remain hardcoded to macOS?
   - The current threat model is explicit and conservative.
   - If Linux support becomes necessary, policy/path handling likely already covers much of the logic, but connector/runtime behavior would need careful review.

6. Should limited mode support a constrained subset of SOCKS5 rather than a blanket block?
   - Current behavior is simple and safe.
   - If future use cases need DNS or TCP-only SOCKS in limited mode, the current architecture would need additional method-equivalent policy semantics.

7. Is the README example still aligned with the actual `NetworkDecision` API?
   - The public enum now models deny decisions as:
     - `NetworkDecision::Deny { reason, source, decision }`
   - The example in the README appears to show only `reason`.
   - That may be documentation drift rather than implementation ambiguity.

## Suggested Mental Model

The crate is easiest to understand as three stacked systems:

1. configuration compiler
   - parses user/managed config and precompiles matching state

2. policy kernel
   - decides whether a destination is allowed, denied, or decider-overridable

3. protocol adapters
   - HTTP, CONNECT, SOCKS5, unix sockets, and MITM each translate transport-specific events into the same policy kernel and telemetry model

That architecture is the main strength of the crate: transport handlers differ, but policy meaning stays centralized.
