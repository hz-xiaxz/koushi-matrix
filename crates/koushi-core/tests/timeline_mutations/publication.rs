use super::fixture::{Fixture, PENDING, is_pending};
use crate::timeline::{
    outbound_send::{
        SendCompletionRegistration, SharedSendCompletionCoordinator, TimelineSendTerminalIngress,
    },
    test_support::fake_rid,
};
use koushi_diagnostics::{DiagnosticValue, test_support};

fn stages(correlation: u64) -> Vec<&'static str> {
    test_support::detail_snapshot()
        .records
        .into_iter()
        .filter(|record| {
            record.event.source == "core.send"
                && record.event.fields.iter().any(|field| {
                    field.key == "correlation"
                        && field.value == DiagnosticValue::Correlation(correlation)
                })
        })
        .map(|record| record.event.stage)
        .collect()
}

#[tokio::test]
async fn insertion_and_successful_actor_publication_have_one_opaque_correlation() {
    let _diagnostics = test_support::lock_async().await;
    let mut fixture = Fixture::new(true).await;
    let coordinator = SharedSendCompletionCoordinator::default();
    let (ingress, _terminals) = TimelineSendTerminalIngress::channel();
    let mut registration = SendCompletionRegistration::begin_with_projection(
        coordinator.clone(),
        ingress,
        fixture.key.clone(),
        PENDING.into(),
        None,
        fake_rid(500),
        true,
        Some(fixture.projection()),
    );
    registration.activate();
    let projections = coordinator
        .lock()
        .unwrap()
        .projections_for_key(&fixture.key);
    let correlation = projections[0]
        .lifecycle_trace
        .as_ref()
        .unwrap()
        .correlation();
    assert!(stages(correlation).contains(&"pending_projection_inserted"));
    assert!(
        !stages(correlation).contains(&"pending_projection_published"),
        "coordinator insertion does not mean actor publication"
    );
    assert!(fixture.publish(projections.clone()).await);
    fixture.wait_for(is_pending).await;
    assert_eq!(
        stages(correlation)
            .iter()
            .filter(|&&stage| stage == "pending_projection_published")
            .count(),
        1
    );
    assert!(fixture.publish(projections.clone()).await);
    fixture
        .context
        .gate
        .invalidate_and_quiesce(&fixture.key)
        .await;
    assert!(
        !fixture.publish(projections).await,
        "retired generation must not publish"
    );
    assert_eq!(
        stages(correlation)
            .iter()
            .filter(|&&stage| stage == "pending_projection_published")
            .count(),
        1,
        "duplicates and stale refreshes do not claim another publication"
    );
    assert!(
        !stages(correlation)
            .iter()
            .any(|stage| stage.contains("renderer")),
        "actor broadcast is not renderer receipt"
    );
    fixture.stop().await;
}

#[tokio::test]
async fn absent_receiver_does_not_consume_publication_before_replay() {
    let _diagnostics = test_support::lock_async().await;
    let mut fixture = Fixture::new(true).await;
    let coordinator = SharedSendCompletionCoordinator::default();
    let (ingress, _terminals) = TimelineSendTerminalIngress::channel();
    let mut registration = SendCompletionRegistration::begin_with_projection(
        coordinator.clone(),
        ingress,
        fixture.key.clone(),
        PENDING.into(),
        None,
        fake_rid(501),
        true,
        Some(fixture.projection()),
    );
    registration.activate();
    let projections = coordinator
        .lock()
        .unwrap()
        .projections_for_key(&fixture.key);
    let correlation = projections[0]
        .lifecycle_trace
        .as_ref()
        .unwrap()
        .correlation();
    let (_, placeholder) = tokio::sync::broadcast::channel(1);
    drop(std::mem::replace(&mut fixture.events, placeholder));
    assert_eq!(fixture.event_tx.receiver_count(), 0);
    assert!(
        fixture.publish(projections).await,
        "generation still advances without receivers"
    );
    assert!(!stages(correlation).contains(&"pending_projection_published"));
    fixture.events = fixture.event_tx.subscribe();
    fixture
        .actor
        .send(
            crate::timeline::actor::TimelineActorMessage::ReplayInitialItems {
                cause_request_id: None,
            },
        )
        .await
        .unwrap();
    fixture.wait_for(|event| matches!(event,
        koushi_protocol::event::CoreEvent::Timeline(koushi_protocol::event::TimelineEvent::InitialItems { items, .. })
        if items.iter().any(|item| matches!(&item.id, koushi_protocol::event::TimelineItemId::Transaction { transaction_id } if transaction_id == PENDING))
    )).await;
    assert_eq!(
        stages(correlation)
            .iter()
            .filter(|&&stage| stage == "pending_projection_published")
            .count(),
        1
    );
    fixture.stop().await;
}
