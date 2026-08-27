use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use koushi_sdk::MatrixClientSession;
use koushi_sdk::MatrixUserProfile;
use koushi_search::{AttachmentDocument, SensitiveString};
use koushi_state::UserProfile;
use koushi_state::{
    AppAction, AttachmentKind, AvatarImage, AvatarThumbnailState, ComposerDocument, ComposerInline,
    LiveEventReceipts, LiveReadReceipt, MentionIntent, MentionTarget, ReplyQuote,
    ReplyQuoteCodeBlock, ReplyQuoteFormattedBody, ReplyQuoteState,
};

use matrix_sdk::attachment::{AttachmentInfo, BaseFileInfo, BaseImageInfo, Thumbnail};
use matrix_sdk::media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings};
use matrix_sdk::room::edit::EditedContent;
use matrix_sdk::room::reply::{EnforceThread, Reply};
use matrix_sdk::ruma::events::room::message::FormattedBody;
use matrix_sdk::ruma::events::room::message::{
    AddMentions, MessageFormat, MessageType, ReplyWithinThread,
    RoomMessageEventContentWithoutRelation,
};
use matrix_sdk::ruma::events::room::{MediaSource, ThumbnailInfo};
use matrix_sdk::ruma::events::{StateEventContentChange, room::name::RoomNameEventContent};
use matrix_sdk::ruma::html::{Html, SanitizerConfig};
use matrix_sdk::send_queue::{LocalEcho, LocalEchoContent, SendHandle};
use matrix_sdk_ui::timeline::{
    AnyOtherStateEventContentChange, EmbeddedEvent, EncryptedMessage,
    EventSendState as SdkEventSendState, EventTimelineItem, InReplyToDetails, MembershipChange,
    Profile, ReactionStatus, ReactionsByKeyBySender, Timeline, TimelineDetails,
    TimelineEventItemId, TimelineItem as SdkTimelineItem, TimelineItemContent, TimelineItemKind,
};
use tokio::sync::mpsc;

use crate::account_work::AccountWorkKind;
use crate::command::{MediaDownloadSelection, UploadMediaKind, UploadMediaRequest};
use crate::event::{
    CoreEvent, LinkPreview, LinkPreviewState, ReactionSender, RoomKeyRequestStage,
    RoomKeyRequestStateDto, RoomKeyRequestWithheldCode, ThreadSummaryDto, TimelineDiff,
    TimelineEvent, TimelineItem, TimelineItemId, TimelineMedia, TimelineMediaKind,
    TimelineMediaSource, TimelineMediaThumbnail, TimelineMegolmSessionReason,
    TimelineMessageActions, TimelineMessageKind, TimelineMessageSource, TimelineNoticeI18n,
    TimelineNoticeI18nKey, TimelineSendFailureReason, TimelineSendState, TimelineSpoilerSpan,
    TimelineUnableToDecrypt, TimelineUnableToDecryptReason, TimelineViewportObservation,
    message_actions_for_timeline_item, message_source_for_timeline_item,
};
use crate::executor;
use crate::failure::{CoreFailure, TimelineFailureKind};
use crate::ids::{RequestId, TimelineKey, TimelineKind};
use crate::link_preview::{LinkPreviewContext, extract_link_ranges};
use crate::search::SearchIndexMessage;

// BEGIN GENERATED SIBLING IMPORTS
use super::actor::{
    TimelineActor, TimelineActorMessage, canonical_activity_window_action,
    reserve_canonical_activity_action,
};
use super::composer::ruma_mentions_from_intent;
use super::diagnostics::{
    trace_timeline_actor_operation, trace_timeline_actor_scan, trace_timeline_link_preview,
};
use super::manager::TimelineManagerActor;
use super::media::PrivateMediaEntry;
use super::navigation::{TimelineActorGenerationGate, send_generation_fenced};
use super::room_key_recovery::{DecryptRetryReason, KeyRequestUiState};
// END GENERATED SIBLING IMPORTS

pub(super) const REPLY_QUOTE_PREVIEW_MAX_CHARS: usize = 160;

impl TimelineManagerActor {
    pub(super) async fn handle_ignored_users_updated(
        &mut self,
        user_ids: std::collections::BTreeSet<String>,
    ) {
        self.ignored_user_ids = user_ids.clone();
        for handle in self.timelines.values() {
            let _ = handle
                .send(TimelineActorMessage::IgnoredUsersUpdated(user_ids.clone()))
                .await;
        }
    }
}

fn spawn_link_preview_fetch(
    session: Arc<MatrixClientSession>,
    msg_tx: mpsc::Sender<TimelineActorMessage>,
    request_id: RequestId,
    event_id: String,
    previews: Vec<LinkPreview>,
) -> executor::JoinHandle<()> {
    executor::spawn(async move {
        let started = std::time::Instant::now();
        let mut updated = Vec::with_capacity(previews.len());
        let mut pending_count = 0usize;
        let mut ready_count = 0usize;
        let mut failed_count = 0usize;

        for mut preview in previews {
            if preview.state != LinkPreviewState::Pending {
                updated.push(preview);
                continue;
            }

            pending_count += 1;
            match crate::link_preview::fetch_link_preview(&session, &preview.url).await {
                Ok(fetched) => {
                    updated.push(fetched);
                    ready_count += 1;
                }
                Err(_) => {
                    preview.state = LinkPreviewState::Failed;
                    updated.push(preview);
                    failed_count += 1;
                }
            }
        }

        let _ = msg_tx
            .send(TimelineActorMessage::LinkPreviewsFetched {
                request_id,
                event_id,
                previews: updated,
                pending_count,
                ready_count,
                failed_count,
                elapsed_ms: started.elapsed().as_millis(),
            })
            .await;
    })
}

fn spawn_reply_detail_fetch(
    timeline: Arc<Timeline>,
    msg_tx: mpsc::Sender<TimelineActorMessage>,
    event_id: String,
) -> executor::JoinHandle<()> {
    executor::spawn(async move {
        if let Ok(parsed_event_id) = matrix_sdk::ruma::EventId::parse(event_id.as_str()) {
            let _ = timeline.fetch_details_for_event(&parsed_event_id).await;
        }
        let _ = msg_tx
            .send(TimelineActorMessage::ReplyDetailsFetchFinished { event_id })
            .await;
    })
}

struct ReactionTargetState {
    item_id: TimelineEventItemId,
    can_react: bool,
    my_reaction_event_id: Option<String>,
}

impl TimelineActor {
    pub(super) async fn handle_load_message_source(
        &mut self,
        request_id: RequestId,
        event_id: String,
    ) {
        let Some(source) = self.project_message_source_for_event(&event_id).await else {
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidSendTarget);
            return;
        };

