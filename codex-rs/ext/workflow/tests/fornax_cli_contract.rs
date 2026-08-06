#![cfg(unix)]
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use codex_workflow_extension::integrations::fornax::FornaxCli;
use codex_workflow_extension::integrations::fornax::FornaxCliConfig;
use codex_workflow_extension::integrations::fornax::FornaxCliEnvironment;
use codex_workflow_extension::integrations::fornax::FornaxCliError;
use codex_workflow_extension::integrations::fornax::PromptDraft;
use codex_workflow_extension::integrations::fornax::PromptLookup;
use codex_workflow_extension::integrations::fornax::PromptRenderError;
use codex_workflow_extension::integrations::fornax::SkillLookup;
use codex_workflow_extension::integrations::fornax::SpanListRequest;
use codex_workflow_extension::integrations::fornax::TimeWindow;
use codex_workflow_extension::integrations::fornax::TraceGetRequest;
use codex_workflow_extension::integrations::fornax::TraceListRequest;
use codex_workflow_extension::integrations::fornax::TraceLookup;
use codex_workflow_extension::integrations::fornax::TrajectoryLookup;
use codex_workflow_extension::integrations::fornax::render_normal_prompt;
use codex_workflow_extension::integrations::process::ProcessError;
use pretty_assertions::assert_eq;
use serde_json::json;

const PROMPT: &str = include_str!("fixtures/fornax-cli/v0.0.51/prompt.json");
const DRAFT_SAVE: &str = include_str!("fixtures/fornax-cli/v0.0.51/draft-save.json");
const SKILL: &str = include_str!("fixtures/fornax-cli/v0.0.51/skill.json");
const SKILLS: &str = include_str!("fixtures/fornax-cli/v0.0.51/skills.json");
const TRACE: &str = include_str!("fixtures/fornax-cli/v0.0.51/trace.json");
const SPAN_PAGE: &str = include_str!("fixtures/fornax-cli/v0.0.51/span-page.json");
const TRAJECTORY: &str = include_str!("fixtures/fornax-cli/v0.0.51/trajectory.json");

struct FakeCli {
    temp: tempfile::TempDir,
    executable: PathBuf,
    fixture_root: PathBuf,
    arg_log: PathBuf,
    mode_log: PathBuf,
}

impl FakeCli {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let fixture_root = temp.path().join("fixtures");
        std::fs::create_dir(&fixture_root)?;
        for (name, contents) in [
            ("prompt.json", PROMPT),
            ("draft-save.json", DRAFT_SAVE),
            ("skill.json", SKILL),
            ("skills.json", SKILLS),
            ("trace.json", TRACE),
            ("span-page.json", SPAN_PAGE),
            ("trajectory.json", TRAJECTORY),
        ] {
            std::fs::write(fixture_root.join(name), contents)?;
        }
        let executable = temp.path().join("fornax-cli");
        std::fs::write(&executable, FAKE_CLI)?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            arg_log: temp.path().join("args.log"),
            mode_log: temp.path().join("mode.log"),
            temp,
            executable,
            fixture_root,
        })
    }

    fn client(
        &self,
        mode: &str,
        version: &str,
        timeout: Duration,
        stdout_limit: usize,
    ) -> FornaxCli {
        let environment = FornaxCliEnvironment::default()
            .with_value("PATH", "/usr/bin:/bin")
            .with_value("FIXTURE_ROOT", self.fixture_root.as_os_str())
            .with_value("ARG_LOG", self.arg_log.as_os_str())
            .with_value("MODE_LOG", self.mode_log.as_os_str())
            .with_value("FAKE_MODE", mode)
            .with_value("FAKE_VERSION", version)
            .with_secret("FORNAX_SK", "super-secret-sk");
        FornaxCli::new(FornaxCliConfig {
            executable: self.executable.clone(),
            cwd: self.temp.path().to_path_buf(),
            environment,
            timeout,
            stdout_limit,
            stderr_limit: 4096,
        })
    }

    fn args(&self) -> String {
        std::fs::read_to_string(&self.arg_log).unwrap_or_default()
    }
}

