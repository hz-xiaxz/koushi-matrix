use super::{
    super::{
        diagnostics::{trace_timeline_actor_operation, trace_timeline_actor_scan},
        item_projection::{
            classify_reaction_error, edit_target_msgtype, edited_document_content_for_edit_target,
            mention_counts_for_edit, mention_summary_for_message_type,
            msgtype_carries_editable_caption, sdk_item_to_timeline_item,
            trace_message_edit_lifecycle, trace_message_edit_target, validate_redact_reaction,
            validate_send_reaction,
        },
        navigation::TimelineActorGenerationGate,
    },
    Mutation,
};
use crate::account_work::{AccountWorkKind, AccountWorkScheduler};
use koushi_protocol::{
    failure::TimelineFailureKind,
    ids::{RequestId, TimelineKey},
};
use matrix_sdk_ui::timeline::{Timeline, TimelineEventItemId, TimelineItemKind};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

pub(in crate::timeline) type SentEventTransactions =
    Arc<Mutex<HashMap<String, matrix_sdk::ruma::OwnedTransactionId>>>;
#[derive(Clone)]
pub(in crate::timeline) struct MutationContext {
    pub key: TimelineKey,
    pub generation: u64,
    pub gate: Arc<TimelineActorGenerationGate>,
    pub timeline: Arc<Timeline>,
    pub account_work: AccountWorkScheduler,
    pub sent_event_txns: SentEventTransactions,
}
impl MutationContext {
    pub fn is_current(&self) -> bool {
        self.gate.try_acquire(&self.key, self.generation).is_some()
    }
    pub async fn execute(
        &self,
        request_id: RequestId,
        mutation: Mutation,
    ) -> Result<(), TimelineFailureKind> {
        if !self.is_current() {
            return Err(TimelineFailureKind::NotSubscribed);
        }
        match mutation {
            Mutation::Edit {
                event_id,
                document,
                formatting,
            } => {
                let candidates = self.item_ids(&event_id);
                if candidates.is_empty() {
                    return Err(TimelineFailureKind::Sdk);
                }
                let items = self.timeline.items().await;
                for item_id in candidates {
                    let target = edit_target_msgtype(&items, &item_id);
                    let target_kind = if target.is_some_and(msgtype_carries_editable_caption) {
                        "media_caption"
                    } else {
                        "text"
                    };
                    let original = mention_summary_for_message_type(target);
                    let original_count = original.0.len() + usize::from(original.1);
                    trace_message_edit_lifecycle(
                        "opened",
                        target_kind,
                        original_count,
                        0,
                        None,
                        None,
                    );
                    let content =
                        edited_document_content_for_edit_target(target, &document, formatting);
                    trace_message_edit_target(target, &content);
                    let (final_count, revision_count) = mention_counts_for_edit(target, &content);
                    trace_message_edit_lifecycle(
                        "submitted",
                        target_kind,
                        original_count,
                        final_count,
                        Some(revision_count),
                        None,
                    );
                    let result = self.timeline.edit(&item_id, content).await;
                    if matches!(
                        result,
                        Err(matrix_sdk_ui::timeline::Error::EventNotInTimeline(_))
                    ) {
                        continue;
                    }
                    trace_message_edit_lifecycle(
                        "settled",
                        target_kind,
                        original_count,
                        final_count,
                        Some(revision_count),
                        Some(if result.is_ok() { "success" } else { "failed" }),
                    );
                    return result.map_err(|_| TimelineFailureKind::Sdk);
                }
                Err(TimelineFailureKind::Sdk)
            }
            Mutation::Redact { event_id } => {
                let _interactive = self
                    .account_work
                    .begin_interactive(AccountWorkKind::MessageSend);
                for item_id in self.item_ids(&event_id) {
                    match self.timeline.redact(&item_id, None).await {
                        Err(matrix_sdk_ui::timeline::Error::EventNotInTimeline(_)) => continue,
                        result => return result.map_err(|_| TimelineFailureKind::Sdk),
                    }
                }
                Err(TimelineFailureKind::Sdk)
            }
            Mutation::AddReaction {
                event_id,
                reaction_key,
            } => {
                self.reaction(request_id, &event_id, &reaction_key, None)
                    .await
            }
            Mutation::RemoveReaction {
                event_id,
                reaction_key,
                reaction_event_id,
            } => {
                self.reaction(
                    request_id,
                    &event_id,
                    &reaction_key,
                    Some(&reaction_event_id),
                )
                .await
            }
            #[cfg(test)]
            Mutation::Synthetic { future, .. } => future.await,
        }
    }
    fn item_ids(&self, event_id: &str) -> Vec<TimelineEventItemId> {
        let mut ids = Vec::with_capacity(2);
        if let Ok(event) = matrix_sdk::ruma::EventId::parse(event_id) {
            ids.push(TimelineEventItemId::EventId(event));
        }
        if let Some(txn) = self
            .sent_event_txns
            .lock()
            .expect("sent event transactions")
            .get(event_id)
        {
            ids.push(TimelineEventItemId::TransactionId(txn.clone()));
        }
        ids
    }
    async fn reaction(
        &self,
        request_id: RequestId,
        event_id: &str,
        key: &str,
        removal: Option<&str>,
    ) -> Result<(), TimelineFailureKind> {
        let started = Instant::now();
        let kind = if removal.is_some() {
            "redact_reaction"
        } else {
            "send_reaction"
        };
        trace_timeline_actor_operation("actor_start", kind, request_id, &self.key, None, None);
        let result = Box::pin(self.reaction_inner(request_id, event_id, key, removal)).await;
        trace_timeline_actor_operation(
            "actor_finish",
            kind,
            request_id,
            &self.key,
            Some(started.elapsed().as_millis()),
            Some(if result.is_ok() { "success" } else { "failed" }),
        );
        result
    }
    async fn reaction_inner(
        &self,
        request_id: RequestId,
        event_id: &str,
        key: &str,
        removal: Option<&str>,
    ) -> Result<(), TimelineFailureKind> {
        let started = Instant::now();
        let kind = if removal.is_some() {
            "redact_reaction"
        } else {
            "send_reaction"
        };
        if key.trim().is_empty() || removal.is_some_and(|id| id.trim().is_empty()) {
            return Err(TimelineFailureKind::InvalidReactionTarget);
        }
        let event_id = matrix_sdk::ruma::EventId::parse(event_id)
            .map_err(|_| TimelineFailureKind::InvalidReactionTarget)?;
        let items = self.timeline.items().await;
        let target = items.iter().rev().find(|item| matches!(item.kind(), TimelineItemKind::Event(event) if event.event_id() == Some(event_id.as_ref())));
        trace_timeline_actor_scan(
            "target_scan",
            kind,
            request_id,
            &self.key,
            items.len(),
            started.elapsed().as_millis(),
            target.is_some(),
        );
        let target = target.ok_or(TimelineFailureKind::InvalidReactionTarget)?;
        let projected =
            sdk_item_to_timeline_item(&self.key, target, Some(self.timeline.room().own_user_id()));
        let reaction = projected
            .reactions
            .iter()
            .find(|reaction| reaction.key == key);
        let own_id = reaction.and_then(|reaction| reaction.my_reaction_event_id.as_deref());
        match removal {
            Some(removal) => validate_redact_reaction(projected.can_react, own_id, removal)?,
            None => {
                validate_send_reaction(projected.can_react, own_id)?;
                if reaction.is_some_and(|reaction| reaction.reacted_by_me) {
                    return Err(TimelineFailureKind::InvalidReactionState);
                }
            }
        }
        let sdk_started = Instant::now();
        let result = match self
            .timeline
            .toggle_reaction(&TimelineEventItemId::EventId(event_id), key)
            .await
        {
            Ok(added) if added == removal.is_none() => Ok(()),
            Ok(_) => Err(TimelineFailureKind::InvalidReactionState),
            Err(error) => Err(classify_reaction_error(&error)),
        };
        trace_timeline_actor_operation(
            "sdk_done",
            kind,
            request_id,
            &self.key,
            Some(sdk_started.elapsed().as_millis()),
            Some(if result.is_ok() { "success" } else { "failed" }),
        );
        result
    }
}
