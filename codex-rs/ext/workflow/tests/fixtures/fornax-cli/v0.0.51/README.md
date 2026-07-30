# Sanitized `fornax-cli v0.0.51` fixtures

These fixtures contain synthetic IDs and content. They model only the JSON
fields consumed by the typed adapters and are safe to commit.

Once a disposable authenticated workspace is provisioned, capture each
operation with `--format json`, remove credentials, personal data, prompt
content, tenant/workspace IDs, log IDs, and provider diagnostics, then compare
the reviewed envelope against these parsers. Never replace these fixtures with
raw production output.