#[test]
fn version_is_pinned_and_never_claims_cli_trace_write() -> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let client = fake.client("normal", "0.0.51", Duration::from_secs(1), 64 * 1024);
    let (version, capabilities) = client.verify(&AtomicBool::new(false))?;
    assert_eq!("0.0.51", version.as_version().to_string());
    assert!(capabilities.prompt_read);
    assert!(capabilities.prompt_draft_save);
    assert!(capabilities.skill_read_and_stage);
    assert!(capabilities.trace_read);
    assert!(!capabilities.trace_write);

    let unsupported = fake
        .client("normal", "0.0.52", Duration::from_secs(1), 64 * 1024)
        .verify(&AtomicBool::new(false));
    assert!(matches!(
        unsupported,
        Err(FornaxCliError::UnsupportedVersion { .. })
    ));
    let malformed = fake
        .client("normal", "not-semver", Duration::from_secs(1), 64 * 1024)
        .verify(&AtomicBool::new(false));
    assert_eq!(Err(FornaxCliError::MalformedVersion), malformed);
    Ok(())
}

#[test]
fn prompt_revision_render_and_private_full_draft_save_are_typed()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let client = fake.client("noisy", "0.0.51", Duration::from_secs(1), 64 * 1024);
    client.verify(&AtomicBool::new(false))?;
    let prompt = client.get_prompt(
        &PromptLookup::Key {
            key: "demo.workflow.greeting".to_string(),
            version: Some("1.2.3".to_string()),
            with_draft: true,
            commit_version: Some("1.2.3".to_string()),
        },
        &AtomicBool::new(false),
    )?;
    assert_eq!("prompt_demo_01", prompt.prompt_id);
    let draft = PromptDraft::new(prompt.draft.expect("fixture draft"))?;
    let rendered = render_normal_prompt(
        &draft,
        &BTreeMap::from([
            ("name".to_string(), "Codex".to_string()),
            ("topic".to_string(), "a release".to_string()),
        ]),
    )?;
    assert_eq!("You are Codex.", rendered[0].content);
    assert_eq!("Prepare a release.", rendered[1].content);

    let saved = client.save_prompt_draft("prompt_demo_01", &draft, &AtomicBool::new(false))?;
    assert_eq!(json!({"prompt_id": "prompt_demo_01", "saved": true}), saved);
    assert_eq!("600\n", std::fs::read_to_string(&fake.mode_log)?);
    let args = fake.args();
    assert!(args.contains("--version 1.2.3"));
    assert!(args.contains("--with-draft"));
    assert!(args.contains("--with-commit --commit-version 1.2.3"));
    assert!(args.contains("prompt draft save"));
    assert!(!args.contains("You are Codex"));
    assert!(!args.contains("super-secret-sk"));
    Ok(())
}

#[test]
fn draft_and_local_rendering_fail_closed_for_unverified_shapes() {
    assert!(PromptDraft::new(json!({"detail": {}})).is_err());
    assert!(
        PromptDraft::new(json!({
            "detail": {
                "prompt_template": {
                    "template_type": "jinja2",
                    "messages": [{"role": "user", "content": "hello"}]
                }
            }
        }))
        .is_err()
    );

    let draft = PromptDraft::new(json!({
        "detail": {
            "prompt_template": {
                "template_type": "normal",
                "messages": [{"role": "user", "content": "Hello {{name}}"}],
                "variable_defs": [{"key": "name", "type": "string"}]
            }
        }
    }))
    .expect("valid normal draft");
    assert_eq!(
        Err(PromptRenderError::MissingVariable("name".to_string())),
        render_normal_prompt(&draft, &BTreeMap::new())
    );
}

