# Codex workflow extension

This crate owns the experimental typed workflow API, durable reducer runtime,
external capability adapters, and reference workflows. External mutations must
be planned in `codex-state` before dispatch and reconciled rather than retried
when their outcome is ambiguous.

## Fornax

The Fornax integration has two separately versioned boundaries:

- `fornax-cli v0.0.51` provides typed prompt reads/full-draft saves, skill
  reads/staging, and trace reads through a controlled process runner.
- The separately installed `codex-fornax-trace-bridge 0.1.0` wheel provides
  authenticated loopback trace writes through pinned `bytedance.fornax
  1.0.46`. Codex never installs the private wheel or SDK.

Rust invokes only the lifecycle CLI with structured arguments. Trace
start/record/finish/status calls use the protocol-v1 typed HTTP client, with
proxies and redirects disabled and an exact `127.0.0.1` endpoint. Credentials,
opaque SDK header maps, and input/output content are excluded from normal
responses and workflow state. Workflow persistence keeps operation, span
handle, trace context, trace, span, and bridge-instance IDs.

Fornax trace writing remains experimental. Preflight requires the exact bridge,
protocol, and SDK versions, authenticated health, secure descriptor files, and
an explicit live-delivery approval. That approval remains false until a
finished/flushed disposable trace is retrieved by ID with `fornax-cli`.
Prompt/skill/trace-read support is unaffected when the companion is absent.
