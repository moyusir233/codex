# Codex Fornax trace bridge

This optional, experimental wheel provides the authenticated loopback protocol used by
Codex workflows to report custom spans through the pinned Fornax Python SDK. It is
distributed separately from Codex and is never installed automatically.

The daemon is Unix-only. It reads `FORNAX_AK`, `FORNAX_SK`, and optional
`FORNAX_CUSTOM_REGION` at startup. Credentials never appear in command arguments or
normal JSON output.

```console
uv run codex-fornax-trace version --format json
uv run codex-fornax-trace daemon ensure --format json
uv run codex-fornax-trace daemon status --format json
uv run codex-fornax-trace daemon stop --grace-seconds 30 --format json
```

Protocol v1 mutations require a UUID `operationId`, the same value in
`Idempotency-Key`, the descriptor credential, and the exact protocol header. An
ambiguous mutation is never retried blindly. A daemon restart orphans live Python span
handles while preserving their opaque header context for recovery children.