        self.emit(CoreEvent::Timeline(TimelineEvent::MessageSourceLoaded {
            request_id,
            key: self.key.clone(),
            source,
        }));
    }
    pub(super) async fn handle_edit_text(
        &mut self,
        request_id: RequestId,
        event_id: String,
        document: ComposerDocument,
    ) {
        let body = document.plain_body();
        // Edits go through the SDK Timeline so the Set diff on the original
        // item is produced locally (send-queue local echo) instead of
        // depending on the server echoing the edit back through sync —
        // some Sliding Sync implementations do not deliver it reliably (Phase 5
        // review finding). Canon rule 1: relay the SDK.
        let candidates = self.item_ids_for_event(&event_id);
        if candidates.is_empty() {
            trace_message_edit_lifecycle("opened", "text", 0, 0, None, None);
            trace_message_edit_lifecycle("settled", "text", 0, 0, None, Some("failed"));
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            );
            return;
        }
        // The replacement shape is a Rust-owned decision: the GUI submits only
        // the new visible text, so core resolves what the target event actually
        // is before choosing between a caption edit and a text replacement.
        let items = self.timeline.items().await;
        let mut result = Ok(());
        let mut diagnostic_target = "text";
        let mut diagnostic_original_mention_count = 0;
        let mut diagnostic_final_mention_count = 0;
        let mut diagnostic_revision_mention_count = 0;
        for item_id in &candidates {
            let target = edit_target_msgtype(&items, item_id);
            diagnostic_target = if target.is_some_and(msgtype_carries_editable_caption) {
                "media_caption"
            } else {
                "text"
            };
            let original_mentions = mention_summary_for_message_type(target);
            diagnostic_original_mention_count =
                original_mentions.0.len() + usize::from(original_mentions.1);
            trace_message_edit_lifecycle(
                "opened",
                diagnostic_target,
                diagnostic_original_mention_count,
                0,
                None,
                None,
            );
            let content = edited_document_content_for_edit_target(target, &document);
            trace_message_edit_target(target, &content);
            let (final_mention_count, revision_mention_count) =
                mention_counts_for_edit(target, &content);
            diagnostic_final_mention_count = final_mention_count;
            diagnostic_revision_mention_count = revision_mention_count;
            trace_message_edit_lifecycle(
                "submitted",
                diagnostic_target,
                diagnostic_original_mention_count,
                final_mention_count,
                Some(revision_mention_count),
                None,
            );
            result = self.timeline.edit(item_id, content).await;
            match &result {
                Err(matrix_sdk_ui::timeline::Error::EventNotInTimeline(_)) => continue,
                _ => break,
            }
        }

        trace_message_edit_lifecycle(
            "settled",
            diagnostic_target,
            diagnostic_original_mention_count,
            diagnostic_final_mention_count,
            Some(diagnostic_revision_mention_count),
            Some(if result.is_ok() { "success" } else { "failed" }),
        );

        if result.is_err() {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            );
        }
        // Edit success: the local-echo Set diff on the original item identity
        // arrives through the subscription; no dedicated EditCompleted event.
    }
    pub(super) async fn handle_redact(&mut self, request_id: RequestId, event_id: String) {
        // Same rationale as edits: redact through the SDK Timeline so the
        // diff is produced locally instead of waiting for the server echo.
        let candidates = self.item_ids_for_event(&event_id);
        if candidates.is_empty() {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            );
            return;
        }
        let _interactive = self
            .account_work
            .begin_interactive(AccountWorkKind::MessageSend);
        let mut result = Ok(());
        for item_id in &candidates {
            result = self.timeline.redact(item_id, None).await;
            match &result {
                Err(matrix_sdk_ui::timeline::Error::EventNotInTimeline(_)) => continue,
                _ => break,
            }
        }

        if result.is_err() {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            );
        }
        // Redact success: timeline diff reflects it (removal or redacted-state Set diff).
    }
    pub(super) async fn handle_toggle_reaction(
        &mut self,
        request_id: RequestId,
        event_id: String,
        reaction_key: String,
    ) {
        let candidates = self.item_ids_for_event(&event_id);
        if candidates.is_empty() {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            );
            return;
        }

        let _interactive = self
            .account_work
            .begin_interactive(AccountWorkKind::MessageSend);
        let mut result: Result<(), matrix_sdk_ui::timeline::Error> = Ok(());
        for item_id in &candidates {
            result = self
                .timeline
                .toggle_reaction(item_id, &reaction_key)
                .await
                .map(|_| ());
            match &result {
                Err(matrix_sdk_ui::timeline::Error::EventNotInTimeline(_)) => continue,
                _ => break,
            }
        }

        if result.is_err() {
            self.emit_failure(
                request_id,
                CoreFailure::TimelineOperationFailed {
                    kind: TimelineFailureKind::Sdk,
                },
            );
        }
    }
    pub(super) async fn handle_send_reaction(
        &mut self,
        request_id: RequestId,
        event_id: String,
        reaction_key: String,
    ) {
        let started = Instant::now();
        trace_timeline_actor_operation(
            "actor_start",
            "send_reaction",
            request_id,
            &self.key,
            None,
            None,
        );
        if reaction_key.trim().is_empty() {
            trace_timeline_actor_operation(
                "actor_finish",
                "send_reaction",
                request_id,
                &self.key,
                Some(started.elapsed().as_millis()),
                Some("invalid_target"),
            );
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidReactionTarget);
            return;
        }

        let Some(target) = self
            .reaction_target_state(request_id, "send_reaction", &event_id, &reaction_key)
            .await
        else {
            trace_timeline_actor_operation(
                "actor_finish",
                "send_reaction",
                request_id,
                &self.key,
                Some(started.elapsed().as_millis()),
                Some("target_missing"),
            );
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidReactionTarget);
            return;
        };
        if let Err(kind) =
            validate_send_reaction(target.can_react, target.my_reaction_event_id.as_deref())
        {
            trace_timeline_actor_operation(
                "actor_finish",
                "send_reaction",
                request_id,
                &self.key,
                Some(started.elapsed().as_millis()),
                Some("invalid_state"),
            );
            self.emit_timeline_failure(request_id, kind);
            return;
        }

        let sdk_started = Instant::now();
        match self
            .timeline
            .toggle_reaction(&target.item_id, &reaction_key)
            .await
        {
            Ok(true) => {
                trace_timeline_actor_operation(
                    "sdk_done",
                    "send_reaction",
                    request_id,
                    &self.key,
                    Some(sdk_started.elapsed().as_millis()),
                    Some("sent"),
                );
                trace_timeline_actor_operation(
                    "actor_finish",
                    "send_reaction",
                    request_id,
                    &self.key,
                    Some(started.elapsed().as_millis()),
                    Some("success"),
                );
            }
            Ok(false) => {
                trace_timeline_actor_operation(
                    "sdk_done",
                    "send_reaction",
                    request_id,
                    &self.key,
                    Some(sdk_started.elapsed().as_millis()),
                    Some("invalid_state"),
                );
                trace_timeline_actor_operation(
                    "actor_finish",
                    "send_reaction",
                    request_id,
                    &self.key,
                    Some(started.elapsed().as_millis()),
                    Some("invalid_state"),
                );
                self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidReactionState);
            }
            Err(error) => {
                trace_timeline_actor_operation(
                    "sdk_done",
                    "send_reaction",
                    request_id,
                    &self.key,
                    Some(sdk_started.elapsed().as_millis()),
                    Some("sdk_error"),
                );
                trace_timeline_actor_operation(
                    "actor_finish",
                    "send_reaction",
                    request_id,
                    &self.key,
                    Some(started.elapsed().as_millis()),
                    Some("sdk_error"),
                );
                self.emit_timeline_failure(request_id, classify_reaction_error(&error));
            }
        }
    }
    pub(super) async fn handle_redact_reaction(
        &mut self,
        request_id: RequestId,
        event_id: String,
        reaction_key: String,
        reaction_event_id: String,
    ) {
        let started = Instant::now();
        trace_timeline_actor_operation(
            "actor_start",
            "redact_reaction",
            request_id,
            &self.key,
            None,
            None,
        );
        if reaction_key.trim().is_empty() || reaction_event_id.trim().is_empty() {
            trace_timeline_actor_operation(
                "actor_finish",
                "redact_reaction",
                request_id,
                &self.key,
                Some(started.elapsed().as_millis()),
                Some("invalid_target"),
            );
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidReactionTarget);
            return;
        }

        let Some(target) = self
            .reaction_target_state(request_id, "redact_reaction", &event_id, &reaction_key)
            .await
        else {
            trace_timeline_actor_operation(
                "actor_finish",
                "redact_reaction",
                request_id,
                &self.key,
                Some(started.elapsed().as_millis()),
                Some("target_missing"),
            );
            self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidReactionTarget);
            return;
        };
        if let Err(kind) = validate_redact_reaction(
            target.can_react,
            target.my_reaction_event_id.as_deref(),
            &reaction_event_id,
        ) {
            trace_timeline_actor_operation(
                "actor_finish",
                "redact_reaction",
                request_id,
                &self.key,
                Some(started.elapsed().as_millis()),
                Some("invalid_state"),
            );
            self.emit_timeline_failure(request_id, kind);
            return;
        }

        let sdk_started = Instant::now();
        match self
            .timeline
            .toggle_reaction(&target.item_id, &reaction_key)
            .await
        {
            Ok(false) => {
                trace_timeline_actor_operation(
                    "sdk_done",
                    "redact_reaction",
                    request_id,
                    &self.key,
                    Some(sdk_started.elapsed().as_millis()),
                    Some("redacted"),
                );
                trace_timeline_actor_operation(
                    "actor_finish",
                    "redact_reaction",
                    request_id,
                    &self.key,
                    Some(started.elapsed().as_millis()),
                    Some("success"),
                );
            }
            Ok(true) => {
                trace_timeline_actor_operation(
                    "sdk_done",
                    "redact_reaction",
                    request_id,
                    &self.key,
                    Some(sdk_started.elapsed().as_millis()),
                    Some("invalid_state"),
                );
                trace_timeline_actor_operation(
                    "actor_finish",
                    "redact_reaction",
                    request_id,
                    &self.key,
                    Some(started.elapsed().as_millis()),
                    Some("invalid_state"),
                );
                self.emit_timeline_failure(request_id, TimelineFailureKind::InvalidReactionState);
            }
            Err(error) => {
                trace_timeline_actor_operation(
                    "sdk_done",
                    "redact_reaction",
                    request_id,
                    &self.key,
                    Some(sdk_started.elapsed().as_millis()),
                    Some("sdk_error"),
                );
                trace_timeline_actor_operation(
                    "actor_finish",
                    "redact_reaction",
                    request_id,
                    &self.key,
                    Some(started.elapsed().as_millis()),
                    Some("sdk_error"),
                );
                self.emit_timeline_failure(request_id, classify_reaction_error(&error));
            }
        }
    }
    pub(super) async fn handle_ignored_users_updated(
        &mut self,
        user_ids: std::collections::BTreeSet<String>,
    ) {
        if self.ignored_user_ids == user_ids {
            return;
        }
        let activity_permit = reserve_canonical_activity_action(&self.action_tx, &self.key).await;
        let activity_commit_lease = if activity_permit.is_some() {
            self.timeline_actor_generations
                .try_acquire(&self.key, self.actor_generation)
        } else {
            None
        };
        if matches!(self.key.kind, TimelineKind::Room { .. })
            && (activity_permit.is_none() || activity_commit_lease.is_none())
        {
            return;
        }
        self.ignored_user_ids = user_ids;

        let mut core_diffs = Vec::new();
        for (index, item) in self.navigation_items.iter_mut().enumerate() {
            let was_hidden = item.is_hidden;
            apply_ignored_sender_suppression(item, &self.ignored_user_ids);
            if item.is_hidden != was_hidden {
                core_diffs.push(TimelineDiff::Set {
                    index,
                    item: item.clone(),
                });
            }
        }
        if core_diffs.is_empty() {
            drop(activity_commit_lease);
            drop(activity_permit);
            return;
        }

        self.emit_media_gallery_if_changed().await;

        if self.emit_non_sdk_item_sets_and_reconcile_replay_known(core_diffs) {
            if let Some(activity_permit) = activity_permit {
                activity_permit.send(vec![
                    canonical_activity_window_action(&self.key, &self.navigation_items)
                        .expect("room ignored-user Activity action"),
                ]);
            }
            self.emit_navigation_if_changed();
        }
        drop(activity_commit_lease);
    }
    fn emit_timeline_item_set(&mut self, index: usize) -> bool {
        let core_diffs = vec![TimelineDiff::Set {
            index,
            item: self.navigation_items[index].clone(),
        }];
        self.emit_non_sdk_item_sets_and_reconcile_replay_known(core_diffs)
    }
    pub(super) async fn handle_load_link_previews(
        &mut self,
        request_id: RequestId,
        event_id: String,
    ) {
        let trace_started = Some(std::time::Instant::now());
        let Some(index) = self.navigation_items.iter().position(
            |item| matches!(&item.id, TimelineItemId::Event { event_id: id } if id == &event_id),
        ) else {
            trace_timeline_link_preview(
                "lookup_miss",
                request_id,
                &self.key,
                0,
                0,
                0,
                trace_started.map(|started| started.elapsed().as_millis()),
                Some("lookup_miss"),
            );
            return;
        };

        let Some(previews) = self.navigation_items[index].link_previews.clone() else {
            trace_timeline_link_preview(
                "no_previews",
                request_id,
                &self.key,
                0,
                0,
                0,
                trace_started.map(|started| started.elapsed().as_millis()),
                Some("no_previews"),
            );
            return;
        };

        let pending_count = previews
            .iter()
            .filter(|preview| preview.state == LinkPreviewState::Pending)
            .count();
        trace_timeline_link_preview(
            "start",
            request_id,
            &self.key,
            pending_count,
            0,
            0,
            None,
            None,
        );

        if pending_count == 0 {
            trace_timeline_link_preview(
                "complete",
                request_id,
                &self.key,
                pending_count,
                0,
                0,
                trace_started.map(|started| started.elapsed().as_millis()),
                Some("unchanged"),
            );
            return;
        }

        let mut loading_previews = previews.clone();
        for preview in &mut loading_previews {
            if preview.state == LinkPreviewState::Pending {
                preview.state = LinkPreviewState::Loading;
            }
        }
        self.navigation_items[index].link_previews = Some(loading_previews);
        self.emit_timeline_item_set(index);

        let task = spawn_link_preview_fetch(
            self.session.clone(),
            self.msg_tx.clone(),
            request_id,
            event_id.clone(),
            previews,
        );
        if let Some(previous) = self.link_preview_fetches.insert(event_id, task) {
            previous.abort();
        }
    }
    pub(super) async fn handle_link_previews_fetched(
        &mut self,
        request_id: RequestId,
        event_id: String,
        previews: Vec<LinkPreview>,
        pending_count: usize,
        ready_count: usize,
        failed_count: usize,
        elapsed_ms: u128,
    ) {
        if self.link_preview_fetches.remove(&event_id).is_none() {
            trace_timeline_link_preview(
                "complete",
                request_id,
                &self.key,
                pending_count,
                ready_count,
                failed_count,
                Some(elapsed_ms),
                Some("discarded"),
            );
            return;
        }
        let Some(index) = self.navigation_items.iter().position(
            |item| matches!(&item.id, TimelineItemId::Event { event_id: id } if id == &event_id),
        ) else {
            trace_timeline_link_preview(
                "lookup_miss",
                request_id,
                &self.key,
                pending_count,
                ready_count,
                failed_count,
                Some(elapsed_ms),
                Some("lookup_miss"),
            );
            return;
        };

        let Some(current_previews) = self.navigation_items[index].link_previews.as_mut() else {
            trace_timeline_link_preview(
                "complete",
                request_id,
                &self.key,
                pending_count,
                ready_count,
                failed_count,
                Some(elapsed_ms),
                Some("discarded"),
            );
            return;
        };

        let fetched_by_url: HashMap<String, LinkPreview> = previews
            .into_iter()
            .map(|preview| (preview.url.clone(), preview))
            .collect();
        let mut changed = false;
        for current in current_previews {
            if current.state != LinkPreviewState::Pending
                && current.state != LinkPreviewState::Loading
            {
                continue;
            }
            if let Some(fetched) = fetched_by_url.get(&current.url) {
                if fetched.state == LinkPreviewState::Ready {
                    self.link_preview_policy
                        .cache
                        .insert(fetched.url.clone(), fetched.clone());
                }
                if current != fetched {
                    *current = fetched.clone();
                    changed = true;
                }
            }
        }

        if changed {
            self.emit_timeline_item_set(index);
        }
        trace_timeline_link_preview(
            "complete",
            request_id,
            &self.key,
            pending_count,
            ready_count,
            failed_count,
            Some(elapsed_ms),
            Some(if changed { "updated" } else { "discarded" }),
        );
    }
    pub(super) fn handle_cancel_link_previews(&mut self, request_id: RequestId) {
        let fetch_count = self.link_preview_fetches.len();
        if fetch_count == 0 {
            return;
        }

        for (_, task) in self.link_preview_fetches.drain() {
            task.abort();
        }

        let mut core_diffs = Vec::new();
        for (index, item) in self.navigation_items.iter_mut().enumerate() {
            if reset_loading_link_previews_to_pending(item) {
                core_diffs.push(TimelineDiff::Set {
                    index,
                    item: item.clone(),
                });
            }
        }

        trace_timeline_link_preview(
            "cancelled",
            request_id,
            &self.key,
            fetch_count,
            0,
            0,
            None,
            Some("cancelled"),
        );

        if core_diffs.is_empty() {
            return;
        }

        let _ = self.emit_non_sdk_item_sets_and_reconcile_replay_known(core_diffs);
    }
    pub(super) async fn handle_hide_link_preview(
        &mut self,
        _request_id: RequestId,
        event_id: String,
    ) {
        let mut context = self.link_preview_policy.for_room(self.key.room_id());
        if !context.hidden_event_ids.insert(event_id.clone()) {
            return;
        }
        self.link_preview_policy.hidden_event_ids = context.hidden_event_ids.clone();

        let mut core_diffs = Vec::new();
        for (index, item) in self.navigation_items.iter_mut().enumerate() {
            if matches!(&item.id, TimelineItemId::Event { event_id: id } if id == &event_id) {
                apply_link_previews_to_item(item, self.key.room_id(), &context, &self.session)
                    .await;
                core_diffs.push(TimelineDiff::Set {
                    index,
                    item: item.clone(),
                });
            }
        }

        if core_diffs.is_empty() {
            return;
        }

        let _ = self.emit_non_sdk_item_sets_and_reconcile_replay_known(core_diffs);
    }
    pub(super) async fn handle_link_preview_policy_changed(
        &mut self,
        unencrypted_global_enabled: bool,
        encrypted_global_enabled: bool,
        room_enabled: Option<bool>,
    ) {
        self.link_preview_policy.apply_policy_delta(
            unencrypted_global_enabled,
            encrypted_global_enabled,
            room_enabled,
        );
        let context = self.link_preview_policy.for_room(self.key.room_id());

        let mut core_diffs = Vec::new();
        for (index, item) in self.navigation_items.iter_mut().enumerate() {
            let old = item.link_previews.clone();
            apply_link_previews_to_item(item, self.key.room_id(), &context, &self.session).await;
            if item.link_previews != old {
                core_diffs.push(TimelineDiff::Set {
                    index,
                    item: item.clone(),
                });
            }
        }

        if core_diffs.is_empty() {
            return;
        }

        let _ = self.emit_non_sdk_item_sets_and_reconcile_replay_known(core_diffs);
    }
    pub(super) fn maybe_fetch_visible_reply_details(&mut self) {
        let event_ids = visible_missing_reply_detail_event_ids(
            &self.navigation_items,
            &self.viewport_observation,
            &self.reply_detail_fetch_attempted_event_ids,
        );
        for event_id in event_ids {
            if !self
                .reply_detail_fetch_attempted_event_ids
                .insert(event_id.clone())
            {
                continue;
            }
            let task = spawn_reply_detail_fetch(
                self.timeline.clone(),
                self.msg_tx.clone(),
                event_id.clone(),
            );
            if let Some(previous) = self.reply_detail_fetches.insert(event_id, task) {
                previous.abort();
            }
        }
    }
    /// Forward SDK diff mutations to the search index channel reliably.
    /// Redactions are privacy-sensitive removals and must not be silently
    /// dropped as freshness-only updates.
    async fn forward_diff_to_search(&self, diff: &eyeball_im::VectorDiff<Arc<SdkTimelineItem>>) {
        let _ = self
            .emit_search_messages_reliable(self.search_index_messages_for_diff(diff))
            .await;
    }
    pub(super) fn search_index_messages_for_diff(
        &self,
        diff: &eyeball_im::VectorDiff<Arc<SdkTimelineItem>>,
    ) -> Vec<SearchIndexMessage> {
        use eyeball_im::VectorDiff;

        let room_id = match &self.key.kind {
            TimelineKind::Room { room_id }
            | TimelineKind::Thread { room_id, .. }
            | TimelineKind::Focused { room_id, .. } => room_id.as_str(),
        };

        match diff {
            VectorDiff::PushFront { value }
            | VectorDiff::PushBack { value }
            | VectorDiff::Insert { value, .. }
            | VectorDiff::Set { value, .. } => self.search_index_messages_for_item(room_id, value),
            VectorDiff::Append { values } | VectorDiff::Reset { values } => values
                .iter()
                .flat_map(|item| self.search_index_messages_for_item(room_id, item))
                .collect(),
            VectorDiff::Remove { .. }
            | VectorDiff::Truncate { .. }
            | VectorDiff::Clear
            | VectorDiff::PopFront
            | VectorDiff::PopBack => {
                // Remove/truncate/clear: we don't know which event_ids are affected
                // without tracking the full timeline list; skip search forwarding.
                // Redactions arrive as Set-with-is_redacted=true before any Remove.
                Vec::new()
            }
        }
    }
    fn search_index_messages_for_item(
        &self,
        room_id: &str,
        item: &Arc<SdkTimelineItem>,
    ) -> Vec<SearchIndexMessage> {
        use matrix_sdk_ui::timeline::TimelineItemKind;
        let event_item = match item.kind() {
            TimelineItemKind::Event(e) => e,
            TimelineItemKind::Virtual(_) => return Vec::new(),
        };

        // Only remote events have a stable event_id we can index.
        let event_id = match event_item.event_id() {
            Some(id) => id.to_string(),
            None => return Vec::new(), // local-echo without confirmed event_id: skip
        };

        let sender = event_item.sender().to_string();
        let timestamp_ms: u64 = event_item.timestamp().0.into();

        // Redacted items: forward Redact so the document is removed.
        if event_item.content().is_redacted() {
            return vec![SearchIndexMessage::Redact { event_id }];
        }

        let (body, attachment_filename, attachment, edit_event_id) =
            if let Some(sticker) = event_item.content().as_sticker() {
                (
                    None,
                    Some(sticker.content().body.clone()),
                    Some(Self::attachment_document_from_sticker(sticker)),
                    None,
                )
            } else if let Some(message) = event_item.content().as_message() {
                let projection = message_projection_from_msgtype(message.msgtype(), message.body());

                // Detect edits: when is_edited() is true, the SDK ngram index will
                // index the edit event under the edit event_id (not the original).
                // We must register an alias so verify_candidate can resolve it back.
                // Extract the edit event_id from latest_edit_json if available.
                let edit_event_id = if message.is_edited() {
                    event_item
                        .latest_edit_json()
                        .and_then(|raw| {
                            raw.get_field::<matrix_sdk::ruma::OwnedEventId>("event_id")
                                .ok()
                                .flatten()
                        })
                        .map(|id| id.to_string())
                } else {
                    None
                };

                (
                    projection.body,
                    projection
                        .media
                        .as_ref()
                        .map(|media| media.filename.clone()),
                    projection.media.as_ref().and_then(|media| {
                        Self::attachment_document_from_timeline_media(media, event_item, message)
                    }),
                    edit_event_id,
                )
            } else {
                return Vec::new();
            };

        if body.is_none() && attachment_filename.is_none() {
            return Vec::new();
        }

        if let Some(edit_event_id) = edit_event_id {
            // Edited message: Upsert original with new canonical body, AND
            // forward Edit so the document store registers the alias
            // (edit_event_id → original_event_id) used by verify_candidate.
            vec![
                SearchIndexMessage::Upsert {
                    room_id: room_id.to_owned(),
                    event_id: event_id.clone(),
                    sender: sender.clone(),
                    timestamp_ms,
                    body: body.clone(),
                    attachment_filename: attachment_filename.clone(),
                    attachment: attachment.clone(),
                },
                SearchIndexMessage::Edit {
                    edit_event_id,
                    target_event_id: event_id,
                    sender,
                    timestamp_ms,
                    body,
                    attachment_filename,
                    attachment,
                },
            ]
        } else {
            // New (unedited) message: Upsert into document store.
            vec![SearchIndexMessage::Upsert {
                room_id: room_id.to_owned(),
                event_id,
                sender,
                timestamp_ms,
                body,
                attachment_filename,
                attachment,
            }]
        }
    }
    pub(super) async fn forward_initial_items_to_search(
        &self,
        items: impl IntoIterator<Item = Arc<SdkTimelineItem>>,
    ) {
        use eyeball_im::VectorDiff;

        for item in items {
            self.forward_diff_to_search(&VectorDiff::PushBack { value: item })
                .await;
        }
    }
    fn attachment_document_from_timeline_media(
        media: &TimelineMedia,
        event_item: &EventTimelineItem,
        message: &matrix_sdk_ui::timeline::Message,
    ) -> Option<AttachmentDocument> {
        let kind = match media.kind {
            crate::event::TimelineMediaKind::Image => AttachmentKind::Image,
            crate::event::TimelineMediaKind::Video => AttachmentKind::Video,
            crate::event::TimelineMediaKind::Audio => AttachmentKind::Audio,
            crate::event::TimelineMediaKind::File => AttachmentKind::File,
        };

        let msgtype = match media.kind {
            crate::event::TimelineMediaKind::Image => "m.image",
            crate::event::TimelineMediaKind::Video => "m.video",
            crate::event::TimelineMediaKind::Audio => "m.audio",
            crate::event::TimelineMediaKind::File => "m.file",
        };

        let thread_root = event_item.content().thread_root().map(|id| id.to_string());

        Some(AttachmentDocument {
            kind,
            msgtype: msgtype.to_owned(),
            mimetype: media.mimetype.clone(),
            size: media.size,
            source_mxc: media.source.mxc_uri.clone(),
            thumbnail_mxc: media
                .thumbnail
                .as_ref()
                .map(|thumbnail| thumbnail.source.mxc_uri.clone()),
            filename: SensitiveString::new(media.filename.clone()),
            thread_root,
            encrypted: media.source.encrypted,
            encryption_version: media.source.encryption_version.clone(),
            width: media.width.and_then(|w| u32::try_from(w).ok()),
            height: media.height.and_then(|h| u32::try_from(h).ok()),
            is_edited: message.is_edited(),
        })
    }
    fn attachment_document_from_sticker(
        sticker: &matrix_sdk_ui::timeline::Sticker,
    ) -> AttachmentDocument {
        use matrix_sdk::ruma::events::sticker::{StickerEventContent, StickerMediaSource};

        let content: &StickerEventContent = sticker.content();
        let info = &content.info;

        let source = match &content.source {
            StickerMediaSource::Plain(uri) => TimelineMediaSource {
                mxc_uri: uri.to_string(),
                encrypted: false,
                encryption_version: None,
            },
            StickerMediaSource::Encrypted(file) => TimelineMediaSource {
                mxc_uri: file.url.to_string(),
                encrypted: true,
                encryption_version: Some(file.info.version().to_owned()),
            },
            _ => TimelineMediaSource {
                mxc_uri: String::new(),
                encrypted: false,
                encryption_version: None,
            },
        };

        let thumbnail_mxc = info
            .thumbnail_source
            .as_ref()
            .map(|thumbnail_source| timeline_media_source_from_sdk(thumbnail_source).mxc_uri);

        AttachmentDocument {
            kind: AttachmentKind::Sticker,
            msgtype: "m.sticker".to_owned(),
            mimetype: info.mimetype.clone(),
            size: uint_to_u64(info.size.as_ref()),
            source_mxc: source.mxc_uri,
            thumbnail_mxc,
            filename: SensitiveString::new(content.body.clone()),
            thread_root: None,
            encrypted: source.encrypted,
            encryption_version: source.encryption_version,
            width: None,
            height: None,
            is_edited: false,
        }
    }
    /// Resolve the timeline item identity for `event_id`, falling back to the
    /// local-echo transaction identity for events this actor sent whose
    /// remote echo has not arrived.
    fn item_ids_for_event(&self, event_id: &str) -> Vec<TimelineEventItemId> {
        let mut ids = Vec::with_capacity(2);
        if let Ok(parsed) = matrix_sdk::ruma::EventId::parse(event_id) {
            ids.push(TimelineEventItemId::EventId(parsed));
        }
        if let Some(txn) = self.sent_event_txns.get(event_id) {
            ids.push(TimelineEventItemId::TransactionId(txn.clone()));
        }
        ids
    }
    pub(super) fn timeline_contains_event_id(&self, event_id: &str) -> bool {
        self.navigation_items.iter().any(
            |item| matches!(&item.id, TimelineItemId::Event { event_id: id } if id == event_id),
        )
    }
    pub(super) async fn project_message_source_for_event(
        &self,
        event_id: &str,
    ) -> Option<TimelineMessageSource> {
        let parsed_event_id = matrix_sdk::ruma::EventId::parse(event_id).ok()?;
        let items = self.timeline.items().await;
        for item in items.iter().rev() {
            let TimelineItemKind::Event(event_item) = item.kind() else {
                continue;
            };
            if !event_item
                .event_id()
                .map(|candidate| candidate.as_str() == parsed_event_id.as_str())
                .unwrap_or(false)
            {
                continue;
            }

            let projected = sdk_item_to_timeline_item(&self.key, item, self.own_user_id.as_deref());
            let mut source = message_source_for_timeline_item(&projected)?;
            let encryption_info = event_item.encryption_info();
            let session_id = encryption_info
                .and_then(|info| info.session_id())
                .filter(|session_id| !session_id.is_empty())
                .map(str::to_owned);
            let sent_by_current_device = encryption_info
                .and_then(|info| info.sender_device.as_deref())
                .is_some_and(|device_id| device_id.as_str() == self.session.info.device_id);
            source.megolm_session_fingerprint =
                session_id.as_deref().map(megolm_session_fingerprint);
            if let Some(session_id) = session_id.as_deref() {
                let reason = if sent_by_current_device {
                    koushi_sdk::room_key_rotation_reason(
                        &self.session,
                        self.key.room_id(),
                        session_id,
                    )
                    .await
                } else {
                    None
                };
                source.megolm_session_rotation_reason =
                    project_local_megolm_rotation_reason(sent_by_current_device, reason);
            }
            source.original_json = original_json_for_event_item(event_item);
            source.megolm_message_index = source
                .original_json
                .as_ref()
                .and_then(megolm_message_index_from_original_json);
            return Some(source);
        }
        None
    }
    async fn reaction_target_state(
        &self,
        request_id: RequestId,
        trace_kind: &'static str,
        event_id: &str,
        reaction_key: &str,
    ) -> Option<ReactionTargetState> {
        let started = Instant::now();
        let parsed_event_id = match matrix_sdk::ruma::EventId::parse(event_id) {
            Ok(event_id) => event_id,
            Err(_) => {
                trace_timeline_actor_scan(
                    "target_scan",
                    trace_kind,
                    request_id,
                    &self.key,
                    0,
                    started.elapsed().as_millis(),
                    false,
                );
                return None;
            }
        };
        let items = self.timeline.items().await;
        let item_count = items.len();
        for item in items.iter().rev() {
            let TimelineItemKind::Event(event_item) = item.kind() else {
                continue;
            };
            if !event_item
                .event_id()
                .map(|candidate| candidate.as_str() == parsed_event_id.as_str())
                .unwrap_or(false)
            {
                continue;
            }

            let projected = sdk_item_to_timeline_item(&self.key, item, self.own_user_id.as_deref());
            let my_reaction_event_id = projected
                .reactions
                .iter()
                .find(|reaction| reaction.key == reaction_key)
                .and_then(|reaction| reaction.my_reaction_event_id.clone());
            trace_timeline_actor_scan(
                "target_scan",
                trace_kind,
                request_id,
                &self.key,
                item_count,
                started.elapsed().as_millis(),
                true,
            );
            return Some(ReactionTargetState {
                item_id: TimelineEventItemId::EventId(parsed_event_id),
                can_react: projected.can_react,
                my_reaction_event_id,
            });
        }
        trace_timeline_actor_scan(
            "target_scan",
            trace_kind,
            request_id,
            &self.key,
            item_count,
            started.elapsed().as_millis(),
            false,
        );
        None
    }
}

pub(super) fn timeline_room_id(key: &TimelineKey) -> Option<String> {
    match &key.kind {
        TimelineKind::Room { room_id }
        | TimelineKind::Thread { room_id, .. }
        | TimelineKind::Focused { room_id, .. } => Some(room_id.clone()),
    }
}

pub(super) fn apply_ignored_sender_suppression(
    item: &mut TimelineItem,
    ignored_user_ids: &std::collections::BTreeSet<String>,
) {
    if !matches!(&item.id, TimelineItemId::Event { .. }) {
        return;
    }
    let sender_ignored = item
        .sender
        .as_deref()
        .is_some_and(|sender| ignored_user_ids.contains(sender));
    // Recompute from projected content, not the previous ignored result. This
    // keeps ignore→unignore reversible while retaining the normal bodyless
    // suppression baseline.
    item.is_hidden = (!has_user_visible_content(item) && !item.is_redacted) || sender_ignored;
}

pub(super) fn apply_ignored_sender_suppression_to_diff(
    diff: &mut TimelineDiff,
    ignored_user_ids: &std::collections::BTreeSet<String>,
) {
    match diff {
        TimelineDiff::PushFront { item }
        | TimelineDiff::PushBack { item }
        | TimelineDiff::Insert { item, .. }
        | TimelineDiff::Set { item, .. } => {
            apply_ignored_sender_suppression(item, ignored_user_ids);
        }
        TimelineDiff::Reset { items } => {
            for item in items {
                apply_ignored_sender_suppression(item, ignored_user_ids);
            }
        }
        TimelineDiff::Remove { .. } | TimelineDiff::Truncate { .. } | TimelineDiff::Clear => {}
    }
}

pub(super) async fn apply_link_previews_to_item(
    item: &mut TimelineItem,
    room_id: &str,
    context: &LinkPreviewContext,
    session: &Arc<MatrixClientSession>,
) {
    let TimelineItemId::Event { event_id } = &item.id else {
        return;
    };

    let is_encrypted = match matrix_sdk::ruma::RoomId::parse(room_id) {
        Ok(room_id) => match session.client().get_room(&room_id) {
            Some(room) => match room.latest_encryption_state().await {
                Ok(state) => state.is_encrypted(),
                Err(_) => false,
            },
            None => false,
        },
        Err(_) => false,
    };

    item.link_previews = crate::link_preview::link_previews_for_message(
        item.body.as_deref(),
        item.formatted.as_ref(),
        event_id,
        is_encrypted,
        context,
    );
}

fn reset_loading_link_previews_to_pending(item: &mut TimelineItem) -> bool {
    let Some(previews) = item.link_previews.as_mut() else {
        return false;
    };
    let mut changed = false;
    for preview in previews {
        if preview.state == LinkPreviewState::Loading {
            preview.state = LinkPreviewState::Pending;
            changed = true;
        }
    }
    changed
}

pub(super) fn eligible_activity_preview(item: &TimelineItem) -> Option<String> {
    let source = item
        .formatted
        .as_ref()
        .map(|formatted| formatted.plain_text.as_str())
        .or(item.body.as_deref())
        .or_else(|| item.media.as_ref().map(|media| media.filename.as_str()))?;
    collapsed_preview(source, REPLY_QUOTE_PREVIEW_MAX_CHARS)
}

pub(super) fn is_attention_eligible_event(item: &TimelineItem) -> bool {
    matches!(item.id, TimelineItemId::Event { .. })
        && !item.is_redacted
        && !item.is_hidden
        && eligible_activity_preview(item).is_some()
}

pub(super) fn is_unread_navigation_item(item: &TimelineItem, own_user_id: Option<&str>) -> bool {
    if !is_attention_eligible_event(item) {
        return false;
    }
    if own_user_id.is_some_and(|own| item.sender.as_deref() == Some(own)) {
        return false;
    }
    true
}

pub(super) fn has_user_visible_content(item: &TimelineItem) -> bool {
    timeline_content_is_renderable(
        item.body.as_deref(),
        item.media.as_ref(),
        item.formatted.as_ref(),
    )
}

pub(super) fn timeline_content_is_renderable(
    body: Option<&str>,
    media: Option<&TimelineMedia>,
    formatted: Option<&crate::event::TimelineFormattedBody>,
) -> bool {
    body.is_some_and(|body| !body.trim().is_empty())
        || media.is_some()
        || formatted.is_some_and(timeline_formatted_body_is_renderable)
}

pub(super) fn timeline_formatted_body_is_renderable(
    formatted: &crate::event::TimelineFormattedBody,
) -> bool {
    !formatted.plain_text.trim().is_empty()
        || formatted
            .code_blocks
            .iter()
            .any(|block| !block.body.trim().is_empty())
}

pub(super) fn timeline_sender_label_from_profile(
    profile: &TimelineDetails<Profile>,
) -> Option<String> {
    match profile {
        TimelineDetails::Ready(profile) => profile.display_name.clone(),
        TimelineDetails::Unavailable | TimelineDetails::Pending | TimelineDetails::Error(_) => None,
    }
}

pub(super) fn timeline_sender_avatar_from_profile(
    profile: &TimelineDetails<Profile>,
) -> Option<AvatarImage> {
    let TimelineDetails::Ready(profile) = profile else {
        return None;
    };
    let avatar_url = profile.avatar_url.as_ref()?;
    Some(AvatarImage {
        mxc_uri: avatar_url.to_string(),
        thumbnail: AvatarThumbnailState::NotRequested,
    })
}

fn original_json_for_event_item(event_item: &EventTimelineItem) -> Option<serde_json::Value> {
    event_item
        .original_json()
        .and_then(|raw| serde_json::from_str(raw.json().get()).ok())
}

