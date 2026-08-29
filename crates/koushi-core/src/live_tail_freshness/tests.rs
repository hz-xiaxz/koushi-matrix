use super::{
    FOREGROUND_LIVE_TAIL_LIMIT, LiveTailFreshnessState, LiveTailRefreshCoordinator,
    LiveTailRefreshOutcome, LiveTailSchedulerAction,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum TestKey {
    A,
    B,
    C,
}

use TestKey::{A, B, C};

#[test]
fn active_unproven_room_starts_once_and_same_epoch_does_not_retry_after_fresh() {
    let mut coordinator = LiveTailRefreshCoordinator::new();

    assert_eq!(
        coordinator.activate(A, 7),
        vec![LiveTailSchedulerAction::Start {
            key: A,
            epoch: 7,
            operation_generation: 1,
            limit: FOREGROUND_LIVE_TAIL_LIMIT,
        }]
    );
    assert_eq!(coordinator.mark_unproven(A, 7), Vec::new());
    assert_eq!(
        coordinator.finish(A, 7, 1, LiveTailRefreshOutcome::Unchanged),
        Vec::new()
    );
    assert_eq!(coordinator.mark_unproven(A, 7), Vec::new());
    assert_eq!(coordinator.activate(A, 7), Vec::new());
    assert_eq!(
        coordinator.freshness(&A),
        Some(LiveTailFreshnessState::Fresh { epoch: 7 })
    );
}

#[test]
fn activating_b_preempts_a_and_delays_a_before_starting_b() {
    let mut coordinator = LiveTailRefreshCoordinator::new();

    assert_eq!(
        coordinator.activate(A, 7),
        vec![LiveTailSchedulerAction::Start {
            key: A,
            epoch: 7,
            operation_generation: 1,
            limit: 128,
        }]
    );
    assert_eq!(
        coordinator.activate(B, 9),
        vec![
            LiveTailSchedulerAction::CancelNetwork {
                key: A,
                operation_generation: 1,
            },
            LiveTailSchedulerAction::Start {
                key: B,
                epoch: 9,
                operation_generation: 2,
                limit: 128,
            },
        ]
    );
    assert_eq!(
        coordinator.freshness(&A),
        Some(LiveTailFreshnessState::Deferred { epoch: 7 })
    );
    assert_eq!(
        coordinator.finish(A, 7, 1, LiveTailRefreshOutcome::Cancelled),
        Vec::new()
    );
    assert_eq!(
        coordinator.finish(B, 9, 2, LiveTailRefreshOutcome::Unchanged),
        vec![LiveTailSchedulerAction::Start {
            key: A,
            epoch: 7,
            operation_generation: 3,
            limit: 128,
        }]
    );
}

#[test]
fn late_old_epoch_finish_cannot_prove_replacement_epoch() {
    let mut coordinator = LiveTailRefreshCoordinator::new();

    assert_eq!(
        coordinator.activate(A, 7),
        vec![LiveTailSchedulerAction::Start {
            key: A,
            epoch: 7,
            operation_generation: 1,
            limit: 128,
        }]
    );
    assert_eq!(
        coordinator.invalidate_epoch(A, 8),
        vec![
            LiveTailSchedulerAction::CancelNetwork {
                key: A,
                operation_generation: 1,
            },
            LiveTailSchedulerAction::Start {
                key: A,
                epoch: 8,
                operation_generation: 2,
                limit: 128,
            },
        ]
    );
    assert_eq!(
        coordinator.finish(A, 7, 1, LiveTailRefreshOutcome::Unchanged),
        Vec::new()
    );
    assert_eq!(coordinator.mark_unproven(A, 8), Vec::new());
    assert_eq!(
        coordinator.finish(A, 8, 2, LiveTailRefreshOutcome::Unchanged),
        Vec::new()
    );
    assert_eq!(coordinator.mark_unproven(A, 8), Vec::new());
    assert_eq!(
        coordinator.freshness(&A),
        Some(LiveTailFreshnessState::Fresh { epoch: 8 })
    );
}

#[test]
fn inactive_epoch_replacement_fences_old_finish_and_preserves_one_deferred_entry() {
    let mut coordinator = LiveTailRefreshCoordinator::new();

    assert_eq!(
        coordinator.activate(A, 7),
        vec![LiveTailSchedulerAction::Start {
            key: A,
            epoch: 7,
            operation_generation: 1,
            limit: 128,
        }]
    );
    assert_eq!(coordinator.mark_unproven(B, 8), Vec::new());
    assert_eq!(
        coordinator.finish(A, 7, 1, LiveTailRefreshOutcome::Unchanged),
        vec![LiveTailSchedulerAction::Start {
            key: B,
            epoch: 8,
            operation_generation: 2,
            limit: 128,
        }]
    );

    assert_eq!(
        coordinator.mark_unproven(B, 9),
        vec![LiveTailSchedulerAction::CancelNetwork {
            key: B,
            operation_generation: 2,
        }]
    );
    assert_eq!(
        coordinator.freshness(&B),
        Some(LiveTailFreshnessState::Deferred { epoch: 9 })
    );
    assert_eq!(
        coordinator.delayed.iter().copied().collect::<Vec<_>>(),
        vec![B]
    );
    assert_eq!(coordinator.delayed_members.len(), 1);
    assert!(coordinator.delayed_members.contains(&B));

    assert_eq!(
        coordinator.finish(B, 8, 2, LiveTailRefreshOutcome::Unchanged),
        Vec::new()
    );
    assert_eq!(
        coordinator.freshness(&B),
        Some(LiveTailFreshnessState::Deferred { epoch: 9 })
    );
    assert_eq!(
        coordinator.delayed.iter().copied().collect::<Vec<_>>(),
        vec![B]
    );

    assert_eq!(
        coordinator.activate(C, 10),
        vec![LiveTailSchedulerAction::Start {
            key: C,
            epoch: 10,
            operation_generation: 3,
            limit: 128,
        }]
    );
    assert_eq!(
        coordinator.finish(C, 10, 3, LiveTailRefreshOutcome::Unchanged),
        vec![LiveTailSchedulerAction::Start {
            key: B,
            epoch: 9,
            operation_generation: 4,
            limit: 128,
        }]
    );
    assert_eq!(
        coordinator.finish(B, 9, 4, LiveTailRefreshOutcome::Unchanged),
        Vec::new()
    );
}

#[test]
fn failed_active_refresh_is_retryable_without_busy_loop() {
    let mut coordinator = LiveTailRefreshCoordinator::new();

    assert_eq!(
        coordinator.activate(A, 7),
        vec![LiveTailSchedulerAction::Start {
            key: A,
            epoch: 7,
            operation_generation: 1,
            limit: 128,
        }]
    );
    assert_eq!(coordinator.mark_unproven(B, 8), Vec::new());
    assert_eq!(
        coordinator.finish(A, 7, 1, LiveTailRefreshOutcome::Failed),
        vec![LiveTailSchedulerAction::Start {
            key: B,
            epoch: 8,
            operation_generation: 2,
            limit: 128,
        }]
    );
    assert_eq!(
        coordinator.freshness(&A),
        Some(LiveTailFreshnessState::Retryable { epoch: 7 })
    );
    assert_eq!(
        coordinator.finish(B, 8, 2, LiveTailRefreshOutcome::Failed),
        Vec::new()
    );
    assert_eq!(
        coordinator.mark_unproven(A, 7),
        vec![LiveTailSchedulerAction::Start {
            key: A,
            epoch: 7,
            operation_generation: 3,
            limit: 128,
        }]
    );
}

#[test]
fn exhausted_operation_serial_never_wraps_or_starts_a_retry_loop() {
    let room = "!a:test";
    let mut coordinator = LiveTailRefreshCoordinator::new();
    coordinator.operation_generation = crate::causal_projection::CAUSAL_PROJECTION_SERIAL_MAX;

    assert!(coordinator.activate(room, 7).is_empty());
    assert_eq!(
        coordinator.freshness(&room),
        Some(LiveTailFreshnessState::Retryable { epoch: 7 }),
    );
    assert!(coordinator.running.is_none());
    assert_eq!(
        coordinator.operation_generation,
        crate::causal_projection::CAUSAL_PROJECTION_SERIAL_MAX,
    );

    assert!(coordinator.activate(room, 7).is_empty());
    assert!(coordinator.mark_unproven(room, 7).is_empty());
    assert!(coordinator.running.is_none());
    assert_eq!(
        coordinator.operation_generation,
        crate::causal_projection::CAUSAL_PROJECTION_SERIAL_MAX,
        "same-generation retries must not reuse serial one",
    );
}

#[test]
fn delayed_rooms_run_one_at_a_time_in_fifo_order() {
    let mut coordinator = LiveTailRefreshCoordinator::new();

    assert_eq!(
        coordinator.activate(A, 7),
        vec![LiveTailSchedulerAction::Start {
            key: A,
            epoch: 7,
            operation_generation: 1,
            limit: 128,
        }]
    );
    assert_eq!(coordinator.mark_unproven(B, 8), Vec::new());
    assert_eq!(coordinator.mark_unproven(C, 9), Vec::new());
    assert_eq!(coordinator.mark_unproven(B, 8), Vec::new());

    assert_eq!(
        coordinator.finish(A, 7, 1, LiveTailRefreshOutcome::Advanced { events: 3 }),
        vec![LiveTailSchedulerAction::Start {
            key: B,
            epoch: 8,
            operation_generation: 2,
            limit: 128,
        }]
    );
    assert_eq!(
        coordinator.finish(
            B,
            8,
            2,
            LiveTailRefreshOutcome::Detached {
                events: 5,
                historical_gap_remaining: true,
            },
        ),
        vec![LiveTailSchedulerAction::Start {
            key: C,
            epoch: 9,
            operation_generation: 3,
            limit: 128,
        }]
    );
    assert_eq!(
        coordinator.finish(C, 9, 3, LiveTailRefreshOutcome::Unchanged),
        Vec::new()
    );
}
