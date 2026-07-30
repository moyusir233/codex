/// Capabilities detected from one pinned `fornax-cli` version and its help.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FornaxCliCapabilities {
    /// Exact final non-empty line from `fornax-cli version`.
    pub version: String,
    /// Whether the trace help explicitly lists `get`.
    pub trace_get: bool,
    /// Whether the trace help explicitly lists `list`.
    pub trace_list: bool,
    /// Whether the trace help explicitly lists a mutation command.
    pub trace_write: bool,
    /// Whether global help explicitly advertises JSON output.
    pub json_output: bool,
    /// Whether global help explicitly advertises request timeouts.
    pub timeout: bool,
}

impl FornaxCliCapabilities {
    /// Parses machine-captured version and help output without inferring
    /// commands that are not explicitly listed.
    pub fn detect(version_output: &str, trace_help: &str) -> Self {
        Self {
            version: last_nonempty_line(version_output).to_string(),
            trace_get: command_is_listed(trace_help, "get"),
            trace_list: command_is_listed(trace_help, "list"),
            trace_write: ["write", "create", "start", "finish"]
                .into_iter()
                .any(|command| command_is_listed(trace_help, command)),
            json_output: trace_help.contains("--format string")
                && trace_help.contains("json (indented JSON)"),
            timeout: trace_help.contains("--timeout duration"),
        }
    }
}

/// Capabilities detected from one pinned `lark-cli` version and relevant help.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LarkCliCapabilities {
    /// Exact final non-empty line from `lark-cli --version`.
    pub version: String,
    /// Whether message help explicitly advertises an idempotency key.
    pub message_idempotency: bool,
    /// Whether event help explicitly identifies stdout as NDJSON.
    pub event_ndjson: bool,
    /// Whether event help explicitly labels forced parallel subscribers unsafe.
    pub event_force_is_unsafe: bool,
}

impl LarkCliCapabilities {
    /// Parses only capabilities explicitly advertised by the installed CLI.
    pub fn detect(version_output: &str, message_help: &str, event_help: &str) -> Self {
        Self {
            version: last_nonempty_line(version_output).to_string(),
            message_idempotency: message_help.contains("--idempotency-key"),
            event_ndjson: event_help.contains("NDJSON output"),
            event_force_is_unsafe: event_help.contains("--force") && event_help.contains("UNSAFE"),
        }
    }
}

fn command_is_listed(help: &str, command: &str) -> bool {
    help.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next() == Some(command) && fields.next().is_some()
    })
}

fn last_nonempty_line(output: &str) -> &str {
    output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default()
}