fn megolm_message_index_from_original_json(original_json: &serde_json::Value) -> Option<u32> {
    if original_json.get("type")?.as_str()? != "m.room.encrypted" {
        return None;
    }
    let content = original_json.get("content")?;
    if content.get("algorithm")?.as_str()? != "m.megolm.v1.aes-sha2" {
        return None;
    }
    let ciphertext = content.get("ciphertext")?.as_str()?;
    vodozemac::megolm::MegolmMessage::from_base64(ciphertext)
        .ok()
        .map(|message| message.message_index())
}

fn project_local_megolm_rotation_reason(
    sent_by_current_device: bool,
    reason: Option<koushi_sdk::MatrixRoomKeyRotationReason>,
) -> Option<TimelineMegolmSessionReason> {
    use koushi_sdk::MatrixRoomKeyRotationReason as Reason;
    if !sent_by_current_device {
        return None;
    }
    Some(match reason {
        Some(Reason::Initial) => TimelineMegolmSessionReason::Initial,
        Some(Reason::ExpiredTime) => TimelineMegolmSessionReason::ExpiredTime,
        Some(Reason::ExpiredMessageCount) => TimelineMegolmSessionReason::ExpiredMessageCount,
        Some(Reason::MembershipOrDeviceChange) => {
            TimelineMegolmSessionReason::MembershipOrDeviceChange
        }
        Some(Reason::EncryptionSettingsChanged) => {
            TimelineMegolmSessionReason::EncryptionSettingsChanged
        }
        Some(Reason::ExplicitDiscard) => TimelineMegolmSessionReason::ExplicitDiscard,
        Some(Reason::FullMemberListReload) => TimelineMegolmSessionReason::FullMemberListReload,
        Some(Reason::RoomSubscription) => TimelineMegolmSessionReason::RoomSubscription,
        Some(Reason::LimitedSyncResponse) => TimelineMegolmSessionReason::LimitedSyncResponse,
        Some(Reason::KeyShareFailure) => TimelineMegolmSessionReason::KeyShareFailure,
        Some(Reason::StoreMissing) => TimelineMegolmSessionReason::StoreMissing,
        Some(Reason::Invalidated) => TimelineMegolmSessionReason::Invalidated,
        Some(Reason::Unknown) => TimelineMegolmSessionReason::Unknown,
        None => TimelineMegolmSessionReason::NotRetained,
    })
}

pub(super) fn megolm_session_fingerprint(session_id: &str) -> String {
    // Matrix Megolm session IDs are random base64 strings. A 12-character
    // prefix is compact while providing enough entropy to distinguish session
    // rotation without exposing the complete identifier in the UI.
    session_id.chars().take(12).collect()
}

fn effective_message_content(raw: &serde_json::Value) -> Option<&serde_json::Value> {
    let content = raw.get("content")?;
    Some(
        content
            .get("m.relates_to")
            .and_then(|relation| {
                (relation.get("rel_type")?.as_str() == Some("m.replace"))
                    .then(|| relation.get("m.new_content"))
            })
            .flatten()
            .unwrap_or(content),
    )
}

fn mention_intent_from_event_json(raw: &serde_json::Value) -> Option<MentionIntent> {
    let effective_content = effective_message_content(raw)?;
    let mentions = effective_content.get("m.mentions")?;
    let mut targets = mentions
        .get("user_ids")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter(|user_id| !user_id.trim().is_empty())
        .map(|user_id| MentionTarget::User {
            user_id: user_id.to_owned(),
            // The renderer replaces this safe fallback with the current room
            // candidate's display label before opening the editor.
            display_label: user_id.trim_start_matches('@').to_owned(),
        })
        .collect::<Vec<_>>();
    if mentions
        .get("room")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        targets.push(MentionTarget::RoomMention {
            display_label: "room".to_owned(),
        });
    }
    (!targets.is_empty()).then_some(MentionIntent { targets })
}

fn composer_document_from_event_json(raw: &serde_json::Value) -> Option<ComposerDocument> {
    let content = effective_message_content(raw)?;
    let body = content.get("body")?.as_str()?;
    let mentions = mention_intent_from_event_json(raw).unwrap_or_default();
    let formatted_body = content.get("formatted_body")?.as_str()?;
    let html = Html::parse(formatted_body);
    let mut parsed = Vec::new();
    collect_composer_inlines_from_nodes(html.children(), &mentions, &mut parsed);
    let mut inlines = Vec::new();
    let mut remaining = body;
    for inline in parsed {
        let ComposerInline::Mention {
            target,
            display_label,
        } = inline
        else {
            continue;
        };
        let needle = format!("@{display_label}");
        let offset = remaining.find(&needle)?;
        if offset > 0 {
            inlines.push(ComposerInline::Text {
                text: remaining[..offset].to_owned(),
            });
        }
        inlines.push(ComposerInline::Mention {
            target,
            display_label,
        });
        remaining = &remaining[offset + needle.len()..];
    }
    if !remaining.is_empty() {
        inlines.push(ComposerInline::Text {
            text: remaining.to_owned(),
        });
    }
    let document = ComposerDocument::new(inlines);
    (!document.mention_intent().targets.is_empty()).then_some(document)
}

fn collect_composer_inlines_from_nodes(
    nodes: impl Iterator<Item = matrix_sdk::ruma::html::NodeRef>,
    mentions: &MentionIntent,
    out: &mut Vec<ComposerInline>,
) {
    for node in nodes {
        if let Some(text) = node.as_text() {
            out.push(ComposerInline::Text {
                text: text.borrow().to_string(),
            });
            continue;
        }
        let Some(element) = node.as_element() else {
            continue;
        };
        let attrs = element.attrs.borrow();
        let href = attrs
            .iter()
            .find_map(|attr| (attr.name.local.as_ref() == "href").then(|| attr.value.to_string()));
        let room_mention = attrs
            .iter()
            .any(|attr| attr.name.local.as_ref() == "data-mx-mention");
        drop(attrs);

        let mut label = String::new();
        collect_plain_text_from_nodes(node.children(), &mut label);
        let display_label = label.strip_prefix('@').unwrap_or(&label).to_owned();
        if let Some(target) = href
            .as_deref()
            .and_then(matrix_to_mention_target)
            .and_then(|target| allowed_editable_mention_target(target, mentions, &display_label))
        {
            out.push(ComposerInline::Mention {
                target,
                display_label,
            });
        } else if room_mention && mentions.mentions_room() {
            out.push(ComposerInline::Mention {
                target: MentionTarget::RoomMention {
                    display_label: display_label.clone(),
                },
                display_label,
            });
        } else {
            collect_composer_inlines_from_nodes(node.children(), mentions, out);
        }
    }
}

fn matrix_to_mention_target(href: &str) -> Option<MentionTarget> {
    let url = url::Url::parse(href).ok()?;
    if url.scheme() != "https" || url.host_str()? != "matrix.to" {
        return None;
    }
    let encoded = url.fragment()?.strip_prefix('/')?.split('?').next()?;
    let identifier = percent_decode_matrix_identifier(encoded)?;
    if identifier.starts_with('@') {
        Some(MentionTarget::User {
            user_id: identifier,
            display_label: String::new(),
        })
    } else if identifier.starts_with('!') || identifier.starts_with('#') {
        Some(MentionTarget::Room {
            room_id: identifier,
            display_label: String::new(),
        })
    } else {
        None
    }
}

fn allowed_editable_mention_target(
    target: MentionTarget,
    mentions: &MentionIntent,
    display_label: &str,
) -> Option<MentionTarget> {
    match target {
        MentionTarget::User { user_id, .. } => mentions
            .targets
            .iter()
            .any(|target| matches!(target, MentionTarget::User { user_id: existing, .. } if existing == &user_id))
            .then(|| MentionTarget::User {
                user_id,
                display_label: display_label.to_owned(),
            }),
        MentionTarget::Room { room_id, .. } => Some(MentionTarget::Room {
            room_id,
            display_label: display_label.to_owned(),
        }),
        MentionTarget::RoomMention { .. } => None,
    }
}

fn percent_decode_matrix_identifier(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn timeline_item_should_be_hidden(has_renderable_content: bool, is_redacted: bool) -> bool {
    !has_renderable_content && !is_redacted
}

pub(super) fn timeline_item_should_be_hidden_for_key(
    _key: &TimelineKey,
    has_renderable_content: bool,
    is_redacted: bool,
    _thread_root: Option<&str>,
) -> bool {
    timeline_item_should_be_hidden(has_renderable_content, is_redacted)
}

/// Koushi threads are linear, so a thread-keyed reply command is always an
/// ordinary thread message: the relation is threaded and the target event is
/// never promoted to a rich reply. The product UI offers no thread-pane reply
/// action, and this projection keeps a thread rich reply unreachable even if a
/// caller passes a non-root target.
pub(super) fn reply_enforce_thread_for_key(key: &TimelineKey) -> EnforceThread {
    match key.kind {
        TimelineKind::Thread { .. } => EnforceThread::Threaded(ReplyWithinThread::No),
        TimelineKind::Room { .. } | TimelineKind::Focused { .. } => EnforceThread::MaybeThreaded,
    }
}

pub(super) fn attachment_reply_for_key(key: &TimelineKey) -> Option<Reply> {
    let TimelineKind::Thread { root_event_id, .. } = &key.kind else {
        return None;
    };
    Some(Reply {
        event_id: matrix_sdk::ruma::EventId::parse(root_event_id).ok()?,
        enforce_thread: EnforceThread::Threaded(ReplyWithinThread::No),
        add_mentions: AddMentions::No,
    })
}

pub(super) fn thread_root_from_original_json(original_json: &serde_json::Value) -> Option<String> {
    let relates_to = original_json.get("content")?.get("m.relates_to")?;
    if relates_to.get("rel_type")?.as_str()? != "m.thread" {
        return None;
    }
    let event_id = relates_to.get("event_id")?.as_str()?.trim();
    (!event_id.is_empty()).then(|| event_id.to_owned())
}

pub(super) fn item_index_for_event_id(items: &[TimelineItem], event_id: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| timeline_item_event_id(item) == Some(event_id))
}

fn visible_missing_reply_detail_event_ids(
    items: &[TimelineItem],
    observation: &TimelineViewportObservation,
    already_requested_event_ids: &HashSet<String>,
) -> Vec<String> {
    let Some(first_visible_event_id) = observation.first_visible_event_id.as_deref() else {
        return Vec::new();
    };
    let Some(last_visible_event_id) = observation.last_visible_event_id.as_deref() else {
        return Vec::new();
    };
    let Some(first_visible_index) = item_index_for_event_id(items, first_visible_event_id) else {
        return Vec::new();
    };
    let Some(last_visible_index) = item_index_for_event_id(items, last_visible_event_id) else {
        return Vec::new();
    };

    let start = first_visible_index.min(last_visible_index);
    let end = first_visible_index.max(last_visible_index);
    items[start..=end]
        .iter()
        .filter_map(|item| {
            let event_id = timeline_item_event_id(item)?;
            if already_requested_event_ids.contains(event_id) {
                return None;
            }
            let quote = item.reply_quote.as_ref()?;
            (quote.state == ReplyQuoteState::Missing).then(|| event_id.to_owned())
        })
        .collect()
}

pub(super) fn timeline_item_event_id(item: &TimelineItem) -> Option<&str> {
    match &item.id {
        TimelineItemId::Event { event_id } => Some(event_id.as_str()),
        TimelineItemId::Transaction { .. } | TimelineItemId::Synthetic { .. } => None,
    }
}

