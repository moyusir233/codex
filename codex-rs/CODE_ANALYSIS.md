# Workspace Code Analysis Index

This document is the workspace-level navigation index for all crate analysis documents generated across this Rust workspace.

- Workspace root: `/Users/bytedance/project/codex/codex-rs`
- Total workspace members indexed: `93`
- Per-crate document name: `CODE_ANALYSIS.md`
- Source of truth for member discovery: `Cargo.toml` workspace `members`

## Quick Retrieval

Use these patterns when you want to jump to the right document quickly:

- By crate name: search this file for the workspace member path, for example `app-server`, `utils/template`, or `codex-api`.
- By document path: every entry links directly to the crate-local `CODE_ANALYSIS.md`.
- By topic across all analyses: run one of the commands below from the workspace root.

```bash
# Search all analysis docs for a concept
rg -n "MCP|WebSocket|SQLite|sandbox|approval" -- */CODE_ANALYSIS.md utils/*/CODE_ANALYSIS.md

# List all headings across analysis docs
rg -n "^## " -- */CODE_ANALYSIS.md utils/*/CODE_ANALYSIS.md

# Locate a specific crate analysis file
fd CODE_ANALYSIS.md .
```

## Quick Jump

- [App Server Family](#app-server-family)
- [Cloud Family](#cloud-family)
- [Core Family](#core-family)
- [Exec Family](#exec-family)
- [Utils Family](#utils-family)
- [Other `codex-*` Crates](#other-codex--crates)
- [Remaining Top-Level Crates](#remaining-top-level-crates)
- [Full Alphabetical Index](#full-alphabetical-index)

## App Server Family

- `app-server`: [`./app-server/CODE_ANALYSIS.md`](./app-server/CODE_ANALYSIS.md)
- `app-server-client`: [`./app-server-client/CODE_ANALYSIS.md`](./app-server-client/CODE_ANALYSIS.md)
- `app-server-protocol`: [`./app-server-protocol/CODE_ANALYSIS.md`](./app-server-protocol/CODE_ANALYSIS.md)
- `app-server-test-client`: [`./app-server-test-client/CODE_ANALYSIS.md`](./app-server-test-client/CODE_ANALYSIS.md)

## Cloud Family

- `cloud-requirements`: [`./cloud-requirements/CODE_ANALYSIS.md`](./cloud-requirements/CODE_ANALYSIS.md)
- `cloud-tasks`: [`./cloud-tasks/CODE_ANALYSIS.md`](./cloud-tasks/CODE_ANALYSIS.md)
- `cloud-tasks-client`: [`./cloud-tasks-client/CODE_ANALYSIS.md`](./cloud-tasks-client/CODE_ANALYSIS.md)
- `cloud-tasks-mock-client`: [`./cloud-tasks-mock-client/CODE_ANALYSIS.md`](./cloud-tasks-mock-client/CODE_ANALYSIS.md)

## Core Family

- `core`: [`./core/CODE_ANALYSIS.md`](./core/CODE_ANALYSIS.md)
- `core-plugins`: [`./core-plugins/CODE_ANALYSIS.md`](./core-plugins/CODE_ANALYSIS.md)
- `core-skills`: [`./core-skills/CODE_ANALYSIS.md`](./core-skills/CODE_ANALYSIS.md)

## Exec Family

- `exec`: [`./exec/CODE_ANALYSIS.md`](./exec/CODE_ANALYSIS.md)
- `exec-server`: [`./exec-server/CODE_ANALYSIS.md`](./exec-server/CODE_ANALYSIS.md)
- `execpolicy`: [`./execpolicy/CODE_ANALYSIS.md`](./execpolicy/CODE_ANALYSIS.md)
- `execpolicy-legacy`: [`./execpolicy-legacy/CODE_ANALYSIS.md`](./execpolicy-legacy/CODE_ANALYSIS.md)

## Utils Family

- `utils/absolute-path`: [`./utils/absolute-path/CODE_ANALYSIS.md`](./utils/absolute-path/CODE_ANALYSIS.md)
- `utils/cargo-bin`: [`./utils/cargo-bin/CODE_ANALYSIS.md`](./utils/cargo-bin/CODE_ANALYSIS.md)
- `utils/cache`: [`./utils/cache/CODE_ANALYSIS.md`](./utils/cache/CODE_ANALYSIS.md)
- `utils/image`: [`./utils/image/CODE_ANALYSIS.md`](./utils/image/CODE_ANALYSIS.md)
- `utils/json-to-toml`: [`./utils/json-to-toml/CODE_ANALYSIS.md`](./utils/json-to-toml/CODE_ANALYSIS.md)
- `utils/home-dir`: [`./utils/home-dir/CODE_ANALYSIS.md`](./utils/home-dir/CODE_ANALYSIS.md)
- `utils/pty`: [`./utils/pty/CODE_ANALYSIS.md`](./utils/pty/CODE_ANALYSIS.md)
- `utils/readiness`: [`./utils/readiness/CODE_ANALYSIS.md`](./utils/readiness/CODE_ANALYSIS.md)
- `utils/rustls-provider`: [`./utils/rustls-provider/CODE_ANALYSIS.md`](./utils/rustls-provider/CODE_ANALYSIS.md)
- `utils/string`: [`./utils/string/CODE_ANALYSIS.md`](./utils/string/CODE_ANALYSIS.md)
- `utils/cli`: [`./utils/cli/CODE_ANALYSIS.md`](./utils/cli/CODE_ANALYSIS.md)
- `utils/elapsed`: [`./utils/elapsed/CODE_ANALYSIS.md`](./utils/elapsed/CODE_ANALYSIS.md)
- `utils/sandbox-summary`: [`./utils/sandbox-summary/CODE_ANALYSIS.md`](./utils/sandbox-summary/CODE_ANALYSIS.md)
- `utils/sleep-inhibitor`: [`./utils/sleep-inhibitor/CODE_ANALYSIS.md`](./utils/sleep-inhibitor/CODE_ANALYSIS.md)
- `utils/approval-presets`: [`./utils/approval-presets/CODE_ANALYSIS.md`](./utils/approval-presets/CODE_ANALYSIS.md)
- `utils/oss`: [`./utils/oss/CODE_ANALYSIS.md`](./utils/oss/CODE_ANALYSIS.md)
- `utils/output-truncation`: [`./utils/output-truncation/CODE_ANALYSIS.md`](./utils/output-truncation/CODE_ANALYSIS.md)
- `utils/path-utils`: [`./utils/path-utils/CODE_ANALYSIS.md`](./utils/path-utils/CODE_ANALYSIS.md)
- `utils/plugins`: [`./utils/plugins/CODE_ANALYSIS.md`](./utils/plugins/CODE_ANALYSIS.md)
- `utils/fuzzy-match`: [`./utils/fuzzy-match/CODE_ANALYSIS.md`](./utils/fuzzy-match/CODE_ANALYSIS.md)
- `utils/stream-parser`: [`./utils/stream-parser/CODE_ANALYSIS.md`](./utils/stream-parser/CODE_ANALYSIS.md)
- `utils/template`: [`./utils/template/CODE_ANALYSIS.md`](./utils/template/CODE_ANALYSIS.md)

## Other `codex-*` Crates

- `codex-api`: [`./codex-api/CODE_ANALYSIS.md`](./codex-api/CODE_ANALYSIS.md)
- `codex-backend-openapi-models`: [`./codex-backend-openapi-models/CODE_ANALYSIS.md`](./codex-backend-openapi-models/CODE_ANALYSIS.md)
- `codex-client`: [`./codex-client/CODE_ANALYSIS.md`](./codex-client/CODE_ANALYSIS.md)
- `codex-experimental-api-macros`: [`./codex-experimental-api-macros/CODE_ANALYSIS.md`](./codex-experimental-api-macros/CODE_ANALYSIS.md)
- `codex-mcp`: [`./codex-mcp/CODE_ANALYSIS.md`](./codex-mcp/CODE_ANALYSIS.md)

## Remaining Top-Level Crates

- `analytics`: [`./analytics/CODE_ANALYSIS.md`](./analytics/CODE_ANALYSIS.md)
- `ansi-escape`: [`./ansi-escape/CODE_ANALYSIS.md`](./ansi-escape/CODE_ANALYSIS.md)
- `apply-patch`: [`./apply-patch/CODE_ANALYSIS.md`](./apply-patch/CODE_ANALYSIS.md)
- `arg0`: [`./arg0/CODE_ANALYSIS.md`](./arg0/CODE_ANALYSIS.md)
- `async-utils`: [`./async-utils/CODE_ANALYSIS.md`](./async-utils/CODE_ANALYSIS.md)
- `backend-client`: [`./backend-client/CODE_ANALYSIS.md`](./backend-client/CODE_ANALYSIS.md)
- `cli`: [`./cli/CODE_ANALYSIS.md`](./cli/CODE_ANALYSIS.md)
- `code-mode`: [`./code-mode/CODE_ANALYSIS.md`](./code-mode/CODE_ANALYSIS.md)
- `collaboration-mode-templates`: [`./collaboration-mode-templates/CODE_ANALYSIS.md`](./collaboration-mode-templates/CODE_ANALYSIS.md)
- `config`: [`./config/CODE_ANALYSIS.md`](./config/CODE_ANALYSIS.md)
- `connectors`: [`./connectors/CODE_ANALYSIS.md`](./connectors/CODE_ANALYSIS.md)
- `debug-client`: [`./debug-client/CODE_ANALYSIS.md`](./debug-client/CODE_ANALYSIS.md)
- `features`: [`./features/CODE_ANALYSIS.md`](./features/CODE_ANALYSIS.md)
- `feedback`: [`./feedback/CODE_ANALYSIS.md`](./feedback/CODE_ANALYSIS.md)
- `file-search`: [`./file-search/CODE_ANALYSIS.md`](./file-search/CODE_ANALYSIS.md)
- `git-utils`: [`./git-utils/CODE_ANALYSIS.md`](./git-utils/CODE_ANALYSIS.md)
- `hooks`: [`./hooks/CODE_ANALYSIS.md`](./hooks/CODE_ANALYSIS.md)
- `install-context`: [`./install-context/CODE_ANALYSIS.md`](./install-context/CODE_ANALYSIS.md)
- `keyring-store`: [`./keyring-store/CODE_ANALYSIS.md`](./keyring-store/CODE_ANALYSIS.md)
- `linux-sandbox`: [`./linux-sandbox/CODE_ANALYSIS.md`](./linux-sandbox/CODE_ANALYSIS.md)
- `lmstudio`: [`./lmstudio/CODE_ANALYSIS.md`](./lmstudio/CODE_ANALYSIS.md)
- `login`: [`./login/CODE_ANALYSIS.md`](./login/CODE_ANALYSIS.md)
- `mcp-server`: [`./mcp-server/CODE_ANALYSIS.md`](./mcp-server/CODE_ANALYSIS.md)
- `model-provider`: [`./model-provider/CODE_ANALYSIS.md`](./model-provider/CODE_ANALYSIS.md)
- `model-provider-info`: [`./model-provider-info/CODE_ANALYSIS.md`](./model-provider-info/CODE_ANALYSIS.md)
- `models-manager`: [`./models-manager/CODE_ANALYSIS.md`](./models-manager/CODE_ANALYSIS.md)
- `network-proxy`: [`./network-proxy/CODE_ANALYSIS.md`](./network-proxy/CODE_ANALYSIS.md)
- `ollama`: [`./ollama/CODE_ANALYSIS.md`](./ollama/CODE_ANALYSIS.md)
- `otel`: [`./otel/CODE_ANALYSIS.md`](./otel/CODE_ANALYSIS.md)
- `plugin`: [`./plugin/CODE_ANALYSIS.md`](./plugin/CODE_ANALYSIS.md)
- `process-hardening`: [`./process-hardening/CODE_ANALYSIS.md`](./process-hardening/CODE_ANALYSIS.md)
- `protocol`: [`./protocol/CODE_ANALYSIS.md`](./protocol/CODE_ANALYSIS.md)
- `realtime-webrtc`: [`./realtime-webrtc/CODE_ANALYSIS.md`](./realtime-webrtc/CODE_ANALYSIS.md)
- `response-debug-context`: [`./response-debug-context/CODE_ANALYSIS.md`](./response-debug-context/CODE_ANALYSIS.md)
- `responses-api-proxy`: [`./responses-api-proxy/CODE_ANALYSIS.md`](./responses-api-proxy/CODE_ANALYSIS.md)
- `rmcp-client`: [`./rmcp-client/CODE_ANALYSIS.md`](./rmcp-client/CODE_ANALYSIS.md)
- `rollout`: [`./rollout/CODE_ANALYSIS.md`](./rollout/CODE_ANALYSIS.md)
- `sandboxing`: [`./sandboxing/CODE_ANALYSIS.md`](./sandboxing/CODE_ANALYSIS.md)
- `secrets`: [`./secrets/CODE_ANALYSIS.md`](./secrets/CODE_ANALYSIS.md)
- `shell-command`: [`./shell-command/CODE_ANALYSIS.md`](./shell-command/CODE_ANALYSIS.md)
- `shell-escalation`: [`./shell-escalation/CODE_ANALYSIS.md`](./shell-escalation/CODE_ANALYSIS.md)
- `skills`: [`./skills/CODE_ANALYSIS.md`](./skills/CODE_ANALYSIS.md)
- `state`: [`./state/CODE_ANALYSIS.md`](./state/CODE_ANALYSIS.md)
- `stdio-to-uds`: [`./stdio-to-uds/CODE_ANALYSIS.md`](./stdio-to-uds/CODE_ANALYSIS.md)
- `terminal-detection`: [`./terminal-detection/CODE_ANALYSIS.md`](./terminal-detection/CODE_ANALYSIS.md)
- `test-binary-support`: [`./test-binary-support/CODE_ANALYSIS.md`](./test-binary-support/CODE_ANALYSIS.md)
- `thread-store`: [`./thread-store/CODE_ANALYSIS.md`](./thread-store/CODE_ANALYSIS.md)
- `tools`: [`./tools/CODE_ANALYSIS.md`](./tools/CODE_ANALYSIS.md)
- `tui`: [`./tui/CODE_ANALYSIS.md`](./tui/CODE_ANALYSIS.md)
- `uds`: [`./uds/CODE_ANALYSIS.md`](./uds/CODE_ANALYSIS.md)
- `v8-poc`: [`./v8-poc/CODE_ANALYSIS.md`](./v8-poc/CODE_ANALYSIS.md)

## Full Alphabetical Index

| Crate | Analysis Document |
| --- | --- |
| `analytics` | [`./analytics/CODE_ANALYSIS.md`](./analytics/CODE_ANALYSIS.md) |
| `ansi-escape` | [`./ansi-escape/CODE_ANALYSIS.md`](./ansi-escape/CODE_ANALYSIS.md) |
| `app-server` | [`./app-server/CODE_ANALYSIS.md`](./app-server/CODE_ANALYSIS.md) |
| `app-server-client` | [`./app-server-client/CODE_ANALYSIS.md`](./app-server-client/CODE_ANALYSIS.md) |
| `app-server-protocol` | [`./app-server-protocol/CODE_ANALYSIS.md`](./app-server-protocol/CODE_ANALYSIS.md) |
| `app-server-test-client` | [`./app-server-test-client/CODE_ANALYSIS.md`](./app-server-test-client/CODE_ANALYSIS.md) |
| `apply-patch` | [`./apply-patch/CODE_ANALYSIS.md`](./apply-patch/CODE_ANALYSIS.md) |
| `arg0` | [`./arg0/CODE_ANALYSIS.md`](./arg0/CODE_ANALYSIS.md) |
| `async-utils` | [`./async-utils/CODE_ANALYSIS.md`](./async-utils/CODE_ANALYSIS.md) |
| `backend-client` | [`./backend-client/CODE_ANALYSIS.md`](./backend-client/CODE_ANALYSIS.md) |
| `cli` | [`./cli/CODE_ANALYSIS.md`](./cli/CODE_ANALYSIS.md) |
| `cloud-requirements` | [`./cloud-requirements/CODE_ANALYSIS.md`](./cloud-requirements/CODE_ANALYSIS.md) |
| `cloud-tasks` | [`./cloud-tasks/CODE_ANALYSIS.md`](./cloud-tasks/CODE_ANALYSIS.md) |
| `cloud-tasks-client` | [`./cloud-tasks-client/CODE_ANALYSIS.md`](./cloud-tasks-client/CODE_ANALYSIS.md) |
| `cloud-tasks-mock-client` | [`./cloud-tasks-mock-client/CODE_ANALYSIS.md`](./cloud-tasks-mock-client/CODE_ANALYSIS.md) |
| `code-mode` | [`./code-mode/CODE_ANALYSIS.md`](./code-mode/CODE_ANALYSIS.md) |
| `codex-api` | [`./codex-api/CODE_ANALYSIS.md`](./codex-api/CODE_ANALYSIS.md) |
| `codex-backend-openapi-models` | [`./codex-backend-openapi-models/CODE_ANALYSIS.md`](./codex-backend-openapi-models/CODE_ANALYSIS.md) |
| `codex-client` | [`./codex-client/CODE_ANALYSIS.md`](./codex-client/CODE_ANALYSIS.md) |
| `codex-experimental-api-macros` | [`./codex-experimental-api-macros/CODE_ANALYSIS.md`](./codex-experimental-api-macros/CODE_ANALYSIS.md) |
| `codex-mcp` | [`./codex-mcp/CODE_ANALYSIS.md`](./codex-mcp/CODE_ANALYSIS.md) |
| `collaboration-mode-templates` | [`./collaboration-mode-templates/CODE_ANALYSIS.md`](./collaboration-mode-templates/CODE_ANALYSIS.md) |
| `config` | [`./config/CODE_ANALYSIS.md`](./config/CODE_ANALYSIS.md) |
| `connectors` | [`./connectors/CODE_ANALYSIS.md`](./connectors/CODE_ANALYSIS.md) |
| `core` | [`./core/CODE_ANALYSIS.md`](./core/CODE_ANALYSIS.md) |
| `core-plugins` | [`./core-plugins/CODE_ANALYSIS.md`](./core-plugins/CODE_ANALYSIS.md) |
| `core-skills` | [`./core-skills/CODE_ANALYSIS.md`](./core-skills/CODE_ANALYSIS.md) |
| `debug-client` | [`./debug-client/CODE_ANALYSIS.md`](./debug-client/CODE_ANALYSIS.md) |
| `exec` | [`./exec/CODE_ANALYSIS.md`](./exec/CODE_ANALYSIS.md) |
| `exec-server` | [`./exec-server/CODE_ANALYSIS.md`](./exec-server/CODE_ANALYSIS.md) |
| `execpolicy` | [`./execpolicy/CODE_ANALYSIS.md`](./execpolicy/CODE_ANALYSIS.md) |
| `execpolicy-legacy` | [`./execpolicy-legacy/CODE_ANALYSIS.md`](./execpolicy-legacy/CODE_ANALYSIS.md) |
| `features` | [`./features/CODE_ANALYSIS.md`](./features/CODE_ANALYSIS.md) |
| `feedback` | [`./feedback/CODE_ANALYSIS.md`](./feedback/CODE_ANALYSIS.md) |
| `file-search` | [`./file-search/CODE_ANALYSIS.md`](./file-search/CODE_ANALYSIS.md) |
| `git-utils` | [`./git-utils/CODE_ANALYSIS.md`](./git-utils/CODE_ANALYSIS.md) |
| `hooks` | [`./hooks/CODE_ANALYSIS.md`](./hooks/CODE_ANALYSIS.md) |
| `install-context` | [`./install-context/CODE_ANALYSIS.md`](./install-context/CODE_ANALYSIS.md) |
| `keyring-store` | [`./keyring-store/CODE_ANALYSIS.md`](./keyring-store/CODE_ANALYSIS.md) |
| `linux-sandbox` | [`./linux-sandbox/CODE_ANALYSIS.md`](./linux-sandbox/CODE_ANALYSIS.md) |
| `lmstudio` | [`./lmstudio/CODE_ANALYSIS.md`](./lmstudio/CODE_ANALYSIS.md) |
| `login` | [`./login/CODE_ANALYSIS.md`](./login/CODE_ANALYSIS.md) |
| `mcp-server` | [`./mcp-server/CODE_ANALYSIS.md`](./mcp-server/CODE_ANALYSIS.md) |
| `model-provider` | [`./model-provider/CODE_ANALYSIS.md`](./model-provider/CODE_ANALYSIS.md) |
| `model-provider-info` | [`./model-provider-info/CODE_ANALYSIS.md`](./model-provider-info/CODE_ANALYSIS.md) |
| `models-manager` | [`./models-manager/CODE_ANALYSIS.md`](./models-manager/CODE_ANALYSIS.md) |
| `network-proxy` | [`./network-proxy/CODE_ANALYSIS.md`](./network-proxy/CODE_ANALYSIS.md) |
| `ollama` | [`./ollama/CODE_ANALYSIS.md`](./ollama/CODE_ANALYSIS.md) |
| `otel` | [`./otel/CODE_ANALYSIS.md`](./otel/CODE_ANALYSIS.md) |
| `plugin` | [`./plugin/CODE_ANALYSIS.md`](./plugin/CODE_ANALYSIS.md) |
| `process-hardening` | [`./process-hardening/CODE_ANALYSIS.md`](./process-hardening/CODE_ANALYSIS.md) |
| `protocol` | [`./protocol/CODE_ANALYSIS.md`](./protocol/CODE_ANALYSIS.md) |
| `realtime-webrtc` | [`./realtime-webrtc/CODE_ANALYSIS.md`](./realtime-webrtc/CODE_ANALYSIS.md) |
| `response-debug-context` | [`./response-debug-context/CODE_ANALYSIS.md`](./response-debug-context/CODE_ANALYSIS.md) |
| `responses-api-proxy` | [`./responses-api-proxy/CODE_ANALYSIS.md`](./responses-api-proxy/CODE_ANALYSIS.md) |
| `rmcp-client` | [`./rmcp-client/CODE_ANALYSIS.md`](./rmcp-client/CODE_ANALYSIS.md) |
| `rollout` | [`./rollout/CODE_ANALYSIS.md`](./rollout/CODE_ANALYSIS.md) |
| `sandboxing` | [`./sandboxing/CODE_ANALYSIS.md`](./sandboxing/CODE_ANALYSIS.md) |
| `secrets` | [`./secrets/CODE_ANALYSIS.md`](./secrets/CODE_ANALYSIS.md) |
| `shell-command` | [`./shell-command/CODE_ANALYSIS.md`](./shell-command/CODE_ANALYSIS.md) |
| `shell-escalation` | [`./shell-escalation/CODE_ANALYSIS.md`](./shell-escalation/CODE_ANALYSIS.md) |
| `skills` | [`./skills/CODE_ANALYSIS.md`](./skills/CODE_ANALYSIS.md) |
| `state` | [`./state/CODE_ANALYSIS.md`](./state/CODE_ANALYSIS.md) |
| `stdio-to-uds` | [`./stdio-to-uds/CODE_ANALYSIS.md`](./stdio-to-uds/CODE_ANALYSIS.md) |
| `terminal-detection` | [`./terminal-detection/CODE_ANALYSIS.md`](./terminal-detection/CODE_ANALYSIS.md) |
| `test-binary-support` | [`./test-binary-support/CODE_ANALYSIS.md`](./test-binary-support/CODE_ANALYSIS.md) |
| `thread-store` | [`./thread-store/CODE_ANALYSIS.md`](./thread-store/CODE_ANALYSIS.md) |
| `tools` | [`./tools/CODE_ANALYSIS.md`](./tools/CODE_ANALYSIS.md) |
| `tui` | [`./tui/CODE_ANALYSIS.md`](./tui/CODE_ANALYSIS.md) |
| `uds` | [`./uds/CODE_ANALYSIS.md`](./uds/CODE_ANALYSIS.md) |
| `utils/absolute-path` | [`./utils/absolute-path/CODE_ANALYSIS.md`](./utils/absolute-path/CODE_ANALYSIS.md) |
| `utils/approval-presets` | [`./utils/approval-presets/CODE_ANALYSIS.md`](./utils/approval-presets/CODE_ANALYSIS.md) |
| `utils/cache` | [`./utils/cache/CODE_ANALYSIS.md`](./utils/cache/CODE_ANALYSIS.md) |
| `utils/cargo-bin` | [`./utils/cargo-bin/CODE_ANALYSIS.md`](./utils/cargo-bin/CODE_ANALYSIS.md) |
| `utils/cli` | [`./utils/cli/CODE_ANALYSIS.md`](./utils/cli/CODE_ANALYSIS.md) |
| `utils/elapsed` | [`./utils/elapsed/CODE_ANALYSIS.md`](./utils/elapsed/CODE_ANALYSIS.md) |
| `utils/fuzzy-match` | [`./utils/fuzzy-match/CODE_ANALYSIS.md`](./utils/fuzzy-match/CODE_ANALYSIS.md) |
| `utils/home-dir` | [`./utils/home-dir/CODE_ANALYSIS.md`](./utils/home-dir/CODE_ANALYSIS.md) |
| `utils/image` | [`./utils/image/CODE_ANALYSIS.md`](./utils/image/CODE_ANALYSIS.md) |
| `utils/json-to-toml` | [`./utils/json-to-toml/CODE_ANALYSIS.md`](./utils/json-to-toml/CODE_ANALYSIS.md) |
| `utils/oss` | [`./utils/oss/CODE_ANALYSIS.md`](./utils/oss/CODE_ANALYSIS.md) |
| `utils/output-truncation` | [`./utils/output-truncation/CODE_ANALYSIS.md`](./utils/output-truncation/CODE_ANALYSIS.md) |
| `utils/path-utils` | [`./utils/path-utils/CODE_ANALYSIS.md`](./utils/path-utils/CODE_ANALYSIS.md) |
| `utils/plugins` | [`./utils/plugins/CODE_ANALYSIS.md`](./utils/plugins/CODE_ANALYSIS.md) |
| `utils/pty` | [`./utils/pty/CODE_ANALYSIS.md`](./utils/pty/CODE_ANALYSIS.md) |
| `utils/readiness` | [`./utils/readiness/CODE_ANALYSIS.md`](./utils/readiness/CODE_ANALYSIS.md) |
| `utils/rustls-provider` | [`./utils/rustls-provider/CODE_ANALYSIS.md`](./utils/rustls-provider/CODE_ANALYSIS.md) |
| `utils/sandbox-summary` | [`./utils/sandbox-summary/CODE_ANALYSIS.md`](./utils/sandbox-summary/CODE_ANALYSIS.md) |
| `utils/sleep-inhibitor` | [`./utils/sleep-inhibitor/CODE_ANALYSIS.md`](./utils/sleep-inhibitor/CODE_ANALYSIS.md) |
| `utils/stream-parser` | [`./utils/stream-parser/CODE_ANALYSIS.md`](./utils/stream-parser/CODE_ANALYSIS.md) |
| `utils/string` | [`./utils/string/CODE_ANALYSIS.md`](./utils/string/CODE_ANALYSIS.md) |
| `utils/template` | [`./utils/template/CODE_ANALYSIS.md`](./utils/template/CODE_ANALYSIS.md) |
| `v8-poc` | [`./v8-poc/CODE_ANALYSIS.md`](./v8-poc/CODE_ANALYSIS.md) |

## Maintenance Notes

- If a new workspace member is added to `Cargo.toml`, add its crate-local `CODE_ANALYSIS.md` and then append it to this index.
- Keep the link target format consistent: `./<member>/CODE_ANALYSIS.md`.
- This index is intentionally path-driven rather than architecture-opinionated, so it stays stable as crate responsibilities evolve.