#[test]
fn skill_reads_and_install_use_only_an_explicit_staging_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let client = fake.client("normal", "0.0.51", Duration::from_secs(1), 64 * 1024);
    let one = client.get_skills(
        &SkillLookup::Id {
            skill_id: "skill_demo_01".to_string(),
            version: Some("1.0.0".to_string()),
        },
        &AtomicBool::new(false),
    )?;
    assert_eq!("demo_skill", one[0].skill_key);
    let by_id = client.get_skills(
        &SkillLookup::Ids {
            ids: vec!["skill_demo_01".to_string(), "skill_demo_02".to_string()],
            version: Some("1.0.0".to_string()),
        },
        &AtomicBool::new(false),
    )?;
    assert_eq!(2, by_id.len());
    let batch = client.get_skills(
        &SkillLookup::Keys {
            keys: vec!["demo_skill".to_string(), "review_skill".to_string()],
            version: Some("1.0.0".to_string()),
        },
        &AtomicBool::new(false),
    )?;
    assert_eq!(2, batch.len());

    let staging = fake.temp.path().join("staging");
    let installed = client.stage_skills(
        &["demo_skill".to_string()],
        &staging,
        &AtomicBool::new(false),
    )?;
    assert_eq!(json!({"installed": true}), installed);
    assert!(staging.join("demo_skill/SKILL.md").is_file());
    assert!(fake.args().contains(&format!(
        "skill install demo_skill --dir {}",
        staging.display()
    )));
    assert!(
        client
            .stage_skills(
                &["demo_skill".to_string()],
                Path::new("relative"),
                &AtomicBool::new(false)
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn trace_span_and_trajectory_readers_preserve_stable_windows_and_pagination()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let client = fake.client("normal", "0.0.51", Duration::from_secs(1), 64 * 1024);
    let trace = client.get_trace(
        &TraceGetRequest {
            lookup: TraceLookup::TraceId("4bf92f3577b34da6a3ce929d0e0e4736".to_string()),
            span_ids: Vec::new(),
            time_window: Some(TimeWindow::EpochMillis {
                start_ms: 1000,
                end_ms: 2000,
            }),
            tree: true,
        },
        &AtomicBool::new(false),
    )?;
    assert_eq!(2, trace.spans.len());
    let traces = client.list_traces(
        &TraceListRequest {
            page_size: 2,
            trace_filter: Some("duration > 100".to_string()),
            span_filter: Some("span_type = 'model'".to_string()),
            time_window: TimeWindow::LastMinutes(60),
        },
        &AtomicBool::new(false),
    )?;
    assert_eq!(2, traces.spans.len());
    let spans = client.list_spans(
        &SpanListRequest {
            page_size: 1,
            page_token: Some("page-1".to_string()),
            span_filter: Some("span_type = 'model'".to_string()),
            time_window: TimeWindow::EpochMillis {
                start_ms: 1000,
                end_ms: 2000,
            },
        },
        &AtomicBool::new(false),
    )?;
    assert_eq!(
        Some("sanitized-next-page"),
        spans.next_page_token.as_deref()
    );
    let trajectories = client.get_trajectories(
        &TrajectoryLookup::TraceIds(vec!["4bf92f3577b34da6a3ce929d0e0e4736".to_string()]),
        None,
        &AtomicBool::new(false),
    )?;
    assert_eq!(1, trajectories.len());
    let args = fake.args();
    assert!(args.contains("--start-ms 1000 --end-ms 2000"));
    assert!(args.contains("--page-token page-1"));
    assert!(args.contains("trajectory --trace-id"));
    Ok(())
}

#[test]
fn timeout_cancellation_and_output_bounds_terminate_the_child()
-> Result<(), Box<dyn std::error::Error>> {
    let fake = FakeCli::new()?;
    let timeout = fake
        .client("wait", "0.0.51", Duration::from_millis(30), 64 * 1024)
        .verify(&AtomicBool::new(false));
    assert!(matches!(
        timeout,
        Err(FornaxCliError::Process(ProcessError::Timeout))
    ));
    let started = Instant::now();
    let descendant = fake
        .client("descendant", "0.0.51", Duration::from_millis(30), 64 * 1024)
        .verify(&AtomicBool::new(false));
    assert!(matches!(
        descendant,
        Err(FornaxCliError::Process(ProcessError::Timeout))
    ));
    assert!(started.elapsed() < Duration::from_secs(1));

    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancelled);
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        signal.store(true, Ordering::Release);
    });
    let result = fake
        .client("wait", "0.0.51", Duration::from_secs(1), 64 * 1024)
        .verify(&cancelled);
    thread.join().expect("cancellation thread");
    assert!(matches!(
        result,
        Err(FornaxCliError::Process(ProcessError::Cancelled))
    ));

    let oversized = fake
        .client("oversized", "0.0.51", Duration::from_secs(1), 32)
        .verify(&AtomicBool::new(false));
    assert!(matches!(
        oversized,
        Err(FornaxCliError::Process(ProcessError::OutputLimit {
            stream: "stdout",
            limit: 32
        }))
    ));
    let oversized_stderr = fake
        .client(
            "oversized-stderr",
            "0.0.51",
            Duration::from_secs(1),
            64 * 1024,
        )
        .verify(&AtomicBool::new(false));
    assert!(matches!(
        oversized_stderr,
        Err(FornaxCliError::Process(ProcessError::OutputLimit {
            stream: "stderr",
            limit: 4096
        }))
    ));
    Ok(())
}