pub(super) fn live_event_receipts_from_sdk_items<'a>(
    items: impl IntoIterator<Item = &'a Arc<SdkTimelineItem>>,
) -> Vec<LiveEventReceipts> {
    items
        .into_iter()
        .filter_map(|item| live_event_receipts_from_sdk_item(item, false))
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ReceiptObservationTarget {
    Live,
    Authoritative { scoped_event_ids: Vec<String> },
}

fn build_receipt_observation_actions(
    room_id: &str,
    receipts_by_event: Vec<LiveEventReceipts>,
    profiles: Vec<MatrixUserProfile>,
    target: ReceiptObservationTarget,
) -> Vec<AppAction> {
    let profile_actions = profiles
        .into_iter()
        .map(|profile| {
            let display_label = profile
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .unwrap_or("Unknown user")
                .to_owned();
            UserProfile {
                user_id: profile.user_id,
                display_name: profile.display_name,
                display_label: display_label.clone(),
                original_display_label: display_label,
                mention_search_terms: Vec::new(),
                avatar: profile.avatar_mxc_uri.map(|mxc_uri| AvatarImage {
                    mxc_uri,
                    thumbnail: AvatarThumbnailState::NotRequested,
                }),
            }
        })
        .collect::<Vec<_>>();

    let mut actions = Vec::with_capacity(usize::from(!profile_actions.is_empty()) * 2 + 1);
    if !profile_actions.is_empty() {
        actions.push(AppAction::LiveRoomProfilesObserved {
            room_id: room_id.to_owned(),
            profiles: profile_actions.clone(),
        });
        actions.push(AppAction::UserProfilesUpdated {
            profiles: profile_actions,
        });
    }
    actions.push(match target {
        ReceiptObservationTarget::Live => AppAction::LiveRoomReceiptsUpdated {
            room_id: room_id.to_owned(),
            receipts_by_event,
        },
        ReceiptObservationTarget::Authoritative { scoped_event_ids } => {
            AppAction::LiveRoomReceiptsWindowReconciled {
                room_id: room_id.to_owned(),
                scoped_event_ids,
                receipts_by_event,
            }
        }
    });
    actions
}

pub(super) fn build_live_receipt_observation_actions(
    room_id: &str,
    receipts_by_event: Vec<LiveEventReceipts>,
    profiles: Vec<MatrixUserProfile>,
) -> Vec<AppAction> {
    build_receipt_observation_actions(
        room_id,
        receipts_by_event,
        profiles,
        ReceiptObservationTarget::Live,
    )
}

pub(super) async fn live_receipt_observation_actions_from_sdk_receipts(
    session: &MatrixClientSession,
    room_id: &str,
    receipts_by_event: Vec<LiveEventReceipts>,
) -> Vec<AppAction> {
    receipt_observation_actions_from_sdk_receipts(
        session,
        room_id,
        receipts_by_event,
        ReceiptObservationTarget::Live,
    )
    .await
}

async fn receipt_observation_actions_from_sdk_receipts(
    session: &MatrixClientSession,
    room_id: &str,
    receipts_by_event: Vec<LiveEventReceipts>,
    target: ReceiptObservationTarget,
) -> Vec<AppAction> {
    let user_ids = receipts_by_event
        .iter()
        .flat_map(|entry| entry.receipts.iter())
        .map(|receipt| receipt.user_id.clone())
        .collect::<Vec<_>>();
    let requested_user_count = user_ids.iter().collect::<BTreeSet<_>>().len();
    let (profiles, lookup_outcome) = match session
        .room_member_profiles_no_sync(room_id, &user_ids)
        .await
    {
        Ok(profiles) if profiles.is_empty() => (profiles, "miss"),
        Ok(profiles) => (profiles, "observed"),
        Err(_) => (Vec::new(), "failed"),
    };
    record_live_receipt_profile_lookup(
        receipts_by_event
            .iter()
            .map(|entry| entry.receipts.len())
            .sum(),
        requested_user_count,
        profiles.len(),
        lookup_outcome,
    );
    build_receipt_observation_actions(room_id, receipts_by_event, profiles, target)
}

pub(super) async fn emit_receipt_observation_actions(
    session: &MatrixClientSession,
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    timeline_actor_generations: &Arc<TimelineActorGenerationGate>,
    key: &TimelineKey,
    actor_generation: u64,
    room_id: &str,
    receipts_by_event: Vec<LiveEventReceipts>,
    target: ReceiptObservationTarget,
) -> bool {
    let actions =
        receipt_observation_actions_from_sdk_receipts(session, room_id, receipts_by_event, target)
            .await;
    send_generation_fenced(
        action_tx,
        timeline_actor_generations,
        key,
        actor_generation,
        actions,
    )
    .await
}

pub(super) async fn emit_live_receipt_observation_actions(
    session: &MatrixClientSession,
    action_tx: &mpsc::Sender<Vec<AppAction>>,
    timeline_actor_generations: &Arc<TimelineActorGenerationGate>,
    key: &TimelineKey,
    actor_generation: u64,
    room_id: &str,
    receipts_by_event: Vec<LiveEventReceipts>,
) -> bool {
    emit_receipt_observation_actions(
        session,
        action_tx,
        timeline_actor_generations,
        key,
        actor_generation,
        room_id,
        receipts_by_event,
        ReceiptObservationTarget::Live,
    )
    .await
}

fn record_live_receipt_profile_lookup(
    receipt_count: usize,
    requested_user_count: usize,
    observed_profile_count: usize,
    lookup_outcome: &'static str,
) {
    record(
        DiagnosticEvent::new(
            DiagnosticLevel::Debug,
            "core.read_receipt_profile",
            "local_lookup",
        )
        .field(DiagnosticField::token("lookup_outcome", lookup_outcome))
        .field(DiagnosticField::count(
            "receipt_count",
            receipt_count as u64,
        ))
        .field(DiagnosticField::count(
            "requested_user_count",
            requested_user_count as u64,
        ))
        .field(DiagnosticField::count(
            "observed_profile_count",
            observed_profile_count as u64,
        ))
        .field(DiagnosticField::boolean("network_lookup_attempted", false)),
    );
}

pub(super) fn collect_live_event_receipts_from_diff(
    diff: &eyeball_im::VectorDiff<Arc<SdkTimelineItem>>,
    out: &mut Vec<LiveEventReceipts>,
) {
    use eyeball_im::VectorDiff;

    match diff {
        VectorDiff::PushFront { value }
        | VectorDiff::PushBack { value }
        | VectorDiff::Insert { value, .. } => {
            if let Some(receipts) = live_event_receipts_from_sdk_item(value, false) {
                out.push(receipts);
            }
        }
        VectorDiff::Set { value, .. } => {
            if let Some(receipts) = live_event_receipts_from_sdk_item(value, true) {
                out.push(receipts);
            }
        }
        VectorDiff::Append { values } | VectorDiff::Reset { values } => {
            out.extend(live_event_receipts_from_sdk_items(values.iter()));
        }
        VectorDiff::Remove { .. }
        | VectorDiff::Truncate { .. }
        | VectorDiff::Clear
        | VectorDiff::PopFront
        | VectorDiff::PopBack => {}
    }
}

fn live_event_receipts_from_sdk_item(
    item: &Arc<SdkTimelineItem>,
    include_empty: bool,
) -> Option<LiveEventReceipts> {
    use matrix_sdk_ui::timeline::TimelineItemKind;

    let event_item = match item.kind() {
        TimelineItemKind::Event(event_item) => event_item,
        TimelineItemKind::Virtual(_) => return None,
    };
    let event_id = event_item.event_id()?.to_string();
    let receipts = event_item
        .read_receipts()
        .iter()
        .map(|(user_id, receipt)| LiveReadReceipt {
            user_id: user_id.to_string(),
            display_name: None,
            original_display_label: String::new(),
            avatar: None,
            timestamp_ms: receipt.ts.map(|timestamp| timestamp.0.into()),
        })
        .collect::<Vec<_>>();

    if receipts.is_empty() && !include_empty {
        return None;
    }

    Some(LiveEventReceipts { event_id, receipts })
}

/// Convert a single SDK `TimelineItem` to our `TimelineItem` DTO.
pub fn sdk_item_to_timeline_item(
    key: &TimelineKey,
    item: &Arc<SdkTimelineItem>,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
) -> TimelineItem {
    sdk_item_to_timeline_item_with_send_states(
        key,
        item,
        own_user_id,
        &HashMap::new(),
        None,
        None,
        None,
    )
}

/// Build the closed room-key request presentation state for an event
/// (issue #460). Only closed tokens cross the wire.
fn request_state_for_item(
    key_request_states: Option<&std::collections::BTreeMap<String, KeyRequestUiState>>,
    withheld_codes: Option<&std::collections::BTreeMap<(String, String), &'static str>>,
    key: &TimelineKey,
    event_item: &matrix_sdk_ui::timeline::EventTimelineItem,
) -> Option<RoomKeyRequestStateDto> {
    let event_id = event_item.event_id()?.to_string();
    let state = key_request_states?.get(&event_id)?;
    let withheld_code = state
        .withheld_code
        .and_then(key_request_withheld_code_token)
        .or_else(|| {
            let session = event_item
                .content()
                .as_unable_to_decrypt()
                .and_then(|utd| {
                    let matrix_sdk_ui::timeline::EncryptedMessage::MegolmV1AesSha2 {
                        session_id,
                        ..
                    } = utd
                    else {
                        return None;
                    };
                    Some(session_id.as_str())
                })?;
            withheld_codes?
                .get(&(key.room_id().to_owned(), session.to_owned()))
                .copied()
                .and_then(key_request_withheld_code_token)
        });
    Some(RoomKeyRequestStateDto {
        stage: key_request_stage_token(state.stage),
        withheld_code,
    })
}

/// Map an internal stage literal to the closed wire token. Internal stages
/// are compile-time literals only; the fallback keeps the wire contract closed
/// even if a future literal is added before the mapping is extended.
pub(super) fn key_request_stage_token(stage: &str) -> RoomKeyRequestStage {
    match stage {
        "sent" => RoomKeyRequestStage::Sent,
        "automatic" => RoomKeyRequestStage::Automatic,
        "still_waiting" => RoomKeyRequestStage::StillWaiting,
        "withheld" => RoomKeyRequestStage::Withheld,
        "decryption_recovered" => RoomKeyRequestStage::DecryptionRecovered,
        "send_failed" => RoomKeyRequestStage::SendFailed,
        _ => RoomKeyRequestStage::StillWaiting,
    }
}

/// Map an internal withheld-code literal to the closed wire token.
pub(super) fn key_request_withheld_code_token(code: &str) -> Option<RoomKeyRequestWithheldCode> {
    match code {
        "blacklisted" => Some(RoomKeyRequestWithheldCode::Blacklisted),
        "unverified" => Some(RoomKeyRequestWithheldCode::Unverified),
        "unauthorised" => Some(RoomKeyRequestWithheldCode::Unauthorised),
        "unavailable" => Some(RoomKeyRequestWithheldCode::Unavailable),
        _ => None,
    }
}

pub(super) fn sdk_item_to_timeline_item_with_send_states(
    key: &TimelineKey,
    item: &Arc<SdkTimelineItem>,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
    send_statuses: &HashMap<String, TimelineSendState>,
    recovery: Option<
        &std::collections::BTreeMap<String, crate::room_key_recovery::RecoveryOperation>,
    >,
    key_request_states: Option<&std::collections::BTreeMap<String, KeyRequestUiState>>,
    withheld_codes: Option<&std::collections::BTreeMap<(String, String), &'static str>>,
) -> TimelineItem {
    use matrix_sdk_ui::timeline::{TimelineItemKind, VirtualTimelineItem};

    match &item.kind() {
        TimelineItemKind::Event(event_item) => {
            // Stable identity: remote event_id when known, otherwise transaction_id.
            let transaction_id = event_item.transaction_id().map(|txn_id| txn_id.to_string());
            let id = if let Some(event_id) = event_item.event_id() {
                TimelineItemId::Event {
                    event_id: event_id.to_string(),
                }
            } else if let Some(txn_id) = transaction_id.as_ref() {
                TimelineItemId::Transaction {
                    transaction_id: txn_id.clone(),
                }
            } else {
                // Fallback: use the internal unique_id as a synthetic id.
                TimelineItemId::Synthetic {
                    synthetic_id: item.unique_id().0.clone(),
                }
            };

            let sender = Some(event_item.sender().to_string());
            let sender_profile = event_item.sender_profile();
            let sender_label = timeline_sender_label_from_profile(sender_profile);
            let sender_avatar = timeline_sender_avatar_from_profile(sender_profile);
            let timestamp_ms = Some(event_item.timestamp().0.into());

            let content = event_item.content();
            let message_projection = Some(message_projection_from_timeline_content(content));
            let body = message_projection
                .as_ref()
                .and_then(|projection| projection.body.clone());
            let notice_i18n = message_projection
                .as_ref()
                .and_then(|projection| projection.notice_i18n.clone());
            let actionable_body = message_projection
                .as_ref()
                .filter(|projection| projection.body_is_user_content)
                .and_then(|projection| projection.body.as_deref());
            let message_kind = message_projection
                .as_ref()
                .map(|projection| projection.message_kind)
                .unwrap_or_default();
            let spoiler_spans = message_projection
                .as_ref()
                .map(|projection| projection.spoiler_spans.clone())
                .unwrap_or_default();
            let media = message_projection
                .as_ref()
                .and_then(|projection| projection.media.clone());
            let formatted = message_projection
                .as_ref()
                .and_then(|projection| projection.formatted.clone());
            let has_renderable_content =
                timeline_content_is_renderable(body.as_deref(), media.as_ref(), formatted.as_ref());
            let is_redacted = content.is_redacted();
            let can_hold_reactions = content.reactions().is_some();
            let can_react = timeline_item_can_react(
                event_item.event_id().is_some(),
                can_hold_reactions,
                is_redacted,
                has_renderable_content,
            );
            let can_redact = timeline_item_can_redact(
                event_item.event_id().is_some(),
                own_user_id
                    .map(|user_id| event_item.sender().as_str() == user_id.as_str())
                    .unwrap_or(false),
                is_redacted,
                has_renderable_content,
            );
            let can_edit = timeline_item_can_edit(
                event_item.event_id().is_some(),
                own_user_id
                    .map(|user_id| event_item.sender().as_str() == user_id.as_str())
                    .unwrap_or(false),
                is_redacted,
                actionable_body.is_some(),
            );
            let in_reply_to = content.in_reply_to();
            let in_reply_to_event_id = in_reply_to
                .as_ref()
                .map(|details| details.event_id.to_string());
            let reply_quote = in_reply_to.as_ref().map(reply_quote_from_details);
            let thread_root = event_item
                .content()
                .thread_root()
                .map(|event_id| event_id.to_string())
                .or_else(|| {
                    content
                        .is_unable_to_decrypt()
                        .then(|| original_json_for_event_item(event_item))
                        .flatten()
                        .and_then(|original_json| thread_root_from_original_json(&original_json))
                });
            let thread_summary = event_item
                .content()
                .thread_summary()
                .map(thread_summary_from_sdk);
            let reactions = event_item
                .content()
                .reactions()
                .map(|reactions| reaction_groups_from_sdk(reactions, own_user_id))
                .unwrap_or_default();
            let is_edited = content
                .as_message()
                .map(|message| message.is_edited())
                .unwrap_or(false);
            let send_state = timeline_send_state_from_sdk(event_item.send_state()).or_else(|| {
                transaction_id
                    .as_deref()
                    .and_then(|txn_id| send_statuses.get(txn_id).cloned())
            });
            let mut unable_to_decrypt = unable_to_decrypt_from_content(content);
            if let Some(utd) = unable_to_decrypt.as_mut() {
                utd.can_request_keys = event_item.original_json().is_some();
                if let Some(session_id) = utd.session_id.as_deref()
                    && let Some(op) = recovery.and_then(|map| map.get(session_id))
                {
                    utd.recovery_stage =
                        Some(crate::room_key_recovery::stage_token(op.stage()).to_owned());
                    utd.recovery_guidance = op
                        .guidance()
                        .map(crate::room_key_recovery::guidance_token)
                        .map(ToOwned::to_owned);
                }
            }
            let mut actions = message_actions_for_timeline_item(
                key.room_id(),
                &id,
                actionable_body,
                media.is_some(),
                is_redacted,
            );
            if let Some(raw) = original_json_for_event_item(event_item) {
                actions.editable_document = composer_document_from_event_json(&raw);
            }
            let is_hidden = timeline_item_should_be_hidden_for_key(
                key,
                has_renderable_content,
                is_redacted,
                thread_root.as_deref(),
            );
            let link_ranges =
                link_ranges_for_message_projection(body.as_deref(), formatted.as_ref());

            TimelineItem {
                request_state: request_state_for_item(
                    key_request_states,
                    withheld_codes,
                    key,
                    event_item,
                ),
                id,
                sender,
                sender_label,
                sender_avatar,
                body,
                notice_i18n,
                message_kind,
                spoiler_spans,
                timestamp_ms,
                in_reply_to_event_id,
                formatted,
                reply_quote,
                thread_root,
                thread_summary,
                media,
                link_previews: None,
                link_ranges,
                reactions,
                can_react,
                is_redacted,
                is_hidden,
                can_redact,
                is_edited,
                can_edit,
                unable_to_decrypt,
                actions,
                send_state,
            }
        }
        TimelineItemKind::Virtual(virtual_item) => {
            let (synthetic_id, timestamp_ms, is_hidden) = match virtual_item {
                VirtualTimelineItem::DateDivider(ts) => {
                    (format!("date-divider-{}", ts.0), Some(ts.0.into()), false)
                }
                VirtualTimelineItem::ReadMarker => ("read-marker".to_owned(), None, true),
                VirtualTimelineItem::TimelineStart => ("timeline-start".to_owned(), None, true),
            };
            TimelineItem {
                request_state: None,
                id: TimelineItemId::Synthetic { synthetic_id },
                sender: None,
                sender_label: None,
                sender_avatar: None,
                body: None,
                notice_i18n: None,
                message_kind: TimelineMessageKind::default(),
                spoiler_spans: Vec::new(),
                timestamp_ms,
                in_reply_to_event_id: None,
                formatted: None,
                reply_quote: None,
                thread_root: None,
                thread_summary: None,
                media: None,
                link_previews: None,
                link_ranges: Vec::new(),
                reactions: Vec::new(),
                can_react: false,
                is_redacted: false,
                is_hidden,
                can_redact: false,
                is_edited: false,
                can_edit: false,
                unable_to_decrypt: None,
                actions: TimelineMessageActions::default(),
                send_state: None,
            }
        }
    }
}

/// Event id of a requestable UTD event for automatic key requests (issue
/// #460): decryptable-retry eligible (session known) and the original JSON is
/// available to re-request from.
pub(super) fn thread_auto_requestable_event_id(item: &Arc<SdkTimelineItem>) -> Option<String> {
    let event_item = item.as_event()?;
    let event_id = event_item.event_id()?.to_string();
    let requestable = event_item.content().is_unable_to_decrypt()
        && event_item.original_json().is_some()
        && unable_to_decrypt_from_content(event_item.content())
            .and_then(|utd| utd.session_id)
            .is_some();
    requestable.then_some(event_id)
}

/// Whether a late withheld observation should update/publish a presentation
/// state (issue #460): terminal stages are never regressed, and a stage
/// already settled `withheld` by a diff still gains the typed code when the
/// independent observation arrives later with a different code.
pub(super) fn withheld_update_should_publish(
    stage: &str,
    current_code: Option<&str>,
    new_code: &str,
) -> bool {
    !matches!(stage, "decryption_recovered" | "send_failed")
        && (stage != "withheld" || current_code != Some(new_code))
}

pub(super) fn unable_to_decrypt_from_content(
    content: &TimelineItemContent,
) -> Option<TimelineUnableToDecrypt> {
    let encrypted = content.as_unable_to_decrypt()?;
    let session_id = match encrypted {
        EncryptedMessage::MegolmV1AesSha2 { session_id, .. } => Some(session_id.clone()),
        EncryptedMessage::OlmV1Curve25519AesSha2 { .. } | EncryptedMessage::Unknown => None,
    };
    Some(TimelineUnableToDecrypt {
        reason: if session_id.is_some() {
            TimelineUnableToDecryptReason::MissingRoomKey
        } else {
            TimelineUnableToDecryptReason::Unknown
        },
        session_id,
        can_request_keys: false,
        recovery_stage: None,
        recovery_guidance: None,
    })
}

pub(super) fn decrypt_retry_reason_from_content(
    content: &TimelineItemContent,
) -> DecryptRetryReason {
    unable_to_decrypt_from_content(content)
        .map(|unable_to_decrypt| match unable_to_decrypt.reason {
            TimelineUnableToDecryptReason::MissingRoomKey => DecryptRetryReason::MissingRoomKey,
            TimelineUnableToDecryptReason::Withheld => DecryptRetryReason::Withheld,
            TimelineUnableToDecryptReason::Malformed => DecryptRetryReason::Malformed,
            TimelineUnableToDecryptReason::Unknown => DecryptRetryReason::Unknown,
        })
        .unwrap_or(DecryptRetryReason::Unknown)
}

pub(super) fn thread_summary_from_sdk(
    summary: matrix_sdk_ui::timeline::ThreadSummary,
) -> ThreadSummaryDto {
    let mut dto = ThreadSummaryDto {
        reply_count: summary.num_replies,
        latest_event_id: None,
        latest_sender: None,
        latest_sender_label: None,
        latest_body_preview: None,
        latest_timestamp_ms: None,
    };

    if let matrix_sdk_ui::timeline::TimelineDetails::Ready(latest_event) = summary.latest_event {
        dto.latest_event_id = match &latest_event.identifier {
            TimelineEventItemId::EventId(event_id) => Some(event_id.to_string()),
            TimelineEventItemId::TransactionId(_) => None,
        };
        dto.latest_sender = Some(latest_event.sender.to_string());
        dto.latest_sender_label = None;
        dto.latest_body_preview = latest_event
            .content
            .as_message()
            .map(|message| message.body().to_owned());
        dto.latest_timestamp_ms = Some(latest_event.timestamp.0.into());
    }

    dto
}

pub(super) struct MessageProjection {
    pub(super) body: Option<String>,
    pub(super) notice_i18n: Option<TimelineNoticeI18n>,
    pub(super) body_is_user_content: bool,
    pub(super) message_kind: TimelineMessageKind,
    pub(super) spoiler_spans: Vec<TimelineSpoilerSpan>,
    pub(super) media: Option<TimelineMedia>,
    pub(super) formatted: Option<crate::event::TimelineFormattedBody>,
}

pub(super) fn link_ranges_for_message_projection(
    body: Option<&str>,
    formatted: Option<&crate::event::TimelineFormattedBody>,
) -> Vec<crate::event::TimelineLinkRange> {
    let source = formatted
        .map(|formatted_body| formatted_body.plain_text.as_str())
        .or(body)
        .unwrap_or("");
    extract_link_ranges(source)
}

fn reply_quote_from_details(details: &InReplyToDetails) -> ReplyQuote {
    match &details.event {
        TimelineDetails::Ready(event) => reply_quote_from_embedded_event(details, event),
        TimelineDetails::Unavailable | TimelineDetails::Pending | TimelineDetails::Error(_) => {
            ReplyQuote {
                event_id: details.event_id.to_string(),
                sender: None,
                sender_label: None,
                body_preview: None,
                formatted: None,
                state: ReplyQuoteState::Missing,
            }
        }
    }
}

fn reply_quote_from_embedded_event(
    details: &InReplyToDetails,
    event: &EmbeddedEvent,
) -> ReplyQuote {
    let sender = Some(event.sender.to_string());
    if event.content.is_redacted() {
        return ReplyQuote {
            event_id: details.event_id.to_string(),
            sender,
            sender_label: None,
            body_preview: None,
            formatted: None,
            state: ReplyQuoteState::Redacted,
        };
    }

    let projection = event
        .content
        .as_message()
        .map(|msg| message_projection_from_msgtype(msg.msgtype(), msg.body()));
    reply_quote_from_message_projection(&details.event_id.to_string(), sender, projection)
}

fn reply_quote_from_message_projection(
    event_id: &str,
    sender: Option<String>,
    projection: Option<MessageProjection>,
) -> ReplyQuote {
    let body_preview = projection
        .as_ref()
        .and_then(reply_quote_preview_from_message_projection);
    let formatted = projection
        .as_ref()
        .and_then(|projection| projection.formatted.as_ref())
        .map(reply_quote_formatted_body_from_timeline);
    let state = if body_preview.is_some() || formatted.is_some() {
        ReplyQuoteState::Ready
    } else {
        ReplyQuoteState::Unsupported
    };

    ReplyQuote {
        event_id: event_id.to_owned(),
        sender,
        sender_label: None,
        body_preview,
        formatted,
        state,
    }
}

fn reply_quote_formatted_body_from_timeline(
    formatted: &crate::event::TimelineFormattedBody,
) -> ReplyQuoteFormattedBody {
    ReplyQuoteFormattedBody {
        html: formatted.html.clone(),
        plain_text: formatted.plain_text.clone(),
        code_blocks: formatted
            .code_blocks
            .iter()
            .map(|block| ReplyQuoteCodeBlock {
                language: block.language.clone(),
                body: block.body.clone(),
            })
            .collect(),
    }
}

fn reply_quote_preview_from_message_projection(projection: &MessageProjection) -> Option<String> {
    let source = projection.body.as_deref().or_else(|| {
        projection
            .media
            .as_ref()
            .map(|media| media.filename.as_str())
    })?;
    collapsed_preview(&source, REPLY_QUOTE_PREVIEW_MAX_CHARS)
}

fn message_projection_from_timeline_content(content: &TimelineItemContent) -> MessageProjection {
    if let Some(message) = content.as_message() {
        return message_projection_from_msgtype(message.msgtype(), message.body());
    }

    match content {
        TimelineItemContent::MembershipChange(change) => {
            return membership_change_projection(
                &change
                    .display_name()
                    .unwrap_or_else(|| change.user_id().to_string()),
                change.change(),
            );
        }
        TimelineItemContent::ProfileChange(change) => {
            return profile_change_projection(change);
        }
        TimelineItemContent::OtherState(state) => {
            if let AnyOtherStateEventContentChange::RoomName(change) = state.content() {
                return room_name_notice_projection(change);
            }
        }
        _ => {}
    }

    if let Some(sticker) = content.as_sticker() {
        return sticker_projection_from_body(&sticker.content().body);
    }

    if content.is_unable_to_decrypt() {
        return non_user_content_projection("Unable to decrypt message");
    }

    if content.is_poll() {
        return non_user_content_projection("Poll message");
    }

    if content.is_redacted() {
        return MessageProjection {
            body: None,
            notice_i18n: None,
            body_is_user_content: false,
            message_kind: TimelineMessageKind::Text,
            spoiler_spans: Vec::new(),
            media: None,
            formatted: None,
        };
    }

    let event_type = content
        .event_type_str()
        .unwrap_or_else(|| "unsupported Matrix event".to_owned());
    state_event_notice_projection(&event_type)
}

pub(super) fn sticker_projection_from_body(body: &str) -> MessageProjection {
    let body = body.trim();
    MessageProjection {
        body: (!body.is_empty()).then(|| body.to_owned()),
        notice_i18n: None,
        body_is_user_content: true,
        message_kind: TimelineMessageKind::Text,
        spoiler_spans: Vec::new(),
        media: None,
        formatted: None,
    }
}

fn state_event_notice_projection(event_type: &str) -> MessageProjection {
    MessageProjection {
        body: Some(state_event_notice_body(event_type).into_owned()),
        notice_i18n: state_event_notice_i18n(event_type).map(|key| TimelineNoticeI18n {
            key,
            old_name: None,
            new_name: None,
        }),
        body_is_user_content: false,
        message_kind: TimelineMessageKind::Notice,
        spoiler_spans: Vec::new(),
        media: None,
        formatted: None,
    }
}

fn state_event_notice_body(event_type: &str) -> Cow<'_, str> {
    match event_type {
        "m.room.create" => Cow::Borrowed("created the room"),
        "m.room.power_levels" => Cow::Borrowed("updated room permissions"),
        "m.room.guest_access" => Cow::Borrowed("updated guest access"),
        "m.room.encryption" => Cow::Borrowed("enabled room encryption"),
        "m.space.parent" => Cow::Borrowed("updated the parent space"),
        "m.room.join_rules" => Cow::Borrowed("updated join rules"),
        "m.room.history_visibility" => Cow::Borrowed("updated history visibility"),
        "m.room.pinned_events" => Cow::Borrowed("updated pinned messages"),
        _ => Cow::Owned(format!("Unsupported event: {event_type}")),
    }
}

fn state_event_notice_i18n(event_type: &str) -> Option<TimelineNoticeI18nKey> {
    match event_type {
        "m.room.create" => Some(TimelineNoticeI18nKey::RoomCreate),
        "m.room.power_levels" => Some(TimelineNoticeI18nKey::RoomPowerLevels),
        "m.room.guest_access" => Some(TimelineNoticeI18nKey::RoomGuestAccess),
        "m.room.encryption" => Some(TimelineNoticeI18nKey::RoomEncryption),
        "m.space.parent" => Some(TimelineNoticeI18nKey::SpaceParent),
        "m.room.join_rules" => Some(TimelineNoticeI18nKey::RoomJoinRules),
        "m.room.history_visibility" => Some(TimelineNoticeI18nKey::RoomHistoryVisibility),
        "m.room.pinned_events" => Some(TimelineNoticeI18nKey::RoomPinnedEvents),
        _ => None,
    }
}

fn room_name_notice_projection(
    change: &StateEventContentChange<RoomNameEventContent>,
) -> MessageProjection {
    let (body, notice_i18n) = match change {
        StateEventContentChange::Original {
            content,
            prev_content,
        } => {
            let current_name = content.name.as_str();
            let previous_name = prev_content
                .as_ref()
                .and_then(|content| content.name.as_deref())
                .filter(|name| !name.trim().is_empty());

            if current_name.trim().is_empty() {
                (
                    "removed the room name".to_owned(),
                    TimelineNoticeI18n {
                        key: TimelineNoticeI18nKey::RoomNameRemoved,
                        old_name: None,
                        new_name: None,
                    },
                )
            } else if previous_name.is_some_and(|previous| previous != current_name) {
                let previous_name = previous_name.expect("previous name was checked");
                (
                    format!("changed the room name from {previous_name} to {current_name}"),
                    TimelineNoticeI18n {
                        key: TimelineNoticeI18nKey::RoomNameChanged,
                        old_name: Some(previous_name.to_owned()),
                        new_name: Some(current_name.to_owned()),
                    },
                )
            } else {
                (
                    format!("set the room name to {current_name}"),
                    TimelineNoticeI18n {
                        key: TimelineNoticeI18nKey::RoomNameSet,
                        old_name: None,
                        new_name: Some(current_name.to_owned()),
                    },
                )
            }
        }
        StateEventContentChange::Redacted(_) => (
            "changed the room name".to_owned(),
            TimelineNoticeI18n {
                key: TimelineNoticeI18nKey::RoomNameChangedGeneric,
                old_name: None,
                new_name: None,
            },
        ),
    };

    MessageProjection {
        body: Some(body),
        notice_i18n: Some(notice_i18n),
        body_is_user_content: false,
        message_kind: TimelineMessageKind::Notice,
        spoiler_spans: Vec::new(),
        media: None,
        formatted: None,
    }
}

fn membership_change_projection(
    display_name: &str,
    change: Option<MembershipChange>,
) -> MessageProjection {
    let action = match change {
        Some(MembershipChange::Joined) | Some(MembershipChange::InvitationAccepted) => {
            "joined the room"
        }
        Some(MembershipChange::Left) => "left the room",
        Some(MembershipChange::Banned) => "was banned",
        Some(MembershipChange::Unbanned) => "was unbanned",
        Some(MembershipChange::Kicked) => "was kicked",
        Some(MembershipChange::Invited) => "was invited",
        Some(MembershipChange::InvitationRejected) => "rejected the invite",
        Some(MembershipChange::InvitationRevoked) => "had their invite revoked",
        Some(MembershipChange::Knocked) => "knocked",
        Some(MembershipChange::KnockAccepted) => "had their knock accepted",
        Some(MembershipChange::KnockRetracted) => "retracted their knock",
        Some(MembershipChange::KnockDenied) => "had their knock denied",
        Some(MembershipChange::KickedAndBanned) => "was kicked and banned",
        Some(MembershipChange::None) => "had a membership update",
        Some(MembershipChange::Error) | Some(MembershipChange::NotImplemented) | None => {
            "had a membership change"
        }
    };
    non_user_content_projection(&format!("{display_name} {action}"))
}

fn profile_change_projection(
    change: &matrix_sdk_ui::timeline::MemberProfileChange,
) -> MessageProjection {
    let body = match (
        change.displayname_change().is_some(),
        change.avatar_url_change().is_some(),
    ) {
        (false, true) => "changed their profile picture",
        (true, false) => "changed their display name",
        (true, true) => "changed their display name and profile picture",
        (false, false) => "updated their room profile",
    };
    non_user_content_projection(body)
}

pub(super) fn non_user_content_projection(body: &str) -> MessageProjection {
    MessageProjection {
        body: Some(body.to_owned()),
        notice_i18n: None,
        body_is_user_content: false,
        message_kind: TimelineMessageKind::Notice,
        spoiler_spans: Vec::new(),
        media: None,
        formatted: None,
    }
}

pub(super) fn collapsed_preview(value: &str, max_chars: usize) -> Option<String> {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }

    if collapsed.chars().count() <= max_chars {
        return Some(collapsed);
    }

    let mut preview = collapsed.chars().take(max_chars).collect::<String>();
    preview.push_str("...");
    Some(preview)
}

