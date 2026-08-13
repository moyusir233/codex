# Lark native document revision semantics

Status: read-only official-contract review, 2026-08-06. No authenticated API
request or external mutation was performed.

## Superseding product consistency decision

The product owner subsequently approved a single-writer invariant for
workflow-owned documents: the current workflow agent is the only writer, and
external users or applications do not edit them. Atomic server-side CAS is
therefore outside the required consistency model.

The implementation does not reinterpret the native revision selector as CAS.
It journals the expected revision and content digest before dispatch, fetches
the latest snapshot, performs only a deterministic full-content overwrite when
that snapshot matches, then fetches again and requires the desired digest at a
new revision. A timeout is reconciled by fetch before retry; the overwrite is
content-idempotent. Any unexpected revision/content drift or post-write
mismatch returns `NeedsOperator`. The official multi-writer limitations below
remain accurate background evidence.

## Exact local CLI surface

The installed binary is `lark-cli 1.0.0`. Its high-level
`docs +update --help` accepts document, mode, content/selection and title
arguments, but has no expected-revision parameter. Its generic
`lark-cli api METHOD PATH` command can represent arbitrary query parameters and
supports `--dry-run`.

A sanitized dry-run proved that this exact binary can construct:

```text
PATCH /open-apis/docx/v1/documents/:document_id/blocks/batch_update
params: document_revision_id, client_token, user_id_type
body: requests[]
identity: bot or user
```

The dry-run did not contact Lark and is command-shape evidence only.

## Official OpenAPI contract

The official [batch block update
specification](https://open.feishu.cn/document/server-docs/docs/docs/docx-v1/document-block/batch_update.md)
documents `PATCH
/open-apis/docx/v1/documents/:document_id/blocks/batch_update`. It requires one
of `docx:document` or `docx:document:write_only`, accepts
`document_revision_id`, `client_token` and `user_id_type`, and returns the
post-update document revision. `client_token` supplies operation idempotency.

The official [document data
contract](https://open.feishu.cn/document/docs/docs/data-structure/document.md)
defines a valid revision selector as `-1` or any positive revision not newer
than the document's latest revision. It also notes that comments can advance
the revision without changing document content. The update API's `1770021`
error is documented only for a revision more than 1,000 versions behind the
latest revision. Therefore the contract permits recent stale revisions; it
does not say that a supplied revision must equal the current revision.

The relevant fetched public Markdown had these SHA-256 values at review time:

- cloud-document API index:
  `5d70d2cd46cf3bb2508da864897d54bfa65b45af42db564b92877621dd0d0671`;
- batch-update specification:
  `a02945d3c6945073aa7e28132eeeaa52b539e15faf5fd6833190dac74e8c3080`;
- document data contract:
  `2aee8bf513bab63fc77c3be37b24c8f89ab9416b489896371e15a49170721a80`;
- get-document specification:
  `36811233c3a0f058b49b45c13c63918f8db88702ce68d1559360d49d644f4f9b`.

## Compatibility decision

Revision selection plus an idempotency token is useful for stable block
addressing and retry reconciliation, but it is not compare-and-swap. A
fetch-then-update sequence can race, and a recent stale update may be accepted
after another user edits or comments on the document. Post-write revision/body
verification detects some drift only after mutation and cannot undo a blind
overwrite.

Consequently the native raw API is not exposed as a multi-writer CAS primitive.
The supported capability is instead
`lark.documents.single-writer-revision-aware.v1`. Live Lark feature composition
remains absent until the separately documented tenant, identity, retention,
Fornax and canary authorization gates are satisfied.

## `lark-cli 1.0.72` source review

The planning host had exposed `lark-cli 1.0.72` help with a `--revision-id`
flag, so the official tag `v1.0.72` (`4a56748bfa941ff0ee0bfec92e65acac427732b0`)
was inspected read-only. `shortcuts/doc/docs_update_v2.go` sends a `PUT` to
`/open-apis/docs_ai/v1/documents/:document_id` and copies `--revision-id`
directly into the request body as `revision_id`. It performs no fetch/current
comparison and no client-side conflict check. Its unit tests and bundled
`lark-doc-update.md` describe the value only as a base revision; neither names
a stale-revision rejection or CAS guarantee. Pull request 638, which introduced
this path, likewise claims structured update operations but no concurrency
precondition.

The inspected official files had these SHA-256 values:

- `shortcuts/doc/docs_update_v2.go`:
  `2d088ee3f8f46bd5ba4e2552b3fa2e24ee0a724a832acc6539a92cea4608a862`;
- `shortcuts/doc/docs_update_test.go`:
  `9836e4cbeb7d4ce7a8f086ce6ba2053c8b7faad262bc16a45bc5426499c4d4bb`;
- `skills/lark-doc/references/lark-doc-update.md`:
  `45f34d5ba6df6edebf830d8416a22d7ee3944b58342e4f9f3d12d2bcb46d6615`;
- public pull request 638 metadata:
  `0198bf43fb48d396a0a02eccd2c6812e06c1a1aeb4e4147d12200fe3e5d7a9b2`.

Because `docs_ai` server-side stale-write behavior is not documented in those
sources and no disposable authenticated conflict fixture is authorized, the
newer flag cannot be promoted to CAS by inference. Version 1.0.72 is also not
the installed or pinned executable in this checkout.

A future profile qualifies as CAS only with reviewed authoritative evidence
that the mutation is rejected before changing content whenever the supplied
expected revision is not the current revision. Its fixtures must cover exact
success, immediate stale conflict with unchanged remote body, timeout after
dispatch reconciled by client token/body digest, malformed response, and a
concurrent comment/edit. Merely adding a `--revision-id` flag is insufficient.