#[test]
fn auth_parse_and_process_errors_never_expose_credentials() -> Result<(), Box<dyn std::error::Error>>
{
    let fake = FakeCli::new()?;
    let auth = fake
        .client("auth", "0.0.51", Duration::from_secs(1), 64 * 1024)
        .get_prompt(
            &PromptLookup::Id {
                prompt_id: "prompt_demo_01".to_string(),
                with_draft: false,
                commit_version: None,
            },
            &AtomicBool::new(false),
        );
    assert_eq!(Err(FornaxCliError::MissingAuthentication), auth);
    let workspace = fake
        .client("workspace", "0.0.51", Duration::from_secs(1), 64 * 1024)
        .get_prompt(
            &PromptLookup::Id {
                prompt_id: "prompt_demo_01".to_string(),
                with_draft: false,
                commit_version: None,
            },
            &AtomicBool::new(false),
        );
    assert_eq!(Err(FornaxCliError::MissingWorkspace), workspace);

    let malformed = fake
        .client("malformed", "0.0.51", Duration::from_secs(1), 64 * 1024)
        .get_prompt(
            &PromptLookup::Id {
                prompt_id: "prompt_demo_01".to_string(),
                with_draft: false,
                commit_version: None,
            },
            &AtomicBool::new(false),
        );
    assert!(matches!(
        malformed,
        Err(FornaxCliError::InvalidResponse { .. })
    ));

    let process_error = fake
        .client("exit", "0.0.51", Duration::from_secs(1), 64 * 1024)
        .get_prompt(
            &PromptLookup::Id {
                prompt_id: "prompt_demo_01".to_string(),
                with_draft: false,
                commit_version: None,
            },
            &AtomicBool::new(false),
        )
        .expect_err("fake exit should fail");
    let diagnostic = process_error.to_string();
    assert!(!diagnostic.contains("super-secret-sk"));
    assert!(diagnostic.contains("[REDACTED]"));
    Ok(())
}

const FAKE_CLI: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$ARG_LOG"
case "$FAKE_MODE" in
  wait) while :; do :; done ;;
  descendant) (while :; do /bin/sleep 1; done) & wait ;;
  oversized)
    i=0
    while [ "$i" -lt 200 ]; do printf 'xxxxxxxxxx'; i=$((i + 1)); done
    exit 0
    ;;
  oversized-stderr)
    i=0
    while [ "$i" -lt 500 ]; do printf 'xxxxxxxxxx' >&2; i=$((i + 1)); done
    printf 'fornax-cli v%s\n' "$FAKE_VERSION"
    exit 0
    ;;
  auth)
    printf 'authentication failed FORNAX_SK=%s\n' "$FORNAX_SK" >&2
    exit 9
    ;;
  workspace)
    printf 'workspace is not configured\n' >&2
    exit 8
    ;;
  malformed)
    printf '{not-json'
    exit 0
    ;;
  exit)
    printf 'provider failed secret=%s\n' "$FORNAX_SK" >&2
    exit 7
    ;;
  noisy) printf 'unrelated metrics warning\n' >&2 ;;
esac

if [ "$1" = "version" ]; then
  printf 'fornax-cli v%s\n' "$FAKE_VERSION"
  exit 0
fi

case "$*" in
  *"prompt get-by-key"*|*"prompt get-by-id"*)
    /bin/cat "$FIXTURE_ROOT/prompt.json"
    ;;
  *"prompt draft save"*)
    previous=
    for argument in "$@"; do
      if [ "$previous" = "--draft-file" ]; then
        /usr/bin/stat -c '%a' "$argument" > "$MODE_LOG"
      fi
      previous=$argument
    done
    /bin/cat "$FIXTURE_ROOT/draft-save.json"
    ;;
  *"skill batch-get-by-key"*|*"skill batch-get-by-id"*)
    /bin/cat "$FIXTURE_ROOT/skills.json"
    ;;
  *"skill get"*)
    /bin/cat "$FIXTURE_ROOT/skill.json"
    ;;
  *"skill install"*)
    previous=
    for argument in "$@"; do
      if [ "$previous" = "--dir" ]; then
        /bin/mkdir -p "$argument/demo_skill"
        printf '---\nname: demo_skill\n---\n' > "$argument/demo_skill/SKILL.md"
      fi
      previous=$argument
    done
    printf '{"data":{"installed":true}}\n'
    ;;
  *"trace get"*|*"trace list"*)
    /bin/cat "$FIXTURE_ROOT/trace.json"
    ;;
  *"span list"*)
    /bin/cat "$FIXTURE_ROOT/span-page.json"
    ;;
  *"trajectory"*)
    /bin/cat "$FIXTURE_ROOT/trajectory.json"
    ;;
  *)
    printf 'unsupported fake command\n' >&2
    exit 2
    ;;
esac
"#;