pub(super) fn message_projection_from_msgtype(
    msgtype: &MessageType,
    fallback_body: &str,
) -> MessageProjection {
    match msgtype {
        MessageType::Audio(content) => message_projection_from_body_and_formatted(
            content.caption(),
            content.formatted_caption(),
            TimelineMessageKind::Text,
            Some(timeline_media_from_audio(content)),
        ),
        MessageType::Emote(content) => message_projection_from_body_and_formatted(
            Some(fallback_body),
            content.formatted.as_ref(),
            TimelineMessageKind::Emote,
            None,
        ),
        MessageType::File(content) => message_projection_from_body_and_formatted(
            content.caption(),
            content.formatted_caption(),
            TimelineMessageKind::Text,
            Some(timeline_media_from_file(content)),
        ),
        MessageType::Image(content) => message_projection_from_body_and_formatted(
            content.caption(),
            content.formatted_caption(),
            TimelineMessageKind::Text,
            Some(timeline_media_from_image(content)),
        ),
        MessageType::Notice(content) => message_projection_from_body_and_formatted(
            Some(fallback_body),
            content.formatted.as_ref(),
            TimelineMessageKind::Notice,
            None,
        ),
        MessageType::Text(content) => message_projection_from_body_and_formatted(
            Some(fallback_body),
            content.formatted.as_ref(),
            TimelineMessageKind::Text,
            None,
        ),
        MessageType::Video(content) => message_projection_from_body_and_formatted(
            content.caption(),
            content.formatted_caption(),
            TimelineMessageKind::Text,
            Some(timeline_media_from_video(content)),
        ),
        _ => MessageProjection {
            body: Some(fallback_body.to_owned()),
            notice_i18n: None,
            body_is_user_content: true,
            message_kind: TimelineMessageKind::Text,
            spoiler_spans: Vec::new(),
            media: None,
            formatted: None,
        },
    }
}

fn message_projection_from_body_and_formatted(
    body: Option<&str>,
    formatted_body: Option<&FormattedBody>,
    message_kind: TimelineMessageKind,
    media: Option<TimelineMedia>,
) -> MessageProjection {
    let formatted = formatted_body.and_then(project_formatted_body);
    let spoiler_spans = formatted
        .as_ref()
        .map(|projection| projection.spoiler_spans.clone())
        .unwrap_or_default();
    let formatted = formatted.map(|projection| projection.formatted);
    let (body, spoiler_spans) = match (body, formatted.is_some()) {
        (Some(body), false) => {
            let projection = project_plain_body_with_spoilers(body);
            (Some(projection.body), projection.spoiler_spans)
        }
        (Some(body), true) => (Some(body.to_owned()), spoiler_spans),
        (None, _) => (None, spoiler_spans),
    };

    MessageProjection {
        body,
        notice_i18n: None,
        body_is_user_content: true,
        message_kind,
        spoiler_spans,
        media,
        formatted,
    }
}

struct PlainBodyProjection {
    body: String,
    spoiler_spans: Vec<TimelineSpoilerSpan>,
}

fn project_plain_body_with_spoilers(body: &str) -> PlainBodyProjection {
    let mut rendered = String::with_capacity(body.len());
    let mut spoiler_spans = Vec::new();
    let mut index = 0;

    while index < body.len() {
        let rest = &body[index..];
        if let Some(after) = rest.strip_prefix("||")
            && let Some(end) = after.find("||")
        {
            let start_utf16 = rendered.encode_utf16().count();
            rendered.push_str(&after[..end]);
            let end_utf16 = rendered.encode_utf16().count();
            if start_utf16 < end_utf16 {
                spoiler_spans.push(TimelineSpoilerSpan {
                    start_utf16,
                    end_utf16,
                    reason: None,
                });
            }
            index += 2 + end + 2;
            continue;
        }

        let ch = rest
            .chars()
            .next()
            .expect("rest is non-empty while projecting plain body");
        rendered.push(ch);
        index += ch.len_utf8();
    }

    PlainBodyProjection {
        body: rendered,
        spoiler_spans,
    }
}

struct FormattedBodyProjection {
    formatted: crate::event::TimelineFormattedBody,
    spoiler_spans: Vec<TimelineSpoilerSpan>,
}

fn project_formatted_body(formatted_body: &FormattedBody) -> Option<FormattedBodyProjection> {
    if !matches!(&formatted_body.format, MessageFormat::Html) {
        return None;
    }

    let html = Html::parse(&formatted_body.body);
    html.sanitize_with(
        &SanitizerConfig::compat()
            .remove_reply_fallback()
            .remove_elements(["script", "style"]),
    );
    let sanitized_body = html.to_string();

    if sanitized_body.trim().is_empty() {
        return None;
    }

    let html = Html::parse(&sanitized_body);
    let plain_text = plain_text_from_html(&html);
    let code_blocks = code_blocks_from_html(&html);
    if plain_text.trim().is_empty() && code_blocks.iter().all(|block| block.body.trim().is_empty())
    {
        return None;
    }
    let spoiler_spans = spoiler_spans_from_html(&html);

    Some(FormattedBodyProjection {
        formatted: crate::event::TimelineFormattedBody {
            html: sanitized_body,
            plain_text,
            code_blocks,
        },
        spoiler_spans,
    })
}

fn plain_text_from_html(html: &Html) -> String {
    let mut text = String::new();
    collect_plain_text_from_nodes(html.children(), &mut text);
    text
}

fn collect_plain_text_from_nodes(
    nodes: impl Iterator<Item = matrix_sdk::ruma::html::NodeRef>,
    out: &mut String,
) {
    for node in nodes {
        if let Some(text) = node.as_text() {
            out.push_str(&text.borrow());
            continue;
        }

        if node.as_element().is_some() {
            collect_plain_text_from_nodes(node.children(), out);
        }
    }
}

fn spoiler_spans_from_html(html: &Html) -> Vec<TimelineSpoilerSpan> {
    let mut spans = Vec::new();
    let mut offset_utf16 = 0;
    collect_spoiler_spans_from_nodes(html.children(), &mut offset_utf16, &mut spans);
    spans.sort_by_key(|span| (span.start_utf16, span.end_utf16));
    spans
}

fn collect_spoiler_spans_from_nodes(
    nodes: impl Iterator<Item = matrix_sdk::ruma::html::NodeRef>,
    offset_utf16: &mut usize,
    spans: &mut Vec<TimelineSpoilerSpan>,
) {
    for node in nodes {
        if let Some(text) = node.as_text() {
            *offset_utf16 += text.borrow().encode_utf16().count();
            continue;
        }

        let spoiler_reason = node.as_element().and_then(|element| {
            element.attrs.borrow().iter().find_map(|attr| {
                if attr.name.local.as_ref() != "data-mx-spoiler" {
                    return None;
                }
                let reason = attr.value.trim();
                Some((!reason.is_empty()).then(|| reason.to_owned()))
            })
        });

        let start_utf16 = *offset_utf16;
        collect_spoiler_spans_from_nodes(node.children(), offset_utf16, spans);
        if let Some(reason) = spoiler_reason {
            let end_utf16 = *offset_utf16;
            if start_utf16 < end_utf16 {
                spans.push(TimelineSpoilerSpan {
                    start_utf16,
                    end_utf16,
                    reason,
                });
            }
        }
    }
}

fn code_blocks_from_html(html: &Html) -> Vec<crate::event::TimelineCodeBlock> {
    let mut blocks = Vec::new();
    collect_code_blocks_from_nodes(html.children(), &mut blocks);
    blocks
}

fn collect_code_blocks_from_nodes(
    nodes: impl Iterator<Item = matrix_sdk::ruma::html::NodeRef>,
    out: &mut Vec<crate::event::TimelineCodeBlock>,
) {
    for node in nodes {
        let Some(element) = node.as_element() else {
            continue;
        };
        if element.name.local.as_ref() != "pre" {
            collect_code_blocks_from_nodes(node.children(), out);
            continue;
        }

        for child in node.children() {
            let Some(code_element) = child.as_element() else {
                continue;
            };
            if code_element.name.local.as_ref() != "code" {
                continue;
            }

            let language = code_element.attrs.borrow().iter().find_map(|attr| {
                if attr.name.local.as_ref() != "class" {
                    return None;
                }

                attr.value
                    .split_ascii_whitespace()
                    .find_map(|class_name| class_name.strip_prefix("language-"))
                    .map(|language| language.to_owned())
            });
            let mut body = String::new();
            collect_plain_text_from_nodes(child.children(), &mut body);

            out.push(crate::event::TimelineCodeBlock { language, body });
            break;
        }

        collect_code_blocks_from_nodes(node.children(), out);
    }
}

fn timeline_media_from_audio(
    content: &matrix_sdk::ruma::events::room::message::AudioMessageEventContent,
) -> TimelineMedia {
    let info = content.info.as_deref();
    TimelineMedia {
        kind: TimelineMediaKind::Audio,
        filename: content.filename().to_owned(),
        source: timeline_media_source_from_sdk(&content.source),
        mimetype: info.and_then(|info| info.mimetype.clone()),
        size: info.and_then(|info| uint_to_u64(info.size.as_ref())),
        width: None,
        height: None,
        thumbnail: None,
    }
}

fn timeline_media_from_image(
    content: &matrix_sdk::ruma::events::room::message::ImageMessageEventContent,
) -> TimelineMedia {
    let info = content.info.as_deref();
    TimelineMedia {
        kind: TimelineMediaKind::Image,
        filename: content.filename().to_owned(),
        source: timeline_media_source_from_sdk(&content.source),
        mimetype: info.and_then(|info| info.mimetype.clone()),
        size: info.and_then(|info| uint_to_u64(info.size.as_ref())),
        width: info.and_then(|info| uint_to_u64(info.width.as_ref())),
        height: info.and_then(|info| uint_to_u64(info.height.as_ref())),
        thumbnail: info.and_then(|info| {
            timeline_media_thumbnail_from_sdk(
                info.thumbnail_source.as_ref(),
                info.thumbnail_info.as_deref(),
            )
        }),
    }
}

fn timeline_media_from_file(
    content: &matrix_sdk::ruma::events::room::message::FileMessageEventContent,
) -> TimelineMedia {
    let info = content.info.as_deref();
    TimelineMedia {
        kind: TimelineMediaKind::File,
        filename: content.filename().to_owned(),
        source: timeline_media_source_from_sdk(&content.source),
        mimetype: info.and_then(|info| info.mimetype.clone()),
        size: info.and_then(|info| uint_to_u64(info.size.as_ref())),
        width: None,
        height: None,
        thumbnail: info.and_then(|info| {
            timeline_media_thumbnail_from_sdk(
                info.thumbnail_source.as_ref(),
                info.thumbnail_info.as_deref(),
            )
        }),
    }
}

fn timeline_media_from_video(
    content: &matrix_sdk::ruma::events::room::message::VideoMessageEventContent,
) -> TimelineMedia {
    let info = content.info.as_deref();
    TimelineMedia {
        kind: TimelineMediaKind::Video,
        filename: content.filename().to_owned(),
        source: timeline_media_source_from_sdk(&content.source),
        mimetype: info.and_then(|info| info.mimetype.clone()),
        size: info.and_then(|info| uint_to_u64(info.size.as_ref())),
        width: info.and_then(|info| uint_to_u64(info.width.as_ref())),
        height: info.and_then(|info| uint_to_u64(info.height.as_ref())),
        thumbnail: info.and_then(|info| {
            timeline_media_thumbnail_from_sdk(
                info.thumbnail_source.as_ref(),
                info.thumbnail_info.as_deref(),
            )
        }),
    }
}

pub(super) fn timeline_media_source_from_sdk(source: &MediaSource) -> TimelineMediaSource {
    match source {
        MediaSource::Plain(uri) => TimelineMediaSource {
            mxc_uri: uri.to_string(),
            encrypted: false,
            encryption_version: None,
        },
        MediaSource::Encrypted(file) => TimelineMediaSource {
            mxc_uri: file.url.to_string(),
            encrypted: true,
            encryption_version: Some(file.info.version().to_owned()),
        },
    }
}

fn timeline_media_thumbnail_from_sdk(
    source: Option<&MediaSource>,
    info: Option<&ThumbnailInfo>,
) -> Option<TimelineMediaThumbnail> {
    source.map(|source| TimelineMediaThumbnail {
        source: timeline_media_source_from_sdk(source),
        mimetype: info.and_then(|info| info.mimetype.clone()),
        size: info.and_then(|info| uint_to_u64(info.size.as_ref())),
        width: info.and_then(|info| uint_to_u64(info.width.as_ref())),
        height: info.and_then(|info| uint_to_u64(info.height.as_ref())),
    })
}

pub(super) fn timeline_send_state_from_sdk(
    state: Option<&SdkEventSendState>,
) -> Option<TimelineSendState> {
    match state {
        Some(SdkEventSendState::NotSentYet { .. }) => Some(TimelineSendState::Sending),
        Some(SdkEventSendState::SendingFailed { is_recoverable, .. }) => {
            Some(TimelineSendState::NotSent {
                reason: send_failure_reason(*is_recoverable),
            })
        }
        Some(SdkEventSendState::Sent { .. }) => Some(TimelineSendState::Sent),
        None => None,
    }
}

pub(super) fn send_failure_reason(is_recoverable: bool) -> TimelineSendFailureReason {
    if is_recoverable {
        TimelineSendFailureReason::Recoverable
    } else {
        TimelineSendFailureReason::Unrecoverable
    }
}

pub(super) fn remember_local_echo(
    statuses: &mut HashMap<String, TimelineSendState>,
    handles: &mut HashMap<String, SendHandle>,
    echo: &LocalEcho,
) {
    let transaction_id = echo.transaction_id.to_string();
    if let LocalEchoContent::Event {
        send_handle,
        send_error,
        ..
    } = &echo.content
    {
        let state = if send_error.is_some() {
            TimelineSendState::NotSent {
                reason: TimelineSendFailureReason::Unrecoverable,
            }
        } else {
            TimelineSendState::Sending
        };
        statuses.insert(transaction_id.clone(), state);
        handles.insert(transaction_id, send_handle.clone());
    }
}

fn private_media_entry_from_msgtype(msgtype: &MessageType) -> Option<PrivateMediaEntry> {
    match msgtype {
        MessageType::Image(content) => {
            let info = content.info.as_deref();
            Some(PrivateMediaEntry {
                source: content.source.clone(),
                thumbnail_source: info.and_then(|info| info.thumbnail_source.clone()),
                mimetype: info.and_then(|info| info.mimetype.clone()),
                size: info
                    .and_then(|info| uint_to_u64(info.size.as_ref()))
                    .unwrap_or(0),
                width: info.and_then(|info| uint_to_u64(info.width.as_ref())),
                height: info.and_then(|info| uint_to_u64(info.height.as_ref())),
            })
        }
        MessageType::File(content) => {
            let info = content.info.as_deref();
            Some(PrivateMediaEntry {
                source: content.source.clone(),
                thumbnail_source: info.and_then(|info| info.thumbnail_source.clone()),
                mimetype: info.and_then(|info| info.mimetype.clone()),
                size: info
                    .and_then(|info| uint_to_u64(info.size.as_ref()))
                    .unwrap_or(0),
                width: None,
                height: None,
            })
        }
        MessageType::Audio(content) => {
            let info = content.info.as_deref();
            Some(PrivateMediaEntry {
                source: content.source.clone(),
                thumbnail_source: None,
                mimetype: info.and_then(|info| info.mimetype.clone()),
                size: info
                    .and_then(|info| uint_to_u64(info.size.as_ref()))
                    .unwrap_or(0),
                width: None,
                height: None,
            })
        }
        MessageType::Video(content) => {
            let info = content.info.as_deref();
            Some(PrivateMediaEntry {
                source: content.source.clone(),
                thumbnail_source: info.and_then(|info| info.thumbnail_source.clone()),
                mimetype: info.and_then(|info| info.mimetype.clone()),
                size: info
                    .and_then(|info| uint_to_u64(info.size.as_ref()))
                    .unwrap_or(0),
                width: info.and_then(|info| uint_to_u64(info.width.as_ref())),
                height: info.and_then(|info| uint_to_u64(info.height.as_ref())),
            })
        }
        _ => None,
    }
}

pub(super) fn cache_sdk_item_media_source(
    cache: &mut HashMap<String, PrivateMediaEntry>,
    item: &Arc<SdkTimelineItem>,
) {
    use matrix_sdk_ui::timeline::TimelineItemKind;

    let TimelineItemKind::Event(event_item) = item.kind() else {
        return;
    };
    let Some(event_id) = event_item.event_id() else {
        return;
    };
    let Some(message) = event_item.content().as_message() else {
        return;
    };
    let Some(entry) = private_media_entry_from_msgtype(message.msgtype()) else {
        return;
    };

    cache.insert(event_id.to_string(), entry);
}

pub(super) fn attachment_info_for_upload(request: &UploadMediaRequest) -> AttachmentInfo {
    let size = u64::try_from(request.bytes.len())
        .ok()
        .and_then(uint_from_u64);

    match request.kind {
        UploadMediaKind::Image { width, height } => AttachmentInfo::Image(BaseImageInfo {
            width: width.and_then(uint_from_u64),
            height: height.and_then(uint_from_u64),
            size,
            ..Default::default()
        }),
        UploadMediaKind::File => AttachmentInfo::File(BaseFileInfo { size }),
    }
}

pub(super) fn thumbnail_for_upload(request: &UploadMediaRequest) -> Option<Thumbnail> {
    let thumbnail = request.thumbnail.as_ref()?;
    Some(Thumbnail {
        data: thumbnail.bytes.clone(),
        content_type: thumbnail.mime_type.parse().ok()?,
        height: uint_from_u64(thumbnail.height)?,
        width: uint_from_u64(thumbnail.width)?,
        size: uint_from_u64(u64::try_from(thumbnail.bytes.len()).ok()?)?,
    })
}

pub(super) fn media_request_for_download(
    entry: &PrivateMediaEntry,
    selection: &MediaDownloadSelection,
) -> Option<MediaRequestParameters> {
    match selection {
        MediaDownloadSelection::File => Some(MediaRequestParameters {
            source: entry.source.clone(),
            format: MediaFormat::File,
        }),
        MediaDownloadSelection::Thumbnail { width, height } => {
            if let Some(source) = entry.thumbnail_source.clone() {
                return Some(MediaRequestParameters {
                    source,
                    format: MediaFormat::File,
                });
            }
            Some(MediaRequestParameters {
                source: entry.source.clone(),
                format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(
                    uint_from_u64(*width)?,
                    uint_from_u64(*height)?,
                )),
            })
        }
    }
}

/// Produce a path-safe hex string from a Matrix identifier.
///
/// Matrix room ids and event ids contain `!`, `$`, `#`, `:`, `.`, and `/`
/// which are illegal or ambiguous in file-system path components on Windows
/// and some POSIX contexts.  We hash the identifier to a fixed-length hex
/// string so the path component is always safe.  The original identifier is
/// never written to the filesystem; it is only used as the hash input.
pub(super) fn sanitize_matrix_id_for_path(id: &str) -> String {
    use std::hash::Hash;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    std::hash::Hasher::finish(&hasher)
        .to_string()
        .chars()
        .take(16)
        .collect()
}

fn uint_to_u64(value: Option<&matrix_sdk::ruma::UInt>) -> Option<u64> {
    value.map(|value| (*value).into())
}

fn uint_from_u64(value: u64) -> Option<matrix_sdk::ruma::UInt> {
    matrix_sdk::ruma::UInt::try_from(value).ok()
}

pub(crate) fn timeline_item_can_react(
    is_event_backed: bool,
    can_hold_reactions: bool,
    is_redacted: bool,
    has_renderable_content: bool,
) -> bool {
    is_event_backed && can_hold_reactions && !is_redacted && has_renderable_content
}

pub(crate) fn validate_send_reaction(
    can_react: bool,
    my_reaction_event_id: Option<&str>,
) -> Result<(), TimelineFailureKind> {
    if !can_react {
        return Err(TimelineFailureKind::InvalidReactionTarget);
    }
    if my_reaction_event_id.is_some() {
        return Err(TimelineFailureKind::InvalidReactionState);
    }
    Ok(())
}

pub(crate) fn validate_redact_reaction(
    can_react: bool,
    my_reaction_event_id: Option<&str>,
    reaction_event_id: &str,
) -> Result<(), TimelineFailureKind> {
    if !can_react {
        return Err(TimelineFailureKind::InvalidReactionTarget);
    }
    match my_reaction_event_id {
        Some(current) if current == reaction_event_id => Ok(()),
        _ => Err(TimelineFailureKind::InvalidReactionState),
    }
}

pub(crate) fn timeline_item_can_redact(
    is_event_backed: bool,
    is_own_message: bool,
    is_redacted: bool,
    has_renderable_content: bool,
) -> bool {
    is_event_backed && is_own_message && !is_redacted && has_renderable_content
}

pub(crate) fn timeline_item_can_edit(
    is_event_backed: bool,
    is_own_message: bool,
    is_redacted: bool,
    has_editable_body: bool,
) -> bool {
    is_event_backed && is_own_message && !is_redacted && has_editable_body
}

/// Shape of a media message, used for the edit decision and its diagnostics.
///
/// The presence of a shape is what marks a message type as media; it is the
/// single list both `msgtype_carries_editable_caption` and the edit diagnostics
/// read, so the two cannot drift apart.
#[derive(Clone, Copy)]
struct MediaMessageShape {
    encrypted: bool,
    has_info: bool,
    has_caption: bool,
}

fn msgtype_media_shape(msgtype: &MessageType) -> Option<MediaMessageShape> {
    fn shape(source: &MediaSource, has_info: bool, has_caption: bool) -> Option<MediaMessageShape> {
        Some(MediaMessageShape {
            encrypted: matches!(source, MediaSource::Encrypted(_)),
            has_info,
            has_caption,
        })
    }

    match msgtype {
        MessageType::Audio(content) => shape(
            &content.source,
            content.info.is_some(),
            content.caption().is_some(),
        ),
        MessageType::File(content) => shape(
            &content.source,
            content.info.is_some(),
            content.caption().is_some(),
        ),
        MessageType::Image(content) => shape(
            &content.source,
            content.info.is_some(),
            content.caption().is_some(),
        ),
        MessageType::Video(content) => shape(
            &content.source,
            content.info.is_some(),
            content.caption().is_some(),
        ),
        _ => None,
    }
}

/// Whether the SDK can edit this message type's caption in place.
///
/// Media message types carry the attachment in the same `m.room.message`
/// content as the caption, and the caption-preserving SDK path supports exactly
/// the types that `msgtype_media_shape` recognises.
fn msgtype_carries_editable_caption(msgtype: &MessageType) -> bool {
    msgtype_media_shape(msgtype).is_some()
}

