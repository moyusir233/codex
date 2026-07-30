#![allow(clippy::expect_used)]

mod support;

use std::num::NonZeroU32;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_state::WorkflowNodeCreate;
use codex_state::WorkflowNodeStatus;
use codex_state::WorkflowNodeTransition;
use codex_workflow_extension::BackoffPolicy;
use codex_workflow_extension::ConcurrencyLimiter;
use codex_workflow_extension::DependencyResolution;
use codex_workflow_extension::DurableScheduler;
use codex_workflow_extension::RetryClassification;
use codex_workflow_extension::RetryDecision;
use codex_workflow_extension::RetryPlanner;
use codex_workflow_extension::RetryPolicy;
use codex_workflow_extension::RetrySession;
use codex_workflow_extension::WorkflowGraph;
use codex_workflow_extension::WorkflowRunId;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn scheduler_runs_chain_fanout_fanin_fairly_and_rejects_cycles() {
    let (_home, runtime) = support::runtime().await;
    let run_id = WorkflowRunId::new();
    support::create_run(
        runtime.as_ref(),
        run_id,
        "scheduler-test",
        "1.0.0",
        1,
        json!({}),
    )
    .await;
    let store = runtime.workflows();
    for (node_id, node_key, failure_policy) in [
        ("root", "00-root", "fail_fast"),
        ("branch-a", "10-branch-a", "fail_fast"),
        ("branch-b", "11-branch-b", "fail_fast"),
        ("join", "20-join", "skip_dependents"),
        ("quorum", "21-quorum", "fail_fast"),
        ("terminal-join", "22-terminal-join", "fail_fast"),
    ] {
        store
            .create_node(
                &run_id.to_string(),
                WorkflowNodeCreate {
                    node_id: node_id.to_string(),
                    node_key: node_key.to_string(),
                    spec: json!({}),
                    status: WorkflowNodeStatus::Pending,
                    failure_policy: failure_policy.to_string(),
                    created_at_ms: 101,
                },
                &[],
            )
            .await
            .expect("create scheduler node");
    }
    for downstream in ["branch-a", "branch-b"] {
        store
            .add_node_dependency(
                &run_id.to_string(),
                downstream,
                "root",
                "all_succeeded",
                None,
                102,
            )
            .await
            .expect("add branch dependency");
    }
    for upstream in ["branch-a", "branch-b"] {
        store
            .add_node_dependency(
                &run_id.to_string(),
                "join",
                upstream,
                "all_succeeded",
                None,
                103,
            )
            .await
            .expect("add all-success fan-in");
        store
            .add_node_dependency(
                &run_id.to_string(),
                "quorum",
                upstream,
                "at_least",
                Some(1),
                104,
            )
            .await
            .expect("add quorum fan-in");
        store
            .add_node_dependency(
                &run_id.to_string(),
                "terminal-join",
                upstream,
                "all_terminal",
                None,
                104,
            )
            .await
            .expect("add all-terminal fan-in");
    }
    assert!(matches!(
        store
            .add_node_dependency(
                &run_id.to_string(),
                "root",
                "join",
                "all_terminal",
                None,
                105,
            )
            .await
            .expect_err("cycle must be rejected"),
        codex_state::WorkflowStoreError::DependencyCycle
    ));

    let scheduler = DurableScheduler::new(
        store.clone(),
        NonZeroUsize::new(2).expect("nonzero scheduler cap"),
    );
    let first = scheduler
        .schedule_ready(&run_id.to_string(), 110)
        .await
        .expect("schedule root");
    assert_eq!(
        first
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["root"]
    );
    transition(store, "root", WorkflowNodeStatus::Succeeded, 111).await;
    let branches = scheduler
        .schedule_ready(&run_id.to_string(), 112)
        .await
        .expect("schedule fan-out");
    assert_eq!(
        branches
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["branch-a", "branch-b"]
    );
    for node in &branches {
        transition(store, &node.node_id, WorkflowNodeStatus::Running, 113).await;
    }
    assert!(
        scheduler
            .schedule_ready(&run_id.to_string(), 114)
            .await
            .expect("active cap")
            .is_empty()
    );
    transition(store, "branch-a", WorkflowNodeStatus::Succeeded, 115).await;
    transition(store, "branch-b", WorkflowNodeStatus::Failed, 116).await;
    let fan_in = scheduler
        .schedule_ready(&run_id.to_string(), 117)
        .await
        .expect("resolve fan-in");
    assert_eq!(
        fan_in
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["quorum", "terminal-join"]
    );
    assert_eq!(
        store
            .read_node("join")
            .await
            .expect("read skipped join")
            .expect("join")
            .status,
        WorkflowNodeStatus::Cancelled
    );

    let mut fail_fast_nodes = store
        .list_nodes_for_scheduler(&run_id.to_string())
        .await
        .expect("load graph");
    fail_fast_nodes
        .iter_mut()
        .find(|node| node.node_id == "join")
        .expect("join")
        .failure_policy = "fail_fast".to_string();
    let graph = WorkflowGraph::new(
        fail_fast_nodes,
        store
            .list_node_dependencies(&run_id.to_string())
            .await
            .expect("load dependencies"),
    )
    .expect("valid graph");
    assert_eq!(
        graph.resolve("join").expect("resolve fail-fast"),
        DependencyResolution::FailFast
    );
    runtime.close().await;
}

