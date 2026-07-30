# Workflow external capability contracts

This note records the Milestone 0 probes and the Milestone 8A typed CLI
contract. It is capability/help evidence plus sanitized fake-backed parser
coverage, not an authenticated disposable-workspace service contract. No
external resource was created or modified.

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

- The production adapter accepts exactly `fornax-cli v0.0.51`.
- Prompt get-by-key/get-by-id and full draft save through `--draft-file` are
  implemented. Draft save uses a mode-0600 temporary file, validates the
  complete replacement object, and remains a journal-before-dispatch mutation.
- Local rendering supports only verified `normal` string messages and
  `{{variable}}` substitution. Multipart, placeholder, snippet, and unknown
  template kinds fail closed; no server render/resolve command is claimed.
- Skill get/batch-get-by-key are implemented. Install always passes an explicit
  absolute `--dir` staging path and never invokes the interactive implicit-home
  behavior.
- Trace get/list, span list pagination, and trajectory reads use typed,
  paired time windows. Span pagination callers retain a fixed absolute window.
- CLI trace writes are absent and must never be synthesized.
- The structured process runner uses an absolute executable, controlled
  absolute cwd, cleared/allow-listed environment, null stdin, bounded
  stdout/stderr, timeout and cooperative cancellation termination, and
  redacted diagnostics.
- Synthetic sanitized fixtures under
  `tests/fixtures/fornax-cli/v0.0.51/` cover only fields consumed by the
  adapters. Authenticated tenant envelopes remain unverified until disposable
  resource IDs are supplied.

Milestone 8A help was rechecked on 2026-07-31 with:

```text
fornax-cli prompt get-by-key --help
fornax-cli prompt get-by-id --help
fornax-cli prompt draft save --help
fornax-cli skill get --help
fornax-cli skill batch-get-by-key --help
fornax-cli skill install --help
fornax-cli trace get --help
fornax-cli trace list --help
fornax-cli span list --help
fornax-cli trajectory --help
```

The installed binary matched the recorded flags. The session had
authentication, but no live prompt, skill, draft, trace, or trajectory command
was executed because no disposable resource IDs or mutation target were
authorized.

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