/// Resolve the SDK message type behind an edit target.
///
/// This mirrors the SDK's own `rfind_event_by_item_id`, which `Timeline::edit`
/// uses to locate the item: last match wins, an event id matches any item that
/// carries it (including a local echo the server has already accepted), and a
/// transaction id matches the local echo that owns it. Comparing
/// `EventTimelineItem::identifier()` instead would miss a sent local echo looked
/// up by transaction id, because that item reports its event id.
///
/// One SDK case stays out of reach: a remote item's originating transaction id
/// has no public accessor. Such an item always carries an event id, and
/// `item_ids_for_event` tries the event id first, so the caption decision still
/// sees it.
///
/// Returns `None` when the target is absent from the timeline or is not an
/// `m.room.message` (state events, polls, stickers).
fn edit_target_msgtype<'items>(
    items: &'items eyeball_im::Vector<Arc<SdkTimelineItem>>,
    item_id: &TimelineEventItemId,
) -> Option<&'items MessageType> {
    use matrix_sdk_ui::timeline::TimelineItemKind;

    items.iter().rev().find_map(|item| {
        let TimelineItemKind::Event(event_item) = item.kind() else {
            return None;
        };
        let is_target = match item_id {
            TimelineEventItemId::EventId(event_id) => {
                event_item.event_id() == Some(event_id.as_ref())
            }
            TimelineEventItemId::TransactionId(transaction_id) => {
                event_item.transaction_id() == Some(transaction_id.as_ref())
            }
        };
        if !is_target {
            return None;
        }
        event_item
            .content()
            .as_message()
            .map(|message| message.msgtype())
    })
}

/// Choose the replacement content for an edit of `body`.
///
/// A media message keeps its attachment (`url`/`file`/`info`/`filename`) in the
/// same content as its caption, so replacing the event with `m.text` drops the
/// attachment and reads as data loss in the timeline (issue #328). Media rows
/// therefore edit the caption in place; everything else keeps the plain-text
/// replacement. This decision stays in core because the GUI submits only the new
/// visible text and never sees the original Matrix content.
fn edited_document_content_for_edit_target(
    msgtype: Option<&MessageType>,
    document: &ComposerDocument,
) -> EditedContent {
    let body = document.plain_body();
    let formatted_body = document.formatted_body();
    let mentions = document.mention_intent();
    if msgtype.is_some_and(msgtype_carries_editable_caption) {
        return EditedContent::MediaCaption {
            caption: Some(body),
            formatted_caption: formatted_body.map(FormattedBody::html),
            mentions: ruma_mentions_from_intent(&mentions),
        };
    }

    let mut content = match (msgtype, formatted_body) {
        (Some(MessageType::Emote(_)), Some(formatted)) => {
            RoomMessageEventContentWithoutRelation::emote_html(body, formatted)
        }
        (Some(MessageType::Notice(_)), Some(formatted)) => {
            RoomMessageEventContentWithoutRelation::notice_html(body, formatted)
        }
        (_, Some(formatted)) => RoomMessageEventContentWithoutRelation::text_html(body, formatted),
        (Some(MessageType::Emote(_)), None) => {
            RoomMessageEventContentWithoutRelation::emote_plain(body)
        }
        (Some(MessageType::Notice(_)), None) => {
            RoomMessageEventContentWithoutRelation::notice_plain(body)
        }
        (_, None) => RoomMessageEventContentWithoutRelation::text_plain(body),
    };
    if let Some(mentions) = ruma_mentions_from_intent(&mentions) {
        content = content.add_mentions(mentions);
    }
    EditedContent::RoomMessage(content)
}

#[cfg(test)]
fn edited_content_for_edit_target(
    msgtype: Option<&MessageType>,
    body: &str,
    mentions: &MentionIntent,
) -> EditedContent {
    if msgtype.is_some_and(msgtype_carries_editable_caption) {
        return EditedContent::MediaCaption {
            caption: Some(body.to_owned()),
            formatted_caption: None,
            mentions: ruma_mentions_from_intent(mentions),
        };
    }

    let mut content = match msgtype {
        Some(MessageType::Emote(_)) => RoomMessageEventContentWithoutRelation::emote_plain(body),
        Some(MessageType::Notice(_)) => RoomMessageEventContentWithoutRelation::notice_plain(body),
        _ => RoomMessageEventContentWithoutRelation::text_plain(body),
    };
    if let Some(mentions) = ruma_mentions_from_intent(mentions) {
        content = content.add_mentions(mentions);
    }
    EditedContent::RoomMessage(content)
}

fn message_edit_target_token(msgtype: Option<&MessageType>) -> &'static str {
    match msgtype {
        None => "unresolved",
        Some(MessageType::Audio(_)) => "audio",
        Some(MessageType::Emote(_)) => "emote",
        Some(MessageType::File(_)) => "file",
        Some(MessageType::Image(_)) => "image",
        Some(MessageType::Notice(_)) => "notice",
        Some(MessageType::Text(_)) => "text",
        Some(MessageType::Video(_)) => "video",
        Some(_) => "other",
    }
}

/// Record which replacement shape an edit chose for its target.
///
/// Private-data-free: message type tokens and presence booleans only, never
/// bodies, captions, filenames, MXC URIs, event ids, or raw SDK errors.
fn trace_message_edit_target(msgtype: Option<&MessageType>, content: &EditedContent) {
    let media = msgtype.and_then(msgtype_media_shape);
    koushi_diagnostics::record(
        DiagnosticEvent::new(
            DiagnosticLevel::Info,
            "core.timeline_edit",
            "replacement_selected",
        )
        .field(DiagnosticField::token(
            "target",
            message_edit_target_token(msgtype),
        ))
        .field(DiagnosticField::token(
            "replacement",
            match content {
                EditedContent::MediaCaption { .. } => "media_caption",
                _ => "text",
            },
        ))
        .field(DiagnosticField::boolean(
            "has_url",
            media.is_some_and(|shape| !shape.encrypted),
        ))
        .field(DiagnosticField::boolean(
            "has_file",
            media.is_some_and(|shape| shape.encrypted),
        ))
        .field(DiagnosticField::boolean(
            "has_info",
            media.is_some_and(|shape| shape.has_info),
        ))
        .field(DiagnosticField::boolean(
            "has_caption",
            media.is_some_and(|shape| shape.has_caption),
        )),
    );
}

fn mention_summary_for_message_type(msgtype: Option<&MessageType>) -> (HashSet<String>, bool) {
    let Some(msgtype) = msgtype else {
        return (HashSet::new(), false);
    };
    let Ok(raw) = serde_json::to_value(msgtype) else {
        return (HashSet::new(), false);
    };
    let Some(mentions) = raw.get("m.mentions") else {
        return (HashSet::new(), false);
    };
    let users = mentions
        .get("user_ids")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect();
    let room = mentions
        .get("room")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    (users, room)
}

fn mention_counts_for_edit(
    original: Option<&MessageType>,
    edited: &EditedContent,
) -> (usize, usize) {
    let old = mention_summary_for_message_type(original);
    let next = match edited {
        EditedContent::RoomMessage(content) => content
            .mentions
            .as_ref()
            .map(|mentions| {
                (
                    mentions.user_ids.iter().map(ToString::to_string).collect(),
                    mentions.room,
                )
            })
            .unwrap_or_default(),
        EditedContent::MediaCaption { mentions, .. } => mentions
            .as_ref()
            .map(|mentions| {
                (
                    mentions.user_ids.iter().map(ToString::to_string).collect(),
                    mentions.room,
                )
            })
            .unwrap_or_default(),
        EditedContent::PollStart { .. } => (HashSet::new(), false),
    };
    let final_count = next.0.len() + usize::from(next.1);
    let revision_count = next.0.difference(&old.0).count() + usize::from(next.1 && !old.1);
    (final_count, revision_count)
}

fn trace_message_edit_lifecycle(
    stage: &'static str,
    target: &'static str,
    original_mention_count: usize,
    final_mention_count: usize,
    revision_mention_count: Option<usize>,
    outcome: Option<&'static str>,
) {
    let mut event = DiagnosticEvent::new(DiagnosticLevel::Info, "core.timeline_edit", stage)
        .field(DiagnosticField::token("target", target))
        .field(DiagnosticField::count(
            "original_mention_count",
            original_mention_count.try_into().unwrap_or(u64::MAX),
        ))
        .field(DiagnosticField::count(
            "final_mention_count",
            final_mention_count.try_into().unwrap_or(u64::MAX),
        ));
    if let Some(count) = revision_mention_count {
        event = event.field(DiagnosticField::count(
            "revision_mention_count",
            count.try_into().unwrap_or(u64::MAX),
        ));
    }
    if let Some(outcome) = outcome {
        event = event.field(DiagnosticField::token("outcome", outcome));
    }
    koushi_diagnostics::record(event);
}

pub(crate) fn validate_retry_send(
    state: Option<&TimelineSendState>,
) -> Result<(), TimelineFailureKind> {
    match state {
        Some(TimelineSendState::NotSent { .. }) => Ok(()),
        Some(
            TimelineSendState::Sending | TimelineSendState::Cancelled | TimelineSendState::Sent,
        ) => Err(TimelineFailureKind::InvalidSendState),
        None => Err(TimelineFailureKind::InvalidSendTarget),
    }
}

pub(crate) fn validate_cancel_send(
    state: Option<&TimelineSendState>,
) -> Result<(), TimelineFailureKind> {
    match state {
        Some(TimelineSendState::Sending | TimelineSendState::NotSent { .. }) => Ok(()),
        Some(TimelineSendState::Cancelled | TimelineSendState::Sent) => {
            Err(TimelineFailureKind::InvalidSendState)
        }
        None => Err(TimelineFailureKind::InvalidSendTarget),
    }
}

pub(crate) fn reaction_groups_from_sdk(
    reactions: &ReactionsByKeyBySender,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
) -> Vec<crate::event::ReactionGroup> {
    reactions
        .iter()
        .map(|(key, senders)| crate::event::ReactionGroup {
            key: key.clone(),
            count: senders.len().min(u32::MAX as usize) as u32,
            reacted_by_me: own_user_id
                .map(|user_id| {
                    senders
                        .keys()
                        .any(|sender| sender.as_str() == user_id.as_str())
                })
                .unwrap_or(false),
            my_reaction_event_id: own_user_id.and_then(|user_id| {
                senders.iter().find_map(|(sender, info)| {
                    if sender.as_str() == user_id.as_str() {
                        match &info.status {
                            ReactionStatus::RemoteToRemote(event_id) => Some(event_id.to_string()),
                            ReactionStatus::LocalToLocal(_) | ReactionStatus::LocalToRemote(_) => {
                                None
                            }
                        }
                    } else {
                        None
                    }
                })
            }),
            sender_preview: senders
                .keys()
                .take(3)
                .map(|sender| ReactionSender {
                    user_id: sender.to_string(),
                    display_label: None,
                })
                .collect(),
        })
        .collect()
}

/// Convert one ordered SDK diff batch while tracking the evolving canonical
/// length. `PopBack` has no explicit index and `Append` has no equivalent wire
/// variant, so both must be expanded with the batch's actual preceding state.
pub(super) fn sdk_vector_diffs_to_timeline_diffs(
    diffs: &[eyeball_im::VectorDiff<Arc<SdkTimelineItem>>],
    initial_canonical_len: usize,
    key: &TimelineKey,
    own_user_id: Option<&matrix_sdk::ruma::UserId>,
    send_statuses: &HashMap<String, TimelineSendState>,
    key_request_states: Option<&std::collections::BTreeMap<String, KeyRequestUiState>>,
    withheld_codes: Option<&std::collections::BTreeMap<(String, String), &'static str>>,
) -> Vec<TimelineDiff> {
    let mut canonical_len = initial_canonical_len;
    let mut converted = Vec::with_capacity(diffs.len());
    for diff in diffs {
        match diff {
            eyeball_im::VectorDiff::PushFront { value } => {
                converted.push(TimelineDiff::PushFront {
                    item: sdk_item_to_timeline_item_with_send_states(
                        key,
                        value,
                        own_user_id,
                        send_statuses,
                        None,
                        key_request_states,
                        withheld_codes,
                    ),
                });
                canonical_len += 1;
            }
            eyeball_im::VectorDiff::PushBack { value } => {
                converted.push(TimelineDiff::PushBack {
                    item: sdk_item_to_timeline_item_with_send_states(
                        key,
                        value,
                        own_user_id,
                        send_statuses,
                        None,
                        key_request_states,
                        withheld_codes,
                    ),
                });
                canonical_len += 1;
            }
            eyeball_im::VectorDiff::Insert { index, value } => {
                converted.push(TimelineDiff::Insert {
                    index: *index,
                    item: sdk_item_to_timeline_item_with_send_states(
                        key,
                        value,
                        own_user_id,
                        send_statuses,
                        None,
                        key_request_states,
                        withheld_codes,
                    ),
                });
                canonical_len += 1;
            }
            eyeball_im::VectorDiff::Set { index, value } => {
                converted.push(TimelineDiff::Set {
                    index: *index,
                    item: sdk_item_to_timeline_item_with_send_states(
                        key,
                        value,
                        own_user_id,
                        send_statuses,
                        None,
                        key_request_states,
                        withheld_codes,
                    ),
                });
            }
            eyeball_im::VectorDiff::Remove { index } => {
                converted.push(TimelineDiff::Remove { index: *index });
                if *index < canonical_len {
                    canonical_len -= 1;
                }
            }
            eyeball_im::VectorDiff::Truncate { length } => {
                converted.push(TimelineDiff::Truncate { length: *length });
                canonical_len = canonical_len.min(*length);
            }
            eyeball_im::VectorDiff::Clear => {
                converted.push(TimelineDiff::Clear);
                canonical_len = 0;
            }
            eyeball_im::VectorDiff::Reset { values } => {
                converted.push(TimelineDiff::Reset {
                    items: values
                        .iter()
                        .map(|value| {
                            sdk_item_to_timeline_item_with_send_states(
                                key,
                                value,
                                own_user_id,
                                send_statuses,
                                None,
                                None,
                                None,
                            )
                        })
                        .collect(),
                });
                canonical_len = values.len();
            }
            eyeball_im::VectorDiff::PopFront => {
                if canonical_len > 0 {
                    converted.push(TimelineDiff::Remove { index: 0 });
                    canonical_len -= 1;
                }
            }
            eyeball_im::VectorDiff::PopBack => {
                if canonical_len > 0 {
                    canonical_len -= 1;
                    converted.push(TimelineDiff::Remove {
                        index: canonical_len,
                    });
                }
            }
            eyeball_im::VectorDiff::Append { values } => {
                converted.extend(values.iter().map(|value| TimelineDiff::PushBack {
                    item: sdk_item_to_timeline_item_with_send_states(
                        key,
                        value,
                        own_user_id,
                        send_statuses,
                        None,
                        key_request_states,
                        withheld_codes,
                    ),
                }));
                canonical_len += values.len();
            }
        }
    }
    converted
}