#[tokio::test]
async fn scheduler_enforces_run_and_branch_concurrency_caps() {
    let limiter = ConcurrencyLimiter::new(NonZeroUsize::new(3).expect("run cap"));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let branch_a_active = Arc::new(AtomicUsize::new(0));
    let branch_a_peak = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for index in 0..12 {
        let limiter = limiter.clone();
        let active = Arc::clone(&active);
        let peak = Arc::clone(&peak);
        let branch_a_active = Arc::clone(&branch_a_active);
        let branch_a_peak = Arc::clone(&branch_a_peak);
        tasks.push(tokio::spawn(async move {
            let branch = if index % 2 == 0 { "a" } else { "b" };
            let _permit = limiter
                .acquire(Some((branch, NonZeroUsize::new(2).expect("branch cap"))))
                .await
                .expect("acquire concurrency");
            let current = active.fetch_add(1, Ordering::AcqRel) + 1;
            peak.fetch_max(current, Ordering::AcqRel);
            if branch == "a" {
                let current = branch_a_active.fetch_add(1, Ordering::AcqRel) + 1;
                branch_a_peak.fetch_max(current, Ordering::AcqRel);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            active.fetch_sub(1, Ordering::AcqRel);
            if branch == "a" {
                branch_a_active.fetch_sub(1, Ordering::AcqRel);
            }
        }));
    }
    for task in tasks {
        task.await.expect("join bounded node");
    }
    assert_eq!(peak.load(Ordering::Acquire), 3);
    assert!(branch_a_peak.load(Ordering::Acquire) <= 2);
}

#[test]
fn scheduler_retry_backoff_is_deterministic_capped_classified_and_deadline_bounded() {
    let policy = RetryPolicy {
        maximum_attempts: NonZeroU32::new(4).expect("attempt cap"),
        backoff: BackoffPolicy::Exponential {
            initial_delay_ms: 100,
            maximum_delay_ms: 250,
            jitter_percent: 20,
        },
        session: RetrySession::FreshThread,
        retry_on: vec![RetryClassification::ModelTransient],
    };
    let first = RetryPlanner::decide(
        &policy,
        1,
        RetryClassification::ModelTransient,
        "attempt-a",
        1_000,
        Some(2_000),
    );
    assert_eq!(
        first,
        RetryPlanner::decide(
            &policy,
            1,
            RetryClassification::ModelTransient,
            "attempt-a",
            1_000,
            Some(2_000),
        )
    );
    assert!(matches!(
        first,
        RetryDecision::RetryAt {
            retry_at_ms: 1_080..=1_120,
            session: RetrySession::FreshThread,
        }
    ));
    assert_eq!(
        RetryPlanner::decide(
            &policy,
            1,
            RetryClassification::ToolTransient,
            "attempt-a",
            1_000,
            None,
        ),
        RetryDecision::Fail
    );
    assert_eq!(
        RetryPlanner::decide(
            &policy,
            3,
            RetryClassification::ModelTransient,
            "attempt-a",
            1_000,
            Some(1_100),
        ),
        RetryDecision::Fail
    );
}

async fn transition(
    store: &codex_state::WorkflowStore,
    node_id: &str,
    status: WorkflowNodeStatus,
    now_ms: i64,
) {
    let node = store
        .read_node(node_id)
        .await
        .expect("read node")
        .expect("node");
    store
        .transition_node(
            node_id,
            node.row_version,
            WorkflowNodeTransition {
                status,
                retry_at_ms: None,
                event_kind: "test.node_transition".to_string(),
                event_metadata: json!({}),
                updated_at_ms: now_ms,
            },
        )
        .await
        .expect("transition node");
}
