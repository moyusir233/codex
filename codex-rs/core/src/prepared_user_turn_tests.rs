use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::TurnCompleteEvent;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn prepared_user_turn_rejects_non_v7_ids() {
    assert_eq!(
        Err(PreparedUserTurnError::SubmissionIdMustBeV7),
        PreparedUserTurn::new(uuid::Uuid::new_v4().to_string(), [1; 32])
    );
}

#[test]
fn prepared_user_turn_reconciles_every_crash_window_without_duplicate_queue() {
    let prepared =
        PreparedUserTurn::new(uuid::Uuid::now_v7().to_string(), [7; 32]).expect("prepared turn");

    let mut before_queue = PreparedUserTurnRegistry::default();
    assert_eq!(
        PreparedUserTurnSubmission::Queue,
        before_queue.register(&prepared).expect("first queue")
    );
    assert_eq!(
        PreparedUserTurnSubmission::AlreadyQueued,
        before_queue
            .register(&prepared)
            .expect("same-process retry")
    );

    let mut failed_before_queue = PreparedUserTurnRegistry::default();
    assert_eq!(
        PreparedUserTurnSubmission::Queue,
        failed_before_queue
            .register(&prepared)
            .expect("failed queue owner")
    );
    failed_before_queue.remove_queued(&prepared);
    assert_eq!(
        PreparedUserTurnSubmission::Queue,
        failed_before_queue
            .register(&prepared)
            .expect("queue retry after submit failure")
    );

    let mut after_queue_process_restart =
        PreparedUserTurnRegistry::from_history(&InitialHistory::New);
    assert_eq!(
        PreparedUserTurnSubmission::Queue,
        after_queue_process_restart
            .register(&prepared)
            .expect("queue was lost before boundary")
    );

    let history = history_with_boundary(&prepared);
    assert_eq!(1, matching_boundary_count(&history, &prepared));
    let mut after_boundary_process_restart = PreparedUserTurnRegistry::from_history(&history);
    assert_eq!(
        PreparedUserTurnSubmission::BoundaryAlreadyPersisted,
        after_boundary_process_restart
            .register(&prepared)
            .expect("persisted boundary")
    );

    let mut terminal_items = history.get_rollout_items().to_vec();
    terminal_items.push(RolloutItem::EventMsg(EventMsg::TurnComplete(
        TurnCompleteEvent {
            turn_id: prepared.submission_id(),
            last_agent_message: Some("done".to_string()),
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        },
    )));
    let terminal_history = InitialHistory::Forked(terminal_items);
    assert_eq!(1, matching_boundary_count(&terminal_history, &prepared));
    let mut after_terminal_process_restart =
        PreparedUserTurnRegistry::from_history(&terminal_history);
    assert_eq!(
        PreparedUserTurnSubmission::BoundaryAlreadyPersisted,
        after_terminal_process_restart
            .register(&prepared)
            .expect("persisted terminal")
    );
}

#[test]
fn prepared_user_turn_rejects_hash_conflicts_in_memory_and_rollout() {
    let submission_id = uuid::Uuid::now_v7().to_string();
    let first = PreparedUserTurn::new(&submission_id, [1; 32]).expect("first");
    let conflicting = PreparedUserTurn::new(&submission_id, [2; 32]).expect("conflicting");

    let mut registry = PreparedUserTurnRegistry::default();
    assert_eq!(
        PreparedUserTurnSubmission::Queue,
        registry.register(&first).expect("first queue")
    );
    assert_eq!(
        Err(PreparedUserTurnError::InputHashConflict),
        registry.register(&conflicting)
    );

    let mut rollout_items = history_with_boundary(&first).get_rollout_items().to_vec();
    rollout_items.extend(
        history_with_boundary(&conflicting)
            .get_rollout_items()
            .to_vec(),
    );
    let mut recovered =
        PreparedUserTurnRegistry::from_history(&InitialHistory::Forked(rollout_items));
    assert_eq!(
        Err(PreparedUserTurnError::InputHashConflict),
        recovered.register(&first)
    );
}

fn history_with_boundary(prepared: &PreparedUserTurn) -> InitialHistory {
    let mut user_message = UserMessageItem::new(&[]);
    user_message.client_id = Some(prepared.marker());
    InitialHistory::Forked(vec![RolloutItem::EventMsg(EventMsg::ItemCompleted(
        ItemCompletedEvent {
            thread_id: ThreadId::new(),
            turn_id: prepared.submission_id(),
            item: TurnItem::UserMessage(user_message),
            completed_at_ms: 0,
        },
    ))])
}

fn matching_boundary_count(history: &InitialHistory, prepared: &PreparedUserTurn) -> usize {
    history
        .get_rollout_items()
        .iter()
        .filter(|item| {
            matches!(
                item,
                RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
                    item: TurnItem::UserMessage(UserMessageItem {
                        client_id: Some(client_id),
                        ..
                    }),
                    ..
                })) if client_id == &prepared.marker()
            )
        })
        .count()
}
