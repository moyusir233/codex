# Sanitized `lark-cli 1.0.0` fixtures

These synthetic fixtures cover only fields consumed by the current typed
adapter. They prove local decoding and fail-closed behavior for revision-bearing
document fetch, verified and paginated document-create reconciliation,
paginated identity ambiguity, exact owned-chat reconciliation, complete member
pagination, a partial member-add response, authoritative post-add
reconciliation, and the single-writer document-update lifecycle. Update
fixtures cover a verified new revision, timeout-after-apply reconciliation,
fetch-before-retry, pre-write drift, post-write mismatch, and post-apply drift.
They are not authenticated tenant captures and do not prove multi-writer
compare-and-swap or tenant scopes.

The 2026-08-06 host binary reports `lark-cli version 1.0.0`. Its read-only help
exposes document fetch/create/update, chat search/create, raw paginated chat
member get/create, contact search, and idempotent message send. Live mode must
remain disabled until each supported operation has a sanitized disposable-
tenant golden response and failure fixture. Never replace these files with raw
production output or content containing private identifiers.

The exact binary also exposes generic `lark-cli api` requests. The official
docx block update accepts a historical `document_revision_id` and idempotent
`client_token`, but does not require the revision to equal the current latest
revision. `notes/lark-native-document-concurrency.md` records why that surface
must not be represented as multi-writer compare-and-swap. The supported model
instead relies on the approved invariant that workflow-owned documents have no
other writer and still fails closed if observed revision/content drift violates
that invariant.
