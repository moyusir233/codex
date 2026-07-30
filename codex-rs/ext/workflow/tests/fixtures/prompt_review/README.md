# Prompt-review fixtures

`live_recipe.sh` is an operator recipe, not a CI test. It refuses to run unless
the operator supplies every disposable target and sets the final confirmation
gate. It does not authenticate, install CLIs, expand scopes, create a workspace,
or delete any resulting resource.

The deterministic Rust E2E uses an in-memory fake capability at the same
`PromptReviewCapability` boundary. That keeps model, Fornax, and Lark output
stable while exercising the real registry, reducer, durable checkpoint, wait,
restart, completion, and output schemas.