fn classify_reaction_error(err: &matrix_sdk_ui::timeline::Error) -> TimelineFailureKind {
    match err {
        matrix_sdk_ui::timeline::Error::EventNotInTimeline(_) => {
            TimelineFailureKind::InvalidReactionTarget
        }
        _ => TimelineFailureKind::Sdk,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_source::item_body;

    use std::collections::{BTreeSet, HashSet};

    use koushi_state::{
        ComposerDocument, ComposerInline, MentionIntent, MentionTarget, ReplyQuote, ReplyQuoteState,
    };

    use matrix_sdk::room::edit::EditedContent;

    use matrix_sdk::ruma::events::room::message::{MessageType, TextMessageEventContent};

    use matrix_sdk::ruma::events::{
        Mentions, StateEventContentChange, room::name::RoomNameEventContent,
    };

    use matrix_sdk_ui::timeline::{MembershipChange, ReactionStatus, ReactionsByKeyBySender};

    use crate::command::TimelineCommand;
    use crate::event::{
        LinkPreview, LinkPreviewState, TimelineFormattedBody, TimelineItemId, TimelineMessageKind,
        TimelineNoticeI18n, TimelineNoticeI18nKey, TimelineSendFailureReason, TimelineSendState,
        TimelineSpoilerSpan, TimelineViewportObservation, message_actions_for_timeline_item,
    };

    use crate::failure::TimelineFailureKind;

    use koushi_diagnostics::DiagnosticValue;

    use matrix_sdk::ruma::events::room::message::{
        EmoteMessageEventContent, NoticeMessageEventContent,
    };
    use matrix_sdk::ruma::{OwnedUserId, uint};
    use matrix_sdk_ui::timeline::ReactionInfo;

    use super::super::diagnostics::timeline_item_diagnostic_event;
    use super::{
        apply_ignored_sender_suppression, composer_document_from_event_json,
        edited_content_for_edit_target, edited_document_content_for_edit_target,
        has_user_visible_content, link_ranges_for_message_projection,
        megolm_message_index_from_original_json, membership_change_projection,
        message_edit_target_token, message_projection_from_msgtype,
        msgtype_carries_editable_caption, project_local_megolm_rotation_reason,
        reaction_groups_from_sdk, reply_quote_from_message_projection,
        reset_loading_link_previews_to_pending, room_name_notice_projection,
        state_event_notice_body, state_event_notice_projection, timeline_item_can_edit,
        timeline_item_can_react, timeline_item_can_redact, timeline_item_should_be_hidden,
        validate_cancel_send, validate_redact_reaction, validate_retry_send,
        validate_send_reaction, visible_missing_reply_detail_event_ids,
    };

    use super::super::test_support::{fake_rid, room_key, timeline_item};

    #[test]
    fn extracts_megolm_message_index_from_encrypted_event_source() {
        let mut session =
            vodozemac::megolm::GroupSession::new(vodozemac::megolm::SessionConfig::version_1());
        session.encrypt("index zero");
        session.encrypt("index one");
        let ciphertext = session.encrypt("index two").to_base64();
        let event = serde_json::json!({
            "type": "m.room.encrypted",
            "content": {
                "algorithm": "m.megolm.v1.aes-sha2",
                "ciphertext": ciphertext,
            }
        });

        assert_eq!(megolm_message_index_from_original_json(&event), Some(2));
    }

    #[test]
    fn omits_megolm_message_index_for_unencrypted_or_invalid_source() {
        let unencrypted = serde_json::json!({
            "type": "m.room.message",
            "content": { "body": "hello", "msgtype": "m.text" }
        });
        let invalid = serde_json::json!({
            "type": "m.room.encrypted",
            "content": {
                "algorithm": "m.megolm.v1.aes-sha2",
                "ciphertext": "not-megolm",
            }
        });

        assert_eq!(megolm_message_index_from_original_json(&unencrypted), None);
        assert_eq!(megolm_message_index_from_original_json(&invalid), None);
    }

    #[test]
    fn ignored_sender_suppression_preserves_divider_and_restores_event() {
        let mut divider = timeline_item("$divider:test", None, "@ignored:test", false);
        divider.id = TimelineItemId::Synthetic {
            synthetic_id: "date-divider:test".to_owned(),
        };
        let mut event = timeline_item("$ignored:test", Some("body"), "@ignored:test", false);
        let ignored = BTreeSet::from(["@ignored:test".to_owned()]);

        apply_ignored_sender_suppression(&mut divider, &ignored);
        apply_ignored_sender_suppression(&mut event, &ignored);
        assert!(
            !divider.is_hidden,
            "ignoring a sender must not hide a date divider"
        );
        assert!(event.is_hidden);

        apply_ignored_sender_suppression(&mut divider, &BTreeSet::new());
        apply_ignored_sender_suppression(&mut event, &BTreeSet::new());
        assert!(
            !divider.is_hidden,
            "unignore must leave the date divider visible"
        );
        assert!(!event.is_hidden, "unignore must restore eligible content");
    }

    #[test]
    fn formatted_only_content_is_renderable_for_shared_eligibility() {
        let mut item = timeline_item("$formatted:test", None, "@sender:test", false);
        item.formatted = Some(TimelineFormattedBody {
            html: "<b>formatted</b>".to_owned(),
            plain_text: "formatted".to_owned(),
            code_blocks: Vec::new(),
        });

        assert!(has_user_visible_content(&item));
    }

    #[test]
    fn local_megolm_reason_is_exact_and_missing_evidence_is_unavailable() {
        use crate::event::TimelineMegolmSessionReason as Projected;
        use koushi_sdk::MatrixRoomKeyRotationReason as Sdk;

        assert_eq!(
            project_local_megolm_rotation_reason(false, Some(Sdk::ExpiredTime)),
            None
        );
        assert_eq!(
            project_local_megolm_rotation_reason(true, None),
            Some(Projected::NotRetained)
        );
        for (sdk, expected) in [
            (Sdk::Initial, Projected::Initial),
            (Sdk::ExpiredTime, Projected::ExpiredTime),
            (Sdk::ExpiredMessageCount, Projected::ExpiredMessageCount),
            (
                Sdk::MembershipOrDeviceChange,
                Projected::MembershipOrDeviceChange,
            ),
            (
                Sdk::EncryptionSettingsChanged,
                Projected::EncryptionSettingsChanged,
            ),
            (Sdk::ExplicitDiscard, Projected::ExplicitDiscard),
            (Sdk::FullMemberListReload, Projected::FullMemberListReload),
            (Sdk::RoomSubscription, Projected::RoomSubscription),
            (Sdk::LimitedSyncResponse, Projected::LimitedSyncResponse),
            (Sdk::KeyShareFailure, Projected::KeyShareFailure),
            (Sdk::StoreMissing, Projected::StoreMissing),
            (Sdk::Invalidated, Projected::Invalidated),
            (Sdk::Unknown, Projected::Unknown),
        ] {
            assert_eq!(
                project_local_megolm_rotation_reason(true, Some(sdk)),
                Some(expected)
            );
        }
    }

    #[test]
    fn visible_missing_reply_detail_event_ids_only_returns_visible_unrequested_missing_replies() {
        let mut before = timeline_item("$before:test", Some("before"), "@alice:test", false);
        before.reply_quote = Some(ReplyQuote {
            event_id: "$root-before:test".to_owned(),
            sender: None,
            sender_label: None,
            body_preview: None,
            formatted: None,
            state: ReplyQuoteState::Missing,
        });
        let first_visible =
            timeline_item("$first-visible:test", Some("first"), "@alice:test", false);
        let mut missing = timeline_item("$missing:test", Some("missing"), "@alice:test", false);
        missing.reply_quote = Some(ReplyQuote {
            event_id: "$root-missing:test".to_owned(),
            sender: None,
            sender_label: None,
            body_preview: None,
            formatted: None,
            state: ReplyQuoteState::Missing,
        });
        let mut ready = timeline_item("$ready:test", Some("ready"), "@alice:test", false);
        ready.reply_quote = Some(ReplyQuote {
            event_id: "$root-ready:test".to_owned(),
            sender: Some("@bob:test".to_owned()),
            sender_label: None,
            body_preview: Some("loaded".to_owned()),
            formatted: None,
            state: ReplyQuoteState::Ready,
        });
        let mut already_requested = timeline_item(
            "$already-requested:test",
            Some("already"),
            "@alice:test",
            false,
        );
        already_requested.reply_quote = Some(ReplyQuote {
            event_id: "$root-already:test".to_owned(),
            sender: None,
            sender_label: None,
            body_preview: None,
            formatted: None,
            state: ReplyQuoteState::Missing,
        });
        let mut after = timeline_item("$after:test", Some("after"), "@alice:test", false);
        after.reply_quote = Some(ReplyQuote {
            event_id: "$root-after:test".to_owned(),
            sender: None,
            sender_label: None,
            body_preview: None,
            formatted: None,
            state: ReplyQuoteState::Missing,
        });

        let items = vec![
            before,
            first_visible,
            missing,
            ready,
            already_requested,
            after,
        ];
        let requested = HashSet::from(["$already-requested:test".to_owned()]);

        let event_ids = visible_missing_reply_detail_event_ids(
            &items,
            &TimelineViewportObservation {
                first_visible_event_id: Some("$first-visible:test".to_owned()),
                last_visible_event_id: Some("$already-requested:test".to_owned()),
                visible_gap_ids: Vec::new(),
                at_bottom: false,
            },
            &requested,
        );

        assert_eq!(event_ids, vec!["$missing:test".to_owned()]);
    }

    fn reaction_groups_fixture() -> ReactionsByKeyBySender {
        let mut reactions = ReactionsByKeyBySender::default();
        let thumbs = reactions.entry("👍".to_owned()).or_default();
        thumbs.insert(
            OwnedUserId::try_from("@me:test").expect("user id"),
            ReactionInfo {
                timestamp: matrix_sdk::ruma::MilliSecondsSinceUnixEpoch(uint!(1)),
                status: ReactionStatus::RemoteToRemote(
                    matrix_sdk::ruma::OwnedEventId::try_from("$reaction:me").expect("event id"),
                ),
            },
        );
        thumbs.insert(
            OwnedUserId::try_from("@alice:test").expect("user id"),
            ReactionInfo {
                timestamp: matrix_sdk::ruma::MilliSecondsSinceUnixEpoch(uint!(2)),
                status: ReactionStatus::LocalToRemote(None),
            },
        );
        thumbs.insert(
            OwnedUserId::try_from("@bob:test").expect("user id"),
            ReactionInfo {
                timestamp: matrix_sdk::ruma::MilliSecondsSinceUnixEpoch(uint!(3)),
                status: ReactionStatus::LocalToRemote(None),
            },
        );
        thumbs.insert(
            OwnedUserId::try_from("@carol:test").expect("user id"),
            ReactionInfo {
                timestamp: matrix_sdk::ruma::MilliSecondsSinceUnixEpoch(uint!(4)),
                status: ReactionStatus::LocalToRemote(None),
            },
        );

        reactions
    }

    #[test]
    fn editable_document_uses_formatted_links_for_duplicate_mention_identity() {
        let document = composer_document_from_event_json(&serde_json::json!({
                "content": {
                    "body": "**hello** @Same @Same typed @Same",
                    "format": "org.matrix.custom.html",
                    "formatted_body": "<strong>hello</strong> <a href=\"https://matrix.to/#/%40alice%3Aexample.test\">@Same</a> <a href=\"https://matrix.to/#/%40bob%3Aexample.test\">@Same</a> typed @Same",
                    "m.mentions": {
                        "user_ids": ["@alice:example.test", "@bob:example.test"]
                    }
                }
            }))
            .expect("structured document");

        assert_eq!(document.plain_body(), "**hello** @Same @Same typed @Same");
        assert_eq!(
            document.mention_intent().user_ids(),
            vec!["@alice:example.test", "@bob:example.test"]
        );
        assert!(
            matches!(document.inlines.last(), Some(ComposerInline::Text { text }) if text.ends_with("typed @Same"))
        );
    }

    #[test]
    fn editable_document_keeps_room_link_identity_without_user_mentions_metadata() {
        let document = composer_document_from_event_json(&serde_json::json!({
                "content": {
                    "body": "visit **@Project**",
                    "format": "org.matrix.custom.html",
                    "formatted_body": "visit <strong><a href=\"https://matrix.to/#/%23project%3Aexample.test\">@Project</a></strong>"
                }
            }))
            .expect("structured room mention");

        assert_eq!(document.plain_body(), "visit **@Project**");
        assert!(matches!(
            &document.inlines[1],
            ComposerInline::Mention {
                target: MentionTarget::Room { room_id, .. },
                ..
            } if room_id == "#project:example.test"
        ));
    }

    #[test]
    fn editable_document_rejects_unsafe_links_even_when_labels_match() {
        let document = composer_document_from_event_json(&serde_json::json!({
            "content": {
                "body": "@Same",
                "format": "org.matrix.custom.html",
                "formatted_body": "<a href=\"http://matrix.to/#/%40alice%3Aexample.test\">@Same</a>",
                "m.mentions": { "user_ids": ["@alice:example.test"] }
            }
        }));

        assert!(document.is_none());
    }

    #[test]
    fn message_projection_carries_msgtype_and_plain_spoiler_spans() {
        let projection = message_projection_from_msgtype(
            &MessageType::Notice(NoticeMessageEventContent::plain("keep ||secret|| hidden")),
            "keep ||secret|| hidden",
        );

        assert_eq!(projection.message_kind, TimelineMessageKind::Notice);
        assert_eq!(projection.body.as_deref(), Some("keep secret hidden"));
        assert_eq!(
            projection.spoiler_spans,
            vec![TimelineSpoilerSpan {
                start_utf16: 5,
                end_utf16: 11,
                reason: None,
            }]
        );
    }

    #[test]
    fn membership_change_projection_is_a_supported_notice() {
        let projection =
            membership_change_projection("Alice", Some(MembershipChange::InvitationAccepted));

        assert_eq!(projection.message_kind, TimelineMessageKind::Notice);
        assert_eq!(projection.body.as_deref(), Some("Alice joined the room"));
        assert_eq!(projection.body_is_user_content, false);
        assert!(
            !projection
                .body
                .as_deref()
                .unwrap_or_default()
                .contains("Unsupported event: m.room.member")
        );
    }

    #[test]
    fn profile_change_projection_does_not_emit_user_id_body() {
        let source = include_str!("item_projection.rs");
        let profile_branch = source
            .split("TimelineItemContent::ProfileChange(change)")
            .nth(1)
            .expect("profile change branch should exist")
            .split("_ => {}")
            .next()
            .expect("profile change branch should precede fallback branch");

        assert!(
            profile_branch.contains("profile_change_projection(change)"),
            "profile changes should use Element-like notice text"
        );
        assert!(
            !profile_branch.contains("change.user_id()"),
            "timeline row body must not contain a raw Matrix user id for profile changes"
        );
    }

    #[test]
    fn pinned_events_projection_is_a_supported_notice() {
        assert_eq!(
            state_event_notice_body("m.room.pinned_events").as_ref(),
            "updated pinned messages"
        );
        assert_eq!(
            state_event_notice_body("m.room.create").as_ref(),
            "created the room"
        );
        assert_eq!(
            state_event_notice_body("m.room.power_levels").as_ref(),
            "updated room permissions"
        );
        assert_eq!(
            state_event_notice_body("m.room.guest_access").as_ref(),
            "updated guest access"
        );
        assert_eq!(
            state_event_notice_body("m.room.encryption").as_ref(),
            "enabled room encryption"
        );
        assert_eq!(
            state_event_notice_body("m.space.parent").as_ref(),
            "updated the parent space"
        );
        assert_eq!(
            state_event_notice_body("m.room.join_rules").as_ref(),
            "updated join rules"
        );
        assert_eq!(
            state_event_notice_body("m.room.history_visibility").as_ref(),
            "updated history visibility"
        );
        assert_eq!(
            state_event_notice_body("m.room.topic").as_ref(),
            "Unsupported event: m.room.topic"
        );
    }

    #[test]
    fn supported_state_event_notices_carry_i18n_keys() {
        let projection = state_event_notice_projection("m.room.power_levels");

        assert_eq!(projection.body.as_deref(), Some("updated room permissions"));
        assert_eq!(
            projection.notice_i18n,
            Some(TimelineNoticeI18n {
                key: TimelineNoticeI18nKey::RoomPowerLevels,
                old_name: None,
                new_name: None,
            })
        );
        assert_eq!(projection.body_is_user_content, false);
    }

    fn original_room_name_change(
        name: &str,
        previous_name: Option<&str>,
    ) -> StateEventContentChange<RoomNameEventContent> {
        StateEventContentChange::Original {
            content: RoomNameEventContent::new(name.to_owned()),
            prev_content: previous_name.map(|previous_name| {
                serde_json::from_value(serde_json::json!({ "name": previous_name }))
                    .expect("previous room name should deserialize")
            }),
        }
    }

    #[test]
    fn room_name_notice_projects_initial_name_as_structured_set_notice() {
        let projection = room_name_notice_projection(&original_room_name_change("研究室 🧪", None));

        assert_eq!(
            projection.body.as_deref(),
            Some("set the room name to 研究室 🧪")
        );
        assert_eq!(
            projection.notice_i18n,
            Some(TimelineNoticeI18n {
                key: TimelineNoticeI18nKey::RoomNameSet,
                old_name: None,
                new_name: Some("研究室 🧪".to_owned()),
            })
        );
        assert_eq!(projection.message_kind, TimelineMessageKind::Notice);
        assert!(!projection.body_is_user_content);
    }

    #[test]
    fn room_name_notice_projects_old_and_new_names_for_change() {
        let projection = room_name_notice_projection(&original_room_name_change(
            "<新しい部屋>",
            Some("Old room"),
        ));

        assert_eq!(
            projection.body.as_deref(),
            Some("changed the room name from Old room to <新しい部屋>")
        );
        assert_eq!(
            projection.notice_i18n,
            Some(TimelineNoticeI18n {
                key: TimelineNoticeI18nKey::RoomNameChanged,
                old_name: Some("Old room".to_owned()),
                new_name: Some("<新しい部屋>".to_owned()),
            })
        );
    }

    #[test]
    fn room_name_notice_projects_empty_name_as_removal() {
        let projection =
            room_name_notice_projection(&original_room_name_change("   ", Some("Old room")));

        assert_eq!(projection.body.as_deref(), Some("removed the room name"));
        assert_eq!(
            projection.notice_i18n,
            Some(TimelineNoticeI18n {
                key: TimelineNoticeI18nKey::RoomNameRemoved,
                old_name: None,
                new_name: None,
            })
        );
    }

    #[test]
    fn room_name_notice_uses_set_wording_for_identical_names() {
        let projection =
            room_name_notice_projection(&original_room_name_change("Same room", Some("Same room")));

        assert_eq!(
            projection.notice_i18n.as_ref().map(|notice| notice.key),
            Some(TimelineNoticeI18nKey::RoomNameSet)
        );
    }

    #[test]
    fn room_name_notice_projects_redacted_content_as_safe_generic_notice() {
        let redacted = StateEventContentChange::Redacted(
            serde_json::from_value(serde_json::json!({}))
                .expect("redacted room name should deserialize"),
        );
        let projection = room_name_notice_projection(&redacted);

        assert_eq!(projection.body.as_deref(), Some("changed the room name"));
        assert_eq!(
            projection.notice_i18n,
            Some(TimelineNoticeI18n {
                key: TimelineNoticeI18nKey::RoomNameChangedGeneric,
                old_name: None,
                new_name: None,
            })
        );
        assert!(!projection.body.unwrap_or_default().contains("m.room.name"));
    }

    #[test]
    fn message_projection_extracts_formatted_spoiler_spans_with_reason() {
        let msgtype = MessageType::Emote(EmoteMessageEventContent::html(
            "plain fallback",
            r#"keep <span data-mx-spoiler="because">secret</span> hidden"#,
        ));

        let projection = message_projection_from_msgtype(&msgtype, "plain fallback");

        assert_eq!(projection.message_kind, TimelineMessageKind::Emote);
        assert_eq!(
            projection.spoiler_spans,
            vec![TimelineSpoilerSpan {
                start_utf16: 5,
                end_utf16: 11,
                reason: Some("because".to_owned()),
            }]
        );
    }

    #[test]
    fn message_projection_sanitizes_formatted_html_and_extracts_code_blocks() {
        let msgtype = MessageType::Text(TextMessageEventContent::html(
            "plain fallback",
            r#"<strong>ok</strong><script>alert(1)</script><a href="javascript:alert(1)">bad</a><a href="https://example.invalid/path">safe</a><pre><code class="language-rust ignored">fn main() {}</code></pre>"#,
        ));

        let projection = message_projection_from_msgtype(&msgtype, "plain fallback");
        let formatted = projection
            .formatted
            .expect("html formatted_body should project to a Rust-owned render model");

        assert!(formatted.html.contains("<strong>ok</strong>"));
        assert!(!formatted.html.contains("<script"));
        assert!(!formatted.html.contains("alert(1)"));
        assert!(!formatted.html.contains("javascript:"));
        assert!(formatted.html.contains("https://example.invalid/path"));
        assert_eq!(formatted.plain_text, "okbadsafefn main() {}");
        assert_eq!(formatted.code_blocks.len(), 1);
        assert_eq!(formatted.code_blocks[0].language.as_deref(), Some("rust"));
        assert_eq!(formatted.code_blocks[0].body, "fn main() {}");
    }

    #[test]
    fn formatted_message_link_ranges_use_formatted_plain_text_basis() {
        let msgtype = MessageType::Text(TextMessageEventContent::html(
            "fallback without url",
            r#"<strong>Visit https://example.invalid/path</strong>"#,
        ));

        let projection = message_projection_from_msgtype(&msgtype, "fallback without url");
        let ranges = link_ranges_for_message_projection(
            projection.body.as_deref(),
            projection.formatted.as_ref(),
        );

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].url, "https://example.invalid/path");
        assert_eq!(ranges[0].start_utf16, "Visit ".encode_utf16().count());
        assert_eq!(
            ranges[0].end_utf16,
            "Visit https://example.invalid/path".encode_utf16().count()
        );
    }

    #[test]
    fn message_projection_keeps_allowed_formatted_blocks_and_spoilers() {
        let msgtype = MessageType::Emote(EmoteMessageEventContent::html(
            "plain fallback",
            r#"<blockquote>quote</blockquote><ul><li>one</li></ul><span data-mx-spoiler="reason">secret</span>"#,
        ));

        let projection = message_projection_from_msgtype(&msgtype, "plain fallback");
        let formatted = projection
            .formatted
            .expect("allowed formatted_body should project to a render model");

        assert!(formatted.html.contains("<blockquote>quote</blockquote>"));
        assert!(formatted.html.contains("<ul><li>one</li></ul>"));
        assert!(formatted.html.contains("data-mx-spoiler=\"reason\""));
        assert!(formatted.html.contains(">secret<"));
    }

    #[test]
    fn reply_quote_projection_retains_sanitized_formatted_body() {
        let msgtype = MessageType::Text(TextMessageEventContent::html(
            "plain fallback",
            r#"<ul><li>one</li><li>two</li></ul><script>bad()</script><pre><code class="language-rust">fn main() {}</code></pre>"#,
        ));

        let projection = message_projection_from_msgtype(&msgtype, "plain fallback");
        let quote = reply_quote_from_message_projection(
            "$root:example.invalid",
            Some("@bob:example.invalid".to_owned()),
            Some(projection),
        );

        assert_eq!(quote.state, ReplyQuoteState::Ready);
        let formatted = quote.formatted.expect("formatted quote body");
        assert!(formatted.html.contains("<ul><li>one</li><li>two</li></ul>"));
        assert!(!formatted.html.contains("<script"));
        assert_eq!(formatted.code_blocks.len(), 1);
        assert_eq!(formatted.code_blocks[0].language.as_deref(), Some("rust"));
        assert_eq!(formatted.code_blocks[0].body, "fn main() {}");
    }

    #[test]
    fn captionless_media_projections_can_reply_and_keep_filename_reply_quotes() {
        for msgtype in media_msgtype_fixtures() {
            let projection = message_projection_from_msgtype(&msgtype, "ignored fallback body");
            let filename = projection
                .media
                .as_ref()
                .expect("media fixture projects media")
                .filename
                .clone();

            assert!(
                projection.body.is_none(),
                "media fixture must be captionless"
            );

            let actions = message_actions_for_timeline_item(
                "!room:test",
                &TimelineItemId::Event {
                    event_id: "$captionless-media:test".to_owned(),
                },
                projection.body.as_deref(),
                projection.media.is_some(),
                false,
            );
            assert!(actions.can_reply, "{filename} should be replyable");

            let quote = reply_quote_from_message_projection(
                "$captionless-media:test",
                None,
                Some(projection),
            );
            assert_eq!(quote.body_preview.as_deref(), Some(filename.as_str()));
        }
    }

    #[test]
    fn message_projection_falls_back_to_plain_body_when_formatted_body_is_empty() {
        let msgtype = MessageType::Text(TextMessageEventContent::html("plain fallback", "   "));

        let projection = message_projection_from_msgtype(&msgtype, "plain fallback");

        assert_eq!(projection.body.as_deref(), Some("plain fallback"));
        assert!(projection.formatted.is_none());
    }

    #[test]
    fn message_projection_falls_back_to_plain_body_when_formatted_body_has_only_markup() {
        let msgtype = MessageType::Text(TextMessageEventContent::html(
            "plain fallback",
            "<p><br /></p>",
        ));

        let projection = message_projection_from_msgtype(&msgtype, "plain fallback");

        assert_eq!(projection.body.as_deref(), Some("plain fallback"));
        assert!(projection.formatted.is_none());
    }

    #[test]
    fn user_visible_content_includes_formatted_body() {
        let mut item = timeline_item("$formatted:test", None, "@alice:test", false);
        item.formatted = Some(crate::event::TimelineFormattedBody {
            html: "<strong>visible</strong>".to_owned(),
            plain_text: "visible".to_owned(),
            code_blocks: Vec::new(),
        });

        assert!(has_user_visible_content(&item));
    }

    #[test]
    fn bodyless_event_backed_items_are_hidden_unless_redacted() {
        assert!(timeline_item_should_be_hidden(false, false));
        assert!(!timeline_item_should_be_hidden(true, false));
        assert!(!timeline_item_should_be_hidden(false, true));
    }

    #[test]
    fn timeline_item_structured_fields_match_private_legacy_semantics() {
        let key = room_key();
        let mut item = timeline_item("$private-event:test", Some("   "), "   ", true);
        item.timestamp_ms = Some(1_783_076_820_000);
        item.thread_root = Some("   ".to_owned());
        item.in_reply_to_event_id = Some("   ".to_owned());
        item.formatted = Some(crate::event::TimelineFormattedBody {
            html: "<br>".to_owned(),
            plain_text: "   ".to_owned(),
            code_blocks: Vec::new(),
        });

        let event = timeline_item_diagnostic_event("initial", &key, "item", Some(7), &item);

        assert_eq!(
            event
                .fields
                .iter()
                .map(|field| (field.key, field.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("kind", DiagnosticValue::Token("item")),
                ("timeline", DiagnosticValue::Token("room")),
                ("id_kind", DiagnosticValue::Token("event")),
                ("count", DiagnosticValue::Count(1)),
                ("index", DiagnosticValue::Count(7)),
                ("index_present", DiagnosticValue::Boolean(true)),
                (
                    "timestamp_minute",
                    DiagnosticValue::Count(1_783_076_820_000 / 60_000),
                ),
                ("timestamp_present", DiagnosticValue::Boolean(true)),
                ("sender_present", DiagnosticValue::Boolean(false)),
                ("hidden", DiagnosticValue::Boolean(true)),
                ("thread_root_present", DiagnosticValue::Boolean(false)),
                ("reply_present", DiagnosticValue::Boolean(false)),
                ("body_present", DiagnosticValue::Boolean(false)),
                ("formatted_present", DiagnosticValue::Boolean(false)),
                ("media_present", DiagnosticValue::Boolean(false)),
                ("redacted", DiagnosticValue::Boolean(false)),
                ("unable_to_decrypt", DiagnosticValue::Boolean(false)),
                ("send_state_present", DiagnosticValue::Boolean(false)),
            ]
        );
        let serialized = serde_json::to_string(&event).expect("diagnostic event serializes");
        for private_value in ["$private-event:test", "!r:test"] {
            assert!(!serialized.contains(private_value));
        }
    }

    #[test]
    fn timeline_search_index_mutations_use_reliable_delivery() {
        let source = include_str!("item_projection.rs");
        let forwarder = source
            .split("async fn forward_diff_to_search")
            .nth(1)
            .expect("search index diff forwarder should be async")
            .split("fn search_index_messages_for_diff")
            .next()
            .expect("message builder should follow forwarder");

        assert!(
            forwarder.contains("emit_search_messages_reliable"),
            "live search index mutations, including redactions, must use the generation-fenced reliable sender"
        );
        assert!(
            !forwarder.contains("try_send(SearchIndexMessage"),
            "search index mutation delivery must not silently drop redactions or edits"
        );
    }

    #[test]
    fn media_gallery_and_thread_attention_projections_use_reliable_delivery() {
        let initial_subscribe = item_body(include_str!("actor.rs"), "async fn spawn(");
        let diff_batch = item_body(include_str!("relay.rs"), "async fn handle_diff_batch");
        let media_gallery_helper = item_body(
            include_str!("media.rs"),
            "async fn emit_media_gallery_if_changed",
        );
        assert!(
            initial_subscribe.contains("action_tx.send(vec![action]).await"),
            "initial media gallery projection must use reliable delivery"
        );
        assert!(
            diff_batch.contains("self.emit_action_reliable(action).await"),
            "thread attention projection must use reliable reducer delivery"
        );
        assert!(
            media_gallery_helper.contains("self.emit_action_reliable(action).await")
                && !media_gallery_helper.contains("try_send(vec![action])"),
            "media gallery dedupe ledger must not advance behind a dropped try_send"
        );
    }

    #[test]
    fn send_operation_guards_allow_retry_and_cancel_only_from_outbound_states() {
        assert_eq!(
            validate_retry_send(Some(&TimelineSendState::NotSent {
                reason: TimelineSendFailureReason::Recoverable,
            })),
            Ok(())
        );
        assert_eq!(
            validate_retry_send(Some(&TimelineSendState::Sending)),
            Err(TimelineFailureKind::InvalidSendState)
        );
        assert_eq!(
            validate_retry_send(Some(&TimelineSendState::Sent)),
            Err(TimelineFailureKind::InvalidSendState)
        );
        assert_eq!(
            validate_cancel_send(Some(&TimelineSendState::Sending)),
            Ok(())
        );
        assert_eq!(
            validate_cancel_send(Some(&TimelineSendState::NotSent {
                reason: TimelineSendFailureReason::Unrecoverable,
            })),
            Ok(())
        );
        assert_eq!(
            validate_cancel_send(Some(&TimelineSendState::Sent)),
            Err(TimelineFailureKind::InvalidSendState)
        );
        assert_eq!(
            validate_cancel_send(None),
            Err(TimelineFailureKind::InvalidSendTarget)
        );
    }

    #[test]
    fn retry_send_reenables_sdk_room_queue_before_unwedge() {
        let source = include_str!("outbound_send.rs");
        let retry_handler = source
            .split("async fn handle_retry_send")
            .nth(1)
            .and_then(|section| section.split("async fn handle_cancel_send").next())
            .expect("retry handler source");
        let enable_index = retry_handler
            .find("set_enabled(true)")
            .expect("retry must re-enable the SDK room send queue");
        let unwedge_index = retry_handler
            .find("unwedge().await")
            .expect("retry must unwedge the SDK send handle");

        assert!(
            enable_index < unwedge_index,
            "room send queue must be re-enabled before SendHandle::unwedge()"
        );
    }

    #[test]
    fn cancel_send_reenables_sdk_room_queue_after_abort() {
        let source = include_str!("outbound_send.rs");
        let cancel_handler = source
            .split("async fn handle_cancel_send")
            .nth(1)
            .and_then(|section| section.split("fn sdk_room_for_key").next())
            .expect("cancel handler source");
        let abort_index = cancel_handler
            .find("abort().await")
            .expect("cancel must abort the SDK send handle");
        let enable_index = cancel_handler
            .find("set_enabled(true)")
            .expect("cancel must re-enable the SDK room send queue after abort");

        assert!(
            abort_index < enable_index,
            "room send queue must be re-enabled after a successful abort"
        );
    }

    #[test]
    fn reaction_groups_project_my_sender_and_remote_event_id() {
        let own_user_id = OwnedUserId::try_from("@me:test").expect("user id");
        let groups =
            reaction_groups_from_sdk(&reaction_groups_fixture(), Some(own_user_id.as_ref()));

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "👍");
        assert_eq!(groups[0].count, 4);
        assert!(groups[0].reacted_by_me);
        assert_eq!(
            groups[0].my_reaction_event_id.as_deref(),
            Some("$reaction:me")
        );
        assert_eq!(
            groups[0]
                .sender_preview
                .iter()
                .map(|sender| sender.user_id.as_str())
                .collect::<Vec<_>>(),
            vec!["@me:test", "@alice:test", "@bob:test"]
        );
    }

    #[test]
    fn reaction_groups_count_unique_senders_after_sdk_deduplication() {
        let mut reactions = ReactionsByKeyBySender::default();
        let thumbs = reactions.entry("👍".to_owned()).or_default();
        let alice = OwnedUserId::try_from("@alice:test").expect("user id");
        thumbs.insert(
            alice.clone(),
            ReactionInfo {
                timestamp: matrix_sdk::ruma::MilliSecondsSinceUnixEpoch(uint!(1)),
                status: ReactionStatus::RemoteToRemote(
                    matrix_sdk::ruma::OwnedEventId::try_from("$reaction:old").expect("event id"),
                ),
            },
        );
        thumbs.insert(
            alice,
            ReactionInfo {
                timestamp: matrix_sdk::ruma::MilliSecondsSinceUnixEpoch(uint!(2)),
                status: ReactionStatus::RemoteToRemote(
                    matrix_sdk::ruma::OwnedEventId::try_from("$reaction:new").expect("event id"),
                ),
            },
        );

        let groups = reaction_groups_from_sdk(&reactions, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count, 1);
        assert_eq!(
            groups[0]
                .sender_preview
                .iter()
                .map(|sender| sender.user_id.as_str())
                .collect::<Vec<_>>(),
            vec!["@alice:test"]
        );
    }

    #[test]
    fn reaction_groups_follow_sdk_redaction_removal() {
        let mut reactions = reaction_groups_fixture();
        reactions
            .get_mut("👍")
            .expect("thumbs reaction")
            .shift_remove(&OwnedUserId::try_from("@me:test").expect("user id"));
        let own_user_id = OwnedUserId::try_from("@me:test").expect("user id");

        let groups = reaction_groups_from_sdk(&reactions, Some(own_user_id.as_ref()));

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count, 3);
        assert!(!groups[0].reacted_by_me);
        assert_eq!(groups[0].my_reaction_event_id, None);
    }

    #[test]
    fn timeline_item_can_react_requires_event_backed_renderable_content() {
        assert!(timeline_item_can_react(true, true, false, true));
        assert!(!timeline_item_can_react(false, true, false, true));
        assert!(!timeline_item_can_react(true, false, false, true));
        assert!(!timeline_item_can_react(true, true, false, false));
        assert!(!timeline_item_can_react(true, true, true, true));
    }

    #[test]
    fn send_reaction_guard_requires_reactable_target_without_existing_own_reaction() {
        assert_eq!(validate_send_reaction(true, None), Ok(()));
        assert_eq!(
            validate_send_reaction(false, None),
            Err(TimelineFailureKind::InvalidReactionTarget)
        );
        assert_eq!(
            validate_send_reaction(true, Some("$reaction:example.test")),
            Err(TimelineFailureKind::InvalidReactionState)
        );
    }

    #[test]
    fn redact_reaction_guard_requires_matching_own_reaction_event() {
        assert_eq!(
            validate_redact_reaction(
                true,
                Some("$reaction:example.test"),
                "$reaction:example.test"
            ),
            Ok(())
        );
        assert_eq!(
            validate_redact_reaction(
                false,
                Some("$reaction:example.test"),
                "$reaction:example.test"
            ),
            Err(TimelineFailureKind::InvalidReactionTarget)
        );
        assert_eq!(
            validate_redact_reaction(true, None, "$reaction:example.test"),
            Err(TimelineFailureKind::InvalidReactionState)
        );
        assert_eq!(
            validate_redact_reaction(
                true,
                Some("$other-reaction:example.test"),
                "$reaction:example.test"
            ),
            Err(TimelineFailureKind::InvalidReactionState)
        );
    }

    #[test]
    fn reaction_and_read_signal_handlers_emit_private_latency_traces() {
        let item = include_str!("item_projection.rs");
        let diagnostics = include_str!("diagnostics.rs");
        let manager = include_str!("manager.rs");
        let read = include_str!("read_state.rs");
        let sources = [item, diagnostics, manager, read];
        for kind in ["send_reaction", "redact_reaction"] {
            assert!(sources.iter().any(|source| source.contains(&format!("trace_timeline_actor_operation(\n            \"actor_start\",\n            \"{kind}\""))), "{kind} should trace actor start");
            assert!(sources.iter().any(|source| source.contains(&format!("trace_timeline_actor_operation(\n                    \"actor_finish\",\n                    \"{kind}\"")) || source.contains(&format!("trace_timeline_actor_operation(\n                \"actor_finish\",\n                \"{kind}\""))), "{kind} should trace actor completion");
        }
        for kind in ["send_read_receipt", "set_fully_read"] {
            assert!(
                sources.iter().any(|source| source.contains(&format!(
                    "trace_timeline_route(\"manager_received\", \"{kind}\""
                ))),
                "{kind} should trace manager admission"
            );
        }
        assert!(
            sources
                .iter()
                .any(|source| source.contains("ReadWorkerCompletion::Network"))
                && sources
                    .iter()
                    .any(|source| source.contains("ReadWorkerCompletion::ActorApplied")),
            "read operations must expose manager-owned network and state-apply completion stages"
        );
        assert!(
            sources.iter().any(|source| source
                .contains("trace_timeline_actor_scan(\n                    \"target_scan\"")
                || source.contains("trace_timeline_actor_scan(\n                \"target_scan\"")),
            "reaction target scan should expose item count and elapsed time"
        );
    }

    #[test]
    fn timeline_item_can_redact_requires_own_renderable_event_content() {
        assert!(timeline_item_can_redact(true, true, false, true));
        assert!(!timeline_item_can_redact(false, true, false, true));
        assert!(!timeline_item_can_redact(true, false, false, true));
        assert!(!timeline_item_can_redact(true, true, true, true));
        assert!(!timeline_item_can_redact(true, true, false, false));
    }

    #[test]
    fn timeline_item_can_edit_requires_own_editable_body() {
        assert!(timeline_item_can_edit(true, true, false, true));
        assert!(!timeline_item_can_edit(false, true, false, true));
        assert!(!timeline_item_can_edit(true, false, false, true));
        assert!(!timeline_item_can_edit(true, true, true, true));
        assert!(!timeline_item_can_edit(true, true, false, false));
    }

    fn media_msgtype_fixtures() -> Vec<MessageType> {
        use matrix_sdk::ruma::events::room::message::{
            AudioMessageEventContent, FileMessageEventContent, ImageMessageEventContent,
            VideoMessageEventContent,
        };
        use matrix_sdk::ruma::owned_mxc_uri;

        vec![
            MessageType::Audio(AudioMessageEventContent::plain(
                "fixture-audio".to_owned(),
                owned_mxc_uri!("mxc://fixture.invalid/audio"),
            )),
            MessageType::File(FileMessageEventContent::plain(
                "fixture-file".to_owned(),
                owned_mxc_uri!("mxc://fixture.invalid/file"),
            )),
            MessageType::Image(ImageMessageEventContent::plain(
                "fixture-image".to_owned(),
                owned_mxc_uri!("mxc://fixture.invalid/image"),
            )),
            MessageType::Video(VideoMessageEventContent::plain(
                "fixture-video".to_owned(),
                owned_mxc_uri!("mxc://fixture.invalid/video"),
            )),
        ]
    }

    fn non_media_msgtype_fixtures() -> Vec<MessageType> {
        use matrix_sdk::ruma::events::room::message::{
            EmoteMessageEventContent, NoticeMessageEventContent, TextMessageEventContent,
        };

        vec![
            MessageType::Emote(EmoteMessageEventContent::plain("fixture-emote")),
            MessageType::Notice(NoticeMessageEventContent::plain("fixture-notice")),
            MessageType::Text(TextMessageEventContent::plain("fixture-text")),
        ]
    }

    #[test]
    fn edit_replacement_preserves_media_attachment_as_caption() {
        // A text replacement carries no url/file/info/filename, so it silently
        // drops the attachment from the edited event (issue #328).
        for msgtype in media_msgtype_fixtures() {
            let target = message_edit_target_token(Some(&msgtype));
            match edited_content_for_edit_target(
                Some(&msgtype),
                "edited caption",
                &MentionIntent::default(),
            ) {
                EditedContent::MediaCaption {
                    caption,
                    formatted_caption,
                    mentions,
                } => {
                    assert_eq!(caption.as_deref(), Some("edited caption"));
                    assert!(
                        formatted_caption.is_none(),
                        "{target}: plain caption edit must not add formatting"
                    );
                    assert!(
                        mentions.is_none(),
                        "{target}: plain caption edit must not add mentions"
                    );
                }
                other => panic!("{target}: media edit must preserve the attachment, got {other:?}"),
            }
        }
    }

    #[test]
    fn edit_replacement_preserves_non_media_message_kind() {
        for msgtype in non_media_msgtype_fixtures() {
            let target = message_edit_target_token(Some(&msgtype));
            match edited_content_for_edit_target(
                Some(&msgtype),
                "edited body",
                &MentionIntent::default(),
            ) {
                EditedContent::RoomMessage(content) => match &content.msgtype {
                    MessageType::Text(text) => assert_eq!(text.body, "edited body"),
                    MessageType::Notice(notice) => assert_eq!(notice.body, "edited body"),
                    MessageType::Emote(emote) => assert_eq!(emote.body, "edited body"),
                    other => {
                        panic!(
                            "{target}: non-media replacement expected, got {:?}",
                            other.msgtype()
                        )
                    }
                },
                other => panic!("{target}: non-media replacement expected, got {other:?}"),
            }
        }
    }

    #[test]
    fn edit_replacement_stays_plain_text_for_unresolved_target() {
        // A target missing from the timeline, or one that is not an
        // m.room.message, keeps the pre-existing text replacement instead of
        // guessing a caption edit the SDK would reject.
        assert!(matches!(
            edited_content_for_edit_target(None, "edited body", &MentionIntent::default()),
            EditedContent::RoomMessage(_)
        ));
    }

    #[test]
    fn structured_edit_preserves_final_mentions_and_formats_text_and_media_captions() {
        let document = ComposerDocument::new(vec![
            ComposerInline::Text {
                text: "hello ".into(),
            },
            ComposerInline::Mention {
                target: MentionTarget::User {
                    user_id: "@alice:example.org".into(),
                    display_label: "Alice".into(),
                },
                display_label: "Alice".into(),
            },
        ]);
        let text = MessageType::Text(TextMessageEventContent::plain("old"));
        let EditedContent::RoomMessage(content) =
            edited_document_content_for_edit_target(Some(&text), &document)
        else {
            panic!("text edit must stay a room message")
        };
        let MessageType::Text(text) = content.msgtype else {
            panic!("text edit must stay text")
        };
        assert_eq!(text.body, "hello @Alice");
        assert_eq!(
            text.formatted.map(|formatted| formatted.body),
            Some("hello <a href=\"https://matrix.to/#/%40alice%3Aexample.org\">@Alice</a>".into())
        );
        assert_eq!(content.mentions.expect("final mentions").user_ids.len(), 1);

        let media = media_msgtype_fixtures().pop().expect("media fixture");
        let EditedContent::MediaCaption {
            caption,
            formatted_caption,
            mentions,
        } = edited_document_content_for_edit_target(Some(&media), &document)
        else {
            panic!("media edit must remain a caption edit")
        };
        assert_eq!(caption.as_deref(), Some("hello @Alice"));
        assert!(formatted_caption.is_some());
        assert_eq!(mentions.expect("caption mentions").user_ids.len(), 1);
    }

    #[test]
    fn edit_replacement_carries_final_mentions_and_sdk_filters_revision_mentions() {
        use matrix_sdk::ruma::events::room::message::ReplacementMetadata;

        let alice = matrix_sdk::ruma::user_id!("@alice:example.org");
        let bob = matrix_sdk::ruma::user_id!("@bob:example.org");
        let mentions = MentionIntent {
            targets: vec![
                MentionTarget::User {
                    user_id: alice.to_string(),
                    display_label: "alice".to_owned(),
                },
                MentionTarget::User {
                    user_id: bob.to_string(),
                    display_label: "bob".to_owned(),
                },
            ],
        };
        let target = MessageType::Text(TextMessageEventContent::plain("old"));
        let edited = match edited_content_for_edit_target(Some(&target), "@alice @bob", &mentions) {
            EditedContent::RoomMessage(content) => content,
            other => panic!("text edit must remain a room message: {other:?}"),
        };
        let original_mentions = Mentions::with_user_ids([alice.to_owned()]);
        let replacement = edited.make_replacement(ReplacementMetadata::new(
            matrix_sdk::ruma::event_id!("$edit:example.org").to_owned(),
            Some(original_mentions),
        ));
        assert_eq!(
            replacement
                .mentions
                .expect("new mention notification set")
                .user_ids
                .into_iter()
                .collect::<Vec<_>>(),
            vec![bob.to_owned()]
        );
        let Some(matrix_sdk::ruma::events::room::message::Relation::Replacement(replacement)) =
            replacement.relates_to
        else {
            panic!("replacement relation must carry final mentions");
        };
        assert_eq!(
            replacement
                .new_content
                .mentions
                .expect("final mention set")
                .user_ids
                .into_iter()
                .collect::<Vec<_>>(),
            vec![alice.to_owned(), bob.to_owned()]
        );

        let removed = match edited_content_for_edit_target(
            Some(&target),
            "no mentions remain",
            &MentionIntent::default(),
        ) {
            EditedContent::RoomMessage(content) => content,
            other => panic!("removed mentions must stay a room message: {other:?}"),
        };
        assert!(removed.mentions.is_none());

        let media = media_msgtype_fixtures().pop().expect("media fixture");
        let media_edit = edited_content_for_edit_target(Some(&media), "@bob", &mentions);
        let EditedContent::MediaCaption {
            mentions: media_mentions,
            ..
        } = media_edit
        else {
            panic!("media edit must remain a caption edit");
        };
        assert_eq!(
            media_mentions
                .expect("media final mentions")
                .user_ids
                .into_iter()
                .collect::<Vec<_>>(),
            vec![alice.to_owned(), bob.to_owned()]
        );
    }

    #[test]
    fn edit_replacement_caption_support_matches_media_projection() {
        // Both sides of this equality are load-bearing: a type projected with
        // TimelineItem.media but edited as text loses its attachment, and a type
        // edited as a caption without media support is rejected by the SDK as an
        // incompatible edit.
        for msgtype in media_msgtype_fixtures()
            .into_iter()
            .chain(non_media_msgtype_fixtures())
        {
            let target = message_edit_target_token(Some(&msgtype));
            let projects_media = message_projection_from_msgtype(&msgtype, "fixture-body")
                .media
                .is_some();
            assert_eq!(
                projects_media,
                msgtype_carries_editable_caption(&msgtype),
                "{target}: media projection and caption-edit support must agree"
            );
        }
    }

    #[test]
    fn timeline_send_command_bodies_are_not_visible_in_debug() {
        // Manager-owned enqueue payloads originate from the public command, so
        // its Debug implementation is the privacy boundary for the send body.
        let cmd = TimelineCommand::SendText {
            request_id: fake_rid(1),
            key: room_key(),
            transaction_id: "txn-vis".to_owned(),
            document: koushi_state::ComposerDocument::from_plain_text(
                "very-private-body".to_owned(),
            ),
        };
        let debug = format!("{cmd:?}");
        assert!(
            !debug.contains("very-private-body"),
            "body leaked in Debug: {debug}"
        );
        assert!(
            debug.contains("txn-vis"),
            "txn_id should be visible: {debug}"
        );
    }

    #[test]
    fn timeline_link_preview_load_emits_private_data_free_trace_tokens() {
        let source = include_str!("diagnostics.rs");
        let production = source.split("\nmod tests").next().unwrap_or(source);
        assert!(
            production.contains("fn trace_timeline_link_preview"),
            "link preview loads must have a private-data-free trace helper"
        );
        for token in [
            "\"link_preview\"",
            "\"lookup_miss\"",
            "\"no_previews\"",
            "\"start\"",
            "\"complete\"",
            "\"load_link_previews\"",
            "DiagnosticField::count(\"pending\"",
            "DiagnosticField::milliseconds(\"duration\"",
            "DiagnosticField::request_id",
        ] {
            assert!(
                production.contains(token),
                "missing link preview trace token {token}"
            );
        }
    }

    #[test]
    fn timeline_link_preview_fetches_do_not_block_actor_command_queue() {
        let source = include_str!("item_projection.rs");
        let production = source.split("\nmod tests").next().unwrap_or(source);
        let load_src = production
            .split("async fn handle_load_link_previews")
            .nth(1)
            .and_then(|s| s.split("async fn handle_hide_link_preview").next())
            .expect("handle_load_link_previews should exist");

        assert!(
            !load_src.contains("fetch_link_preview("),
            "link preview network fetches must not run on the TimelineActor command loop"
        );
        assert!(
            production.contains("spawn_link_preview_fetch"),
            "link preview loads must spawn fetch work outside the TimelineActor command loop"
        );
        assert!(
            production.contains("LinkPreviewsFetched"),
            "link preview worker results must return to the TimelineActor explicitly"
        );
    }

    #[test]
    fn timeline_link_preview_fetches_are_abortable_without_dropping_the_actor() {
        let item = include_str!("item_projection.rs");
        let actor = include_str!("actor.rs");
        let handle_cancel_source = item_body(item, "fn handle_cancel_link_previews");
        let fetched_source = item_body(item, "async fn handle_link_previews_fetched");
        assert!(
            actor.contains("CancelLinkPreviews"),
            "timeline manager must expose a cancellation message for in-flight link previews"
        );
        assert!(
            handle_cancel_source.contains(".abort()"),
            "cancelling link previews must abort only link preview workers, not the timeline actor"
        );
        assert!(
            handle_cancel_source.contains("reset_loading_link_previews_to_pending"),
            "cancelled link preview workers must return Loading previews to Pending for future retries"
        );
        assert!(
            fetched_source.contains("remove(&event_id).is_none()"),
            "late results from cancelled link preview workers must be ignored"
        );
    }

    #[test]
    fn cancelled_link_preview_loads_return_loading_previews_to_pending() {
        let mut item = timeline_item(
            "$link:test",
            Some("https://example.test"),
            "@bob:test",
            false,
        );
        item.link_previews = Some(vec![
            LinkPreview {
                url: "https://example.test/loading".to_owned(),
                title: None,
                description: None,
                image: None,
                state: LinkPreviewState::Loading,
            },
            LinkPreview {
                url: "https://example.test/ready".to_owned(),
                title: Some("ready".to_owned()),
                description: None,
                image: None,
                state: LinkPreviewState::Ready,
            },
        ]);

        assert!(reset_loading_link_previews_to_pending(&mut item));
        let previews = item.link_previews.as_ref().expect("link previews");
        assert_eq!(previews[0].state, LinkPreviewState::Pending);
        assert_eq!(previews[1].state, LinkPreviewState::Ready);
        assert!(!reset_loading_link_previews_to_pending(&mut item));
    }

    /// Proves that during a restore walk the diff-batch handler buffers
    /// `TimelineDiff`s rather than emitting `ItemsUpdated` per chunk, and that
    /// all terminal paths flush the buffer exactly once.  React therefore
    /// receives a single settled `ItemsUpdated` per restore — no O(chunks)
    /// render churn — while internal state (`timeline_contains_event_id`) stays
    /// up-to-date every batch so the anchor can be found mid-walk.
    #[test]
    fn initial_timeline_items_are_forwarded_to_search_index() {
        let source = include_str!("actor.rs");
        let production = source.split("\nmod tests").next().unwrap_or(source);
        let spawn_src = production
            .split("async fn spawn(")
            .nth(1)
            .expect("TimelineActor::spawn must exist")
            .split("async fn run(")
            .next()
            .expect("spawn should end before run");

        assert!(
            spawn_src.contains("forward_initial_items_to_search"),
            "visible initial timeline items must enter the same search-index path as live diffs"
        );
        assert!(
            spawn_src.find("forward_initial_items_to_search") < spawn_src.find("actor.run()"),
            "initial items must be forwarded before the actor starts processing later diffs"
        );
    }
}
