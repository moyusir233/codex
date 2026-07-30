use clap::Parser;

use super::*;

fn parse_workflow(args: &[&str]) -> WorkflowCli {
    let cli =
        crate::MultitoolCli::try_parse_from(std::iter::once("codex").chain(args.iter().copied()))
            .expect("workflow CLI should parse");
    let Some(crate::Subcommand::Workflow(workflow)) = cli.subcommand else {
        panic!("expected workflow subcommand");
    };
    workflow
}

#[test]
fn runner_options_before_name_do_not_consume_workflow_arguments() {
    let workflow = parse_workflow(&[
        "workflow",
        "--json",
        "--version",
        "1.2.3",
        "release",
        "--topic",
        "launch",
    ]);
    assert!(workflow.json);
    assert_eq!(workflow.version.as_deref(), Some("1.2.3"));
    assert_eq!(workflow.workflow_name(), Some("release"));
    assert_eq!(workflow.workflow_args(), ["--topic", "launch"]);
}

#[test]
fn runner_option_spelling_after_name_is_forwarded_unchanged() {
    let workflow = parse_workflow(&["workflow", "release", "--json", "--version"]);
    assert!(!workflow.json);
    assert_eq!(workflow.workflow_args(), ["--json", "--version"]);
}

#[test]
fn explicit_separator_is_not_forwarded() {
    let workflow = parse_workflow(&["workflow", "release", "--", "--topic", "launch"]);
    assert_eq!(workflow.workflow_args(), ["--topic", "launch"]);
}

#[test]
fn interrupts_cancel_once_then_exit_130() {
    let mut interrupted = false;
    assert_eq!(next_interrupt(&mut interrupted), InterruptAction::Cancel);
    assert!(interrupted);
    assert_eq!(next_interrupt(&mut interrupted), InterruptAction::Exit);
    assert_eq!(
        exit_for_status(WorkflowRunStatus::Cancelled, interrupted),
        EXIT_INTERRUPT
    );
}
