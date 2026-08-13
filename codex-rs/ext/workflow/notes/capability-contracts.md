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

### 2026-08-06 implementation reconfirmation

- Harness source: `fa43925012e611e649268f00ca412556ff2ab21f`.
- Codex source: `d1c082e008aa13486672171c2286ca9559b44f47`.
- The repository-level include `/Users/bytedance/.codex/RTK.md` is not present in
  this Linux checkout. The installed `/usr/local/bin/rtk` proxy was used for
  validation commands, and `codex/AGENTS.md` was read in full.
- `lark-cli --version` reported `lark-cli version 1.0.0` and
  `fornax-cli --version` reported `fornax-cli v0.0.51`.
- Read-only help confirmed that the installed Lark binary exposes document
  fetch/create/update, chat search/create, raw paginated `im chat.members`
  get/create, `contact +search-user`, and idempotent message send. The public
  version string alone is not enough to claim authenticated response or
  revision/CAS compatibility.
- No Lark or Fornax authenticated command was run. Live capability remains
  disabled until disposable, sanitized golden envelopes and delivery evidence
  are reviewed.
- The exact Lark profile now has fixture-backed, typed identity search; complete
  owned-chat search; full chat-member pagination/addition/reconciliation; and
  immutable document-revision fetch. Chat and document-create reconciliation
  inspect every page, require one exact ownership marker/body digest, and reject
  duplicates, continuation-token loops, missing tokens and multiple matches.
  Document creation is not accepted until a fetch verifies its ID, URL, exact
  body and nonempty revision. Member addition uses `succeed_type=2`, rejects
  invalid, missing, or pending IDs, and reconciles an ambiguous result from the
  complete authoritative member set. Workflow-owned documents use the approved
  single-writer consistency model: journal expected revision/content digest,
  fetch before dispatch, issue a deterministic full-content overwrite, then
  fetch and verify a new revision with the desired digest. Ambiguous timeouts
  are re-fetched before an idempotent retry. Unexpected pre-write drift,
  post-write mismatch, post-apply drift, or exhausted observation retries yield
  `NeedsOperator`; no unexpected content is silently overwritten.
- The generic `lark-cli api` surface can construct the official docx block
  update with `document_revision_id` and idempotent `client_token`. Official
  semantics allow recent historical revisions, so this is revision-aware but
  not expected-current-revision CAS. Exact evidence and qualification criteria
  are recorded in `notes/lark-native-document-concurrency.md`.
- Atomic multi-writer CAS is deliberately not claimed. The product contract
  requires one writer—the current workflow agent—and excludes external edits by
  users or applications. `LarkWorkflowCompatibility::Supported` therefore
  means this revision-aware single-writer protocol is fixture-backed, not that
  the server rejects concurrent writers.
- Message idempotency keys are capped at 50 characters. Workflow-generated
  keys use the `wf-` prefix plus 47 hexadecimal characters.
- Baseline evidence used integration-test binaries explicitly. The plan's bare
  name filters selected zero tests under nextest; `--test <binary>` is required
  for the contract binaries in this checkout.

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
