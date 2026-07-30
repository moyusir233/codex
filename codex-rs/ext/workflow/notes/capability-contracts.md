# Workflow external capability contracts

This note records the read-only Milestone 0 probes. It is capability evidence,
not an authenticated service contract. No external resource was created or
modified.

## Probe environment

- Date: 2026-07-30 PRC.
- Working directory: `/data00/home/yangchengrun/harness-engine/codex`.
- Commands were invoked through the required `rtk` proxy.
- Help output was read without passing credentials or selecting a workspace.

## Fornax CLI

`fornax-cli version` reported `fornax-cli v0.0.51`.

`fornax-cli trace --help` listed only `get` and `list`. It advertised structured
`raw` and `json` output, `--timeout`, three time-window forms, and `-o/--output`.
It did not list a trace/span mutation command. `fornax-cli span --help` listed
only `list`. `fornax-cli trajectory --help` required exactly one of trace ID,
experiment ID, or log ID.

Observed process behavior: the binary emitted unrelated metrics warnings on
stderr even for version/help. Adapters must therefore parse stdout and retain
bounded, redacted stderr only as diagnostics.

Capability conclusion:

- Prompt/skill/trace-read support still requires command-specific fixtures.
- Trace reads are present.
- CLI trace writes are absent and must never be synthesized.
- Authenticated JSON envelopes remain unverified.

## Lark CLI

`lark-cli --version` reported `lark-cli version 1.0.0`.

`lark-cli im +messages-send --help` advertised bot-only chat/user targeting,
mutually exclusive typed bodies, `--dry-run`, and `--idempotency-key`.

`lark-cli event +subscribe --help` advertised WebSocket NDJSON, a single-instance
lock, and marked `--force` unsafe because multiple connections split events.

Capability conclusion:

- Message idempotency is explicitly supported.
- A workflow host must own one shared raw NDJSON subscriber and must not use
  `--force`.
- Authenticated tenant response envelopes, scopes, and replay behavior remain
  unverified.

## Fake executable harness

`tests/feasibility.rs` exercises structured argv, controlled cwd, cleared
environment, JSON stdout, timeout termination, and cooperative cancellation
without authenticating to either service. The harness is intentionally
experimental; bounded streaming output and production redaction belong to
Milestone 8A.
