use super::fixture::{Fixture, LIVENESS};
use crate::timeline::{
    mutation::{MUTATION_CAPACITY, MUTATION_QUEUE_CAPACITY, Mutation, MutationLane},
    test_support::fake_rid,
};
use koushi_protocol::{
    event::CoreEvent,
    failure::{CoreFailure, TimelineFailureKind},
};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};

fn operation(
    target: &str,
    index: u64,
    entered: mpsc::UnboundedSender<(u64, oneshot::Sender<Result<(), TimelineFailureKind>>)>,
) -> Mutation {
    Mutation::Synthetic {
        event_id: target.into(),
        future: Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            entered.send((index, tx)).unwrap();
            rx.await.unwrap_or(Err(TimelineFailureKind::Sdk))
        }),
    }
}
fn failure(event: CoreEvent) -> (u64, TimelineFailureKind) {
    match event {
        CoreEvent::OperationFailed {
            request_id,
            failure: CoreFailure::TimelineOperationFailed { kind },
        } => (request_id.sequence, kind),
        _ => panic!("expected correlated timeline failure"),
    }
}
async fn receive<T>(rx: &mut mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(LIVENESS, rx.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn fifo_preserves_target_order_while_other_targets_progress() {
    let fixture = Fixture::new(true).await;
    let mut lane = MutationLane::default();
    let (events, mut rx) = broadcast::channel(16);
    let (tx, mut entered) = mpsc::unbounded_channel();
    lane.admit(
        fixture.context.clone(),
        fake_rid(20),
        operation("target", 20, tx.clone()),
        events.clone(),
    );
    let (first, release) = receive(&mut entered).await;
    assert_eq!(first, 20);
    lane.admit(
        fixture.context.clone(),
        fake_rid(21),
        operation("target", 21, tx.clone()),
        events.clone(),
    );
    lane.admit(
        fixture.context.clone(),
        fake_rid(22),
        operation("other", 22, tx.clone()),
        events,
    );
    let (independent, other) = receive(&mut entered).await;
    assert_eq!(independent, 22);
    assert!(
        entered.try_recv().is_err(),
        "same-target successor must stay queued"
    );
    other.send(Err(TimelineFailureKind::Sdk)).unwrap();
    assert_eq!(
        failure(rx.recv().await.unwrap()),
        (22, TimelineFailureKind::Sdk)
    );
    release.send(Ok(())).unwrap();
    let (second, release) = receive(&mut entered).await;
    assert_eq!(second, 21);
    release
        .send(Err(TimelineFailureKind::InvalidReactionState))
        .unwrap();
    assert_eq!(
        failure(rx.recv().await.unwrap()),
        (21, TimelineFailureKind::InvalidReactionState)
    );
    fixture.stop().await;
    lane.stop().await;
    assert!(
        rx.try_recv().is_err(),
        "success and settled failures cannot fail again at teardown"
    );
}

#[tokio::test]
async fn expiry_settles_queued_requests_without_abandoning_active_execution() {
    let fixture = Fixture::new(true).await;
    let mut lane = MutationLane::default();
    let (events, mut rx) = broadcast::channel(16);
    let (tx, mut entered) = mpsc::unbounded_channel();
    lane.admit(
        fixture.context.clone(),
        fake_rid(30),
        operation("target", 30, tx.clone()),
        events.clone(),
    );
    let (_, release) = receive(&mut entered).await;
    lane.admit(
        fixture.context.clone(),
        fake_rid(31),
        operation("target", 31, tx),
        events,
    );
    // Virtual time advances the admission deadline; no fixed sleep waits for SDK effects.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    let mut failures = vec![
        failure(rx.recv().await.unwrap()),
        failure(rx.recv().await.unwrap()),
    ];
    failures.sort_by_key(|pair| pair.0);
    assert_eq!(
        failures,
        vec![
            (30, TimelineFailureKind::Sdk),
            (31, TimelineFailureKind::Sdk)
        ]
    );
    assert!(
        !release.is_closed(),
        "timed-out active future stays alive for SDK rollback"
    );
    assert!(
        entered.try_recv().is_err(),
        "expired queued operation never runs"
    );
    release.send(Err(TimelineFailureKind::Sdk)).unwrap();
    tokio::time::resume();
    fixture.stop().await;
    lane.stop().await;
    assert!(
        rx.try_recv().is_err(),
        "late SDK failure cannot emit a duplicate terminal"
    );
}

#[tokio::test]
async fn global_capacity_counts_ingress_and_drop_settles_every_accepted_request() {
    let fixture = Fixture::new(true).await;
    let mut lane = MutationLane::default();
    let (events, mut rx) = broadcast::channel(128);
    let (tx, _entered) = mpsc::unbounded_channel();
    for index in 0..MUTATION_CAPACITY as u64 {
        lane.admit(
            fixture.context.clone(),
            fake_rid(100 + index),
            operation(&format!("target-{index}"), index, tx.clone()),
            events.clone(),
        );
    }
    lane.admit(
        fixture.context.clone(),
        fake_rid(999),
        operation("overflow", 999, tx),
        events,
    );
    assert_eq!(
        failure(rx.recv().await.unwrap()),
        (999, TimelineFailureKind::QueueOverflow)
    );
    // Retire presentation before orderly cancellation, as in production shutdown.
    fixture.stop().await;
    lane.stop().await;
    let mut accepted = Vec::new();
    while let Ok(event) = rx.try_recv() {
        accepted.push(failure(event).0);
    }
    accepted.sort();
    accepted.dedup();
    assert_eq!(accepted.len(), MUTATION_CAPACITY);
}

#[tokio::test]
async fn per_target_capacity_and_replaced_generation_fail_explicitly() {
    let fixture = Fixture::new(true).await;
    let mut lane = MutationLane::default();
    let (events, mut rx) = broadcast::channel(32);
    let (tx, mut entered) = mpsc::unbounded_channel();
    lane.admit(
        fixture.context.clone(),
        fake_rid(40),
        operation("target", 40, tx.clone()),
        events.clone(),
    );
    let (_, active) = receive(&mut entered).await;
    for index in 0..=MUTATION_QUEUE_CAPACITY as u64 {
        lane.admit(
            fixture.context.clone(),
            fake_rid(41 + index),
            operation("target", 41 + index, tx.clone()),
            events.clone(),
        );
    }
    assert_eq!(
        failure(
            tokio::time::timeout(LIVENESS, rx.recv())
                .await
                .unwrap()
                .unwrap()
        )
        .1,
        TimelineFailureKind::QueueOverflow
    );
    fixture
        .context
        .gate
        .invalidate_and_quiesce(&fixture.key)
        .await;
    active.send(Ok(())).unwrap();
    for _ in 0..MUTATION_QUEUE_CAPACITY {
        assert_eq!(
            failure(
                tokio::time::timeout(LIVENESS, rx.recv())
                    .await
                    .unwrap()
                    .unwrap()
            )
            .1,
            TimelineFailureKind::NotSubscribed
        );
    }
    assert!(
        entered.try_recv().is_err(),
        "queued work cannot execute on a retired controller"
    );
    fixture.stop().await;
    lane.stop().await;
}

#[tokio::test]
async fn routing_retained_controller_after_generation_activation_refuses_sdk_work() {
    use super::fixture::{REACTION, TARGET};
    use koushi_protocol::command::TimelineCommand;
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path_regex},
    };
    let mut fixture = Fixture::new(false).await;
    Mock::given(method("PUT"))
        .and(path_regex("/redact/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"event_id":"$redacted:example.invalid"})),
        )
        .expect(0)
        .mount(fixture.server.server())
        .await;
    // Replacement activates before its async focused build installs a new
    // handle. Retain the old handle exactly as the manager does in that gap.
    let successor = fixture
        .context
        .gate
        .activate_after_quiescence(&fixture.key)
        .await;
    assert_ne!(successor.generation, fixture.generation);
    fixture
        .command(TimelineCommand::RedactReaction {
            request_id: fake_rid(600),
            key: fixture.key.clone(),
            event_id: TARGET.into(),
            reaction_key: "👍".into(),
            reaction_event_id: REACTION.into(),
        })
        .await;
    fixture.wait_for(|event| matches!(event,
        CoreEvent::OperationFailed { request_id, failure: CoreFailure::TimelineOperationFailed { kind: TimelineFailureKind::NotSubscribed } }
        if *request_id == fake_rid(600)
    )).await;
    fixture.stop().await;
}
