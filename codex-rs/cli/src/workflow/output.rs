use std::collections::HashSet;
use std::io;
use std::io::Write;

use codex_app_server_protocol::WorkflowDefinitionSummary;
use codex_app_server_protocol::WorkflowEvent;
use codex_app_server_protocol::WorkflowRun;
use serde::Serialize;
use serde_json::json;

pub(super) struct WorkflowOutput<'a> {
    json: bool,
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
}

impl<'a> WorkflowOutput<'a> {
    pub(super) fn new(json: bool, stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Self {
        Self {
            json,
            stdout,
            stderr,
        }
    }

    pub(super) fn definitions(
        &mut self,
        definitions: &[WorkflowDefinitionSummary],
    ) -> io::Result<()> {
        if self.json {
            for definition in definitions {
                self.json_line(&json!({
                    "type": "definition",
                    "definition": definition,
                }))?;
            }
            return self.json_line(&json!({
                "type": "result",
                "definitionCount": definitions.len(),
            }));
        }

        if definitions.is_empty() {
            writeln!(self.stdout, "No workflows are registered.")?;
            return Ok(());
        }
        for definition in definitions {
            let default = if definition.is_default {
                " (default)"
            } else {
                ""
            };
            writeln!(
                self.stdout,
                "{} {}{} — {}",
                definition.name, definition.version, default, definition.description
            )?;
        }
        Ok(())
    }

    pub(super) fn workflow_help(
        &mut self,
        definition: &WorkflowDefinitionSummary,
    ) -> io::Result<()> {
        if self.json {
            return self.json_line(&json!({
                "type": "workflowHelp",
                "definition": definition,
            }));
        }
        write!(self.stdout, "{}", definition.argv_help)
    }

    pub(super) fn progress(&mut self, message: &str) -> io::Result<()> {
        if self.json {
            return self.json_line(&json!({
                "type": "progress",
                "message": message,
            }));
        }
        writeln!(self.stderr, "{message}")
    }

    pub(super) fn event<T: Serialize>(&mut self, event_type: &str, event: &T) -> io::Result<()> {
        if self.json {
            return self.json_line(&json!({
                "type": event_type,
                "event": event,
            }));
        }
        writeln!(self.stderr, "{event_type}: {}", compact_json(event))
    }

    pub(super) fn replay_event(&mut self, event: &WorkflowEvent) -> io::Result<()> {
        self.event("workflowEvent", event)
    }

    pub(super) fn error(&mut self, code: &str, message: &str) -> io::Result<()> {
        if self.json {
            return self.json_line(&json!({
                "type": "error",
                "code": code,
                "message": message,
            }));
        }
        writeln!(self.stderr, "Error: {message}")
    }

    pub(super) fn result(&mut self, run: &WorkflowRun, detached: bool) -> io::Result<()> {
        let resume_commands = resume_commands(run);
        if self.json {
            return self.json_line(&json!({
                "type": "result",
                "detached": detached,
                "run": run,
                "resumeCommands": resume_commands,
            }));
        }

        writeln!(
            self.stdout,
            "Workflow run {}: {:?}{}",
            run.run_id,
            run.status,
            if detached { " (detached)" } else { "" }
        )?;
        if let Some(output) = &run.output {
            writeln!(self.stdout, "Output: {}", compact_json(output))?;
        }
        if let Some(error_code) = &run.error_code {
            writeln!(self.stdout, "Error code: {error_code}")?;
        }
        for artifact in &run.artifacts {
            writeln!(
                self.stdout,
                "Artifact {}: {} ({}, {} bytes)",
                artifact.artifact_id,
                artifact.relative_path,
                artifact.classification,
                artifact.size_bytes
            )?;
        }
        for command in resume_commands {
            writeln!(self.stdout, "{command}")?;
        }
        Ok(())
    }

    fn json_line<T: Serialize>(&mut self, value: &T) -> io::Result<()> {
        serde_json::to_writer(&mut self.stdout, value)?;
        writeln!(self.stdout)
    }
}

fn compact_json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"<unserializable>\"".to_string())
}

fn resume_commands(run: &WorkflowRun) -> Vec<String> {
    let mut seen = HashSet::new();
    run.nodes
        .iter()
        .flat_map(|node| node.thread_ids.iter().chain(node.thread_id.iter()))
        .filter(|thread_id| seen.insert((*thread_id).clone()))
        .map(|thread_id| format!("codex resume {thread_id}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::WorkflowArtifactSummary;
    use codex_app_server_protocol::WorkflowNodeStatus;
    use codex_app_server_protocol::WorkflowNodeSummary;
    use codex_app_server_protocol::WorkflowRunStatus;
    use serde_json::json;

    fn completed_run() -> WorkflowRun {
        WorkflowRun {
            run_id: "wfr_01".to_string(),
            workflow_name: "release".to_string(),
            workflow_version: "1.0.0".to_string(),
            status: WorkflowRunStatus::Succeeded,
            arguments: json!({}),
            non_interactive: false,
            detached: false,
            concurrency: Some(2),
            output: Some(json!({"ok": true})),
            error_code: None,
            wake: None,
            next_sequence: 3,
            nodes: vec![WorkflowNodeSummary {
                node_id: "wfn_01".to_string(),
                node_key: "build".to_string(),
                thread_id: Some("thr_02".to_string()),
                thread_ids: vec!["thr_01".to_string(), "thr_02".to_string()],
                status: WorkflowNodeStatus::Succeeded,
                retry_at_ms: None,
                created_at_ms: 1,
                updated_at_ms: 2,
            }],
            artifacts: vec![WorkflowArtifactSummary {
                artifact_id: "wfa_01".to_string(),
                relative_path: "reports/final.json".to_string(),
                classification: "public".to_string(),
                media_type: "application/json".to_string(),
                size_bytes: 12,
                sha256: "abc".to_string(),
                created_at_ms: 2,
            }],
            created_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    #[test]
    fn json_mode_writes_only_ndjson_to_stdout() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut output = WorkflowOutput::new(true, &mut stdout, &mut stderr);
        output.progress("running").expect("progress");
        output
            .result(&completed_run(), false)
            .expect("final result");

        assert!(stderr.is_empty());
        let lines = String::from_utf8(stdout).expect("utf8");
        let values = lines
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("ndjson line"))
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0]["type"], "progress");
        assert_eq!(values[1]["resumeCommands"][0], "codex resume thr_01");
        assert_eq!(values[1]["resumeCommands"][1], "codex resume thr_02");
    }

    #[test]
    fn human_mode_keeps_progress_off_stdout() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut output = WorkflowOutput::new(false, &mut stdout, &mut stderr);
        output.progress("running").expect("progress");
        output
            .result(&completed_run(), false)
            .expect("final result");

        let stdout = String::from_utf8(stdout).expect("utf8");
        let stderr = String::from_utf8(stderr).expect("utf8");
        assert!(!stdout.contains("running"));
        assert!(stderr.contains("running"));
        assert!(stdout.contains("codex resume thr_01"));
        assert!(stdout.contains("Artifact wfa_01"));
    }
}
