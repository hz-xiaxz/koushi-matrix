use super::*;

pub enum AppCommand {
    Shutdown {
        request_id: RequestId,
    },
    SetComposerReplyTarget {
        request_id: RequestId,
        room_id: String,
        event_id: String,
    },
    CancelComposerReply {
        request_id: RequestId,
    },
    SetComposerDraft {
        request_id: RequestId,
        expected_account: koushi_key::SessionKeyId,
        room_id: String,
        document: ComposerDocument,
        revision: ComposerDraftRevision,
    },
    SetThreadComposerDraft {
        request_id: RequestId,
        expected_account: koushi_key::SessionKeyId,
        room_id: String,
        root_event_id: String,
        document: ComposerDocument,
        revision: ComposerDraftRevision,
    },
    AcceptComposerDraft {
        request_id: RequestId,
        expected_account: koushi_key::SessionKeyId,
        target: koushi_state::ComposerTarget,
        submitted_revision: ComposerDraftRevision,
    },
    SetUploadStaging {
        request_id: RequestId,
        target: koushi_state::ComposerTarget,
        items: Vec<StagedUploadItem>,
    },
    UpdateStagedUploadCaption {
        request_id: RequestId,
        target: koushi_state::ComposerTarget,
        staged_id: String,
        caption: Option<FormattedMessageDraft>,
    },
    UpdateStagedUploadCompression {
        request_id: RequestId,
        target: koushi_state::ComposerTarget,
        staged_id: String,
        compression_choice: StagedUploadCompressionChoice,
    },
    SelectStagedUploadOutput {
        request_id: RequestId,
        target: koushi_state::ComposerTarget,
        staged_id: String,
        selection: koushi_state::StagedUploadOutputSelection,
    },
    ClearUploadStaging {
        request_id: RequestId,
        target: koushi_state::ComposerTarget,
    },
    ScheduleSend {
        request_id: RequestId,
        expected_account: koushi_key::SessionKeyId,
        room_id: String,
        thread_root_event_id: Option<String>,
        body: String,
        send_at_ms: u64,
        draft_revision: ComposerDraftRevision,
    },
    CancelScheduledSend {
        request_id: RequestId,
        scheduled_id: String,
    },
    RescheduleScheduledSend {
        request_id: RequestId,
        scheduled_id: String,
        body: String,
        send_at_ms: u64,
    },
    OpenThread {
        request_id: RequestId,
        room_id: String,
        root_event_id: String,
        intent: koushi_state::ThreadOpenIntent,
    },
    CloseThread {
        request_id: RequestId,
    },
    OpenFocusedContext {
        request_id: RequestId,
        room_id: String,
        event_id: String,
    },
    /// Starts a main-pane Focused navigation whose anchor is withheld until
    /// the WebView acknowledges applying the matching actor projection.
    OpenAnchoredTimeline {
        request_id: RequestId,
        room_id: String,
        event_id: String,
        allow_live_fallback: bool,
    },
    /// Confirms that the canonical WebView timeline store applied one exact
    /// InitialItems projection. The projection request id remains stable when
    /// the active actor reprojects after a consumer remount.
    AcknowledgeTimelineProjection {
        request_id: RequestId,
        projection_request_id: RequestId,
        key: TimelineKey,
        generation: TimelineGeneration,
        item_count: u64,
        target_present: bool,
    },
    /// Confirms that the WebView committed a repair-produced timeline batch
    /// through layout. Every generation fence is required so a stale actor,
    /// timeline, repair, or batch cannot advance the repair scheduler.
    AcknowledgeTimelineBatchRendered {
        request_id: RequestId,
        key: TimelineKey,
        actor_generation: u64,
        timeline_generation: TimelineGeneration,
        repair_generation: u64,
        batch_id: TimelineBatchId,
    },
    EnterAnchoredTimeline {
        request_id: RequestId,
        room_id: String,
        event_id: String,
    },
    OpenTimelineAtTimestamp {
        request_id: RequestId,
        room_id: String,
        timestamp_ms: u64,
    },
    RepairRoomTimeline {
        request_id: RequestId,
        room_id: String,
    },
    TimelineScrollAnchorUpdated {
        request_id: RequestId,
        room_id: String,
        anchor: TimelineScrollAnchor,
    },
    CloseFocusedContext {
        request_id: RequestId,
    },
    CloseSearch {
        request_id: RequestId,
    },
    OpenInviteWorkflow {
        request_id: RequestId,
        room_id: String,
    },
    CloseInviteWorkflow {
        request_id: RequestId,
    },
    SearchInviteTargets {
        request_id: RequestId,
        room_id: String,
        query: String,
    },
    SetInviteScope {
        request_id: RequestId,
        room_id: String,
        scope: InviteScopeSelection,
    },
    SelectInviteTarget {
        request_id: RequestId,
        room_id: String,
        user_id: String,
    },
    RemoveInviteTarget {
        request_id: RequestId,
        user_id: String,
    },
    UpdateSettings {
        request_id: RequestId,
        patch: SettingsPatch,
    },
    RebuildSearchIndex {
        request_id: RequestId,
    },
    SetRoomUrlPreviewOverride {
        request_id: RequestId,
        room_id: String,
        enabled: bool,
    },
    OpenActivity {
        request_id: RequestId,
    },
    CloseActivity {
        request_id: RequestId,
    },
    SetActivityTab {
        request_id: RequestId,
        tab: ActivityTab,
    },
    PaginateActivity {
        request_id: RequestId,
        tab: ActivityTab,
        cursor: Option<String>,
    },
    RetryActivityResolution {
        request_id: RequestId,
    },
    MarkActivityRead {
        request_id: RequestId,
        target: ActivityMarkReadTarget,
    },
    OpenFilesView {
        request_id: RequestId,
        scope: FilesViewScope,
        filter: AttachmentFilter,
        sort: AttachmentSort,
    },
    CloseFilesView {
        request_id: RequestId,
    },
    OpenThreadsList {
        request_id: RequestId,
        scope: koushi_state::ThreadsListScope,
    },
    CloseThreadsList {
        request_id: RequestId,
    },
    PaginateThreadsList {
        request_id: RequestId,
        scope: koushi_state::ThreadsListScope,
    },
    RecordLocalEncryptionHealth {
        request_id: RequestId,
        health: LocalEncryptionHealth,
    },
    UpdateNativeAttentionState {
        request_id: RequestId,
        attention: NativeAttentionState,
    },
    ObserveNativeWindowFocus {
        request_id: RequestId,
        focused: bool,
        observation_generation: u64,
    },
    StartNativeAttentionDispatch {
        request_id: RequestId,
        dispatch_id: NativeAttentionDispatchId,
    },
    SettleNativeAttentionDispatch {
        request_id: RequestId,
        dispatch_id: NativeAttentionDispatchId,
        outcome: NativeAttentionSoundOutcome,
    },
    UpdateJapaneseCatalogProfile {
        request_id: RequestId,
        profile: JapaneseCatalogProfile,
    },
    SelectRoomListFilter {
        request_id: RequestId,
        filter: RoomListFilter,
    },
}

impl fmt::Debug for AppCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shutdown { request_id } => formatter
                .debug_struct("Shutdown")
                .field("request_id", request_id)
                .finish(),
            Self::SetComposerReplyTarget {
                request_id,
                room_id,
                ..
            } => formatter
                .debug_struct("SetComposerReplyTarget")
                .field("request_id", request_id)
                .field("room_id", room_id)
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::CancelComposerReply { request_id } => formatter
                .debug_struct("CancelComposerReply")
                .field("request_id", request_id)
                .finish(),
            Self::SetComposerDraft {
                request_id,
                room_id,
                ..
            } => formatter
                .debug_struct("SetComposerDraft")
                .field("request_id", request_id)
                .field("room_id", room_id)
                .field("draft", &"MessageBody(..)")
                .finish(),
            Self::SetThreadComposerDraft {
                request_id,
                room_id,
                ..
            } => formatter
                .debug_struct("SetThreadComposerDraft")
                .field("request_id", request_id)
                .field("room_id", room_id)
                .field("root_event_id", &"EventId(..)")
                .field("draft", &"MessageBody(..)")
                .finish(),
            Self::AcceptComposerDraft { request_id, .. } => formatter
                .debug_struct("AcceptComposerDraft")
                .field("request_id", request_id)
                .field("target", &"ComposerTarget(..)")
                .finish(),
            Self::SetUploadStaging {
                request_id, items, ..
            } => formatter
                .debug_struct("SetUploadStaging")
                .field("request_id", request_id)
                .field("target", &"ComposerTarget(..)")
                .field("item_count", &items.len())
                .finish(),
            Self::UpdateStagedUploadCaption { request_id, .. } => formatter
                .debug_struct("UpdateStagedUploadCaption")
                .field("request_id", request_id)
                .field("staged_id", &"StagedUploadId(..)")
                .field("caption", &"MediaCaption(..)")
                .finish(),
            Self::UpdateStagedUploadCompression {
                request_id,
                compression_choice,
                ..
            } => formatter
                .debug_struct("UpdateStagedUploadCompression")
                .field("request_id", request_id)
                .field("staged_id", &"StagedUploadId(..)")
                .field("compression_choice", compression_choice)
                .finish(),
            Self::SelectStagedUploadOutput {
                request_id,
                selection,
                ..
            } => formatter
                .debug_struct("SelectStagedUploadOutput")
                .field("request_id", request_id)
                .field("target", &"ComposerTarget(..)")
                .field("staged_id", &"StagedUploadId(..)")
                // The chosen axes are not private data; the filename is.
                .field("selection", selection)
                .finish(),
            Self::ClearUploadStaging { request_id, .. } => formatter
                .debug_struct("ClearUploadStaging")
                .field("request_id", request_id)
                .field("target", &"ComposerTarget(..)")
                .finish(),
            Self::ScheduleSend {
                request_id,
                send_at_ms,
                ..
            } => formatter
                .debug_struct("ScheduleSend")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("body", &"MessageBody(..)")
                .field("send_at_ms", &send_at_ms)
                .finish(),
            Self::CancelScheduledSend {
                request_id,
                scheduled_id,
            } => formatter
                .debug_struct("CancelScheduledSend")
                .field("request_id", request_id)
                .field("scheduled_id", scheduled_id)
                .finish(),
            Self::RescheduleScheduledSend {
                request_id,
                scheduled_id,
                body: _,
                send_at_ms,
            } => formatter
                .debug_struct("RescheduleScheduledSend")
                .field("request_id", request_id)
                .field("scheduled_id", scheduled_id)
                .field("send_at_ms", send_at_ms)
                .finish(),
            Self::OpenThread {
                request_id, intent, ..
            } => formatter
                .debug_struct("OpenThread")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("root_event_id", &"EventId(..)")
                .field("intent", intent)
                .finish(),
            Self::CloseThread { request_id } => formatter
                .debug_struct("CloseThread")
                .field("request_id", request_id)
                .finish(),
            Self::OpenFocusedContext {
                request_id,
                room_id,
                ..
            } => formatter
                .debug_struct("OpenFocusedContext")
                .field("request_id", request_id)
                .field("room_id", room_id)
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::OpenAnchoredTimeline { request_id, .. } => formatter
                .debug_struct("OpenAnchoredTimeline")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::AcknowledgeTimelineProjection {
                request_id,
                projection_request_id,
                generation,
                item_count,
                target_present,
                ..
            } => formatter
                .debug_struct("AcknowledgeTimelineProjection")
                .field("request_id", request_id)
                .field("projection_request_id", projection_request_id)
                .field("key", &"TimelineKey(..)")
                .field("generation", generation)
                .field("item_count", item_count)
                .field("target_present", target_present)
                .finish(),
            Self::AcknowledgeTimelineBatchRendered {
                request_id,
                actor_generation,
                timeline_generation,
                repair_generation,
                batch_id,
                ..
            } => formatter
                .debug_struct("AcknowledgeTimelineBatchRendered")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("actor_generation", actor_generation)
                .field("timeline_generation", timeline_generation)
                .field("repair_generation", repair_generation)
                .field("batch_id", batch_id)
                .finish(),
            Self::EnterAnchoredTimeline {
                request_id,
                room_id,
                ..
            } => formatter
                .debug_struct("EnterAnchoredTimeline")
                .field("request_id", request_id)
                .field("room_id", room_id)
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::OpenTimelineAtTimestamp { request_id, .. } => formatter
                .debug_struct("OpenTimelineAtTimestamp")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("timestamp_ms", &"Timestamp(..)")
                .finish(),
            Self::RepairRoomTimeline { request_id, .. } => formatter
                .debug_struct("RepairRoomTimeline")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::TimelineScrollAnchorUpdated {
                request_id, anchor, ..
            } => formatter
                .debug_struct("TimelineScrollAnchorUpdated")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("event_id", &"EventId(..)")
                .field("offset_px", &anchor.offset_px)
                .field("updated_at_ms", &anchor.updated_at_ms)
                .finish(),
            Self::CloseFocusedContext { request_id } => formatter
                .debug_struct("CloseFocusedContext")
                .field("request_id", request_id)
                .finish(),
            Self::CloseSearch { request_id } => formatter
                .debug_struct("CloseSearch")
                .field("request_id", request_id)
                .finish(),
            Self::OpenInviteWorkflow { request_id, .. } => formatter
                .debug_struct("OpenInviteWorkflow")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::CloseInviteWorkflow { request_id } => formatter
                .debug_struct("CloseInviteWorkflow")
                .field("request_id", request_id)
                .finish(),
            Self::SearchInviteTargets {
                request_id, query, ..
            } => formatter
                .debug_struct("SearchInviteTargets")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("query_len", &query.len())
                .finish(),
            Self::SetInviteScope {
                request_id, scope, ..
            } => formatter
                .debug_struct("SetInviteScope")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("scope", scope)
                .finish(),
            Self::SelectInviteTarget { request_id, .. } => formatter
                .debug_struct("SelectInviteTarget")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("user_id", &"UserId(..)")
                .finish(),
            Self::RemoveInviteTarget { request_id, .. } => formatter
                .debug_struct("RemoveInviteTarget")
                .field("request_id", request_id)
                .field("user_id", &"UserId(..)")
                .finish(),
            Self::UpdateSettings { request_id, patch } => formatter
                .debug_struct("UpdateSettings")
                .field("request_id", request_id)
                .field("patch_fields", &settings_patch_field_names(patch))
                .finish(),
            Self::RebuildSearchIndex { request_id } => formatter
                .debug_struct("RebuildSearchIndex")
                .field("request_id", request_id)
                .finish(),
            Self::SetRoomUrlPreviewOverride {
                request_id,
                enabled,
                ..
            } => formatter
                .debug_struct("SetRoomUrlPreviewOverride")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .field("enabled", enabled)
                .finish(),
            Self::OpenActivity { request_id } => formatter
                .debug_struct("OpenActivity")
                .field("request_id", request_id)
                .finish(),
            Self::CloseActivity { request_id } => formatter
                .debug_struct("CloseActivity")
                .field("request_id", request_id)
                .finish(),
            Self::SetActivityTab { request_id, tab } => formatter
                .debug_struct("SetActivityTab")
                .field("request_id", request_id)
                .field("tab", tab)
                .finish(),
            Self::PaginateActivity {
                request_id,
                tab,
                cursor,
            } => formatter
                .debug_struct("PaginateActivity")
                .field("request_id", request_id)
                .field("tab", tab)
                .field("cursor", &cursor.as_ref().map(|_| "PageToken(..)"))
                .finish(),
            Self::RetryActivityResolution { request_id } => formatter
                .debug_struct("RetryActivityResolution")
                .field("request_id", request_id)
                .finish(),
            Self::MarkActivityRead { request_id, target } => formatter
                .debug_struct("MarkActivityRead")
                .field("request_id", request_id)
                .field("target", target)
                .finish(),
            Self::OpenFilesView {
                request_id,
                scope,
                filter,
                sort,
            } => formatter
                .debug_struct("OpenFilesView")
                .field("request_id", request_id)
                .field("scope", scope)
                .field("filter", filter)
                .field("sort", sort)
                .finish(),
            Self::CloseFilesView { request_id } => formatter
                .debug_struct("CloseFilesView")
                .field("request_id", request_id)
                .finish(),
            Self::OpenThreadsList { request_id, .. } => formatter
                .debug_struct("OpenThreadsList")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::CloseThreadsList { request_id } => formatter
                .debug_struct("CloseThreadsList")
                .field("request_id", request_id)
                .finish(),
            Self::PaginateThreadsList { request_id, .. } => formatter
                .debug_struct("PaginateThreadsList")
                .field("request_id", request_id)
                .field("room_id", &"RoomId(..)")
                .finish(),
            Self::RecordLocalEncryptionHealth { request_id, health } => formatter
                .debug_struct("RecordLocalEncryptionHealth")
                .field("request_id", request_id)
                .field("health", health)
                .finish(),
            Self::UpdateNativeAttentionState {
                request_id,
                attention,
            } => formatter
                .debug_struct("UpdateNativeAttentionState")
                .field("request_id", request_id)
                .field("unread_count", &attention.summary.unread_count)
                .field("highlight_count", &attention.summary.highlight_count)
                .field("badge_count", &attention.summary.badge_count)
                .field("dispatch", &attention.dispatch.kind())
                .field(
                    "candidate",
                    &attention
                        .summary
                        .candidate
                        .as_ref()
                        .map(|_| "AttentionCandidate(..)"),
                )
                .finish(),
            Self::ObserveNativeWindowFocus {
                request_id,
                focused,
                observation_generation,
            } => formatter
                .debug_struct("ObserveNativeWindowFocus")
                .field("request_id", request_id)
                .field("focused", focused)
                .field("observation_generation", observation_generation)
                .finish(),
            Self::StartNativeAttentionDispatch {
                request_id,
                dispatch_id,
            } => formatter
                .debug_struct("StartNativeAttentionDispatch")
                .field("request_id", request_id)
                .field("dispatch_id", dispatch_id)
                .finish(),
            Self::SettleNativeAttentionDispatch {
                request_id,
                dispatch_id,
                outcome,
            } => formatter
                .debug_struct("SettleNativeAttentionDispatch")
                .field("request_id", request_id)
                .field("dispatch_id", dispatch_id)
                .field("outcome", outcome)
                .finish(),
            Self::UpdateJapaneseCatalogProfile {
                request_id,
                profile,
            } => formatter
                .debug_struct("UpdateJapaneseCatalogProfile")
                .field("request_id", request_id)
                .field("catalog_locale", &profile.catalog_locale)
                .field("complete", &profile.complete)
                .field("missing_count", &profile.missing_message_ids.len())
                .finish(),
            Self::SelectRoomListFilter { request_id, filter } => formatter
                .debug_struct("SelectRoomListFilter")
                .field("request_id", request_id)
                .field("filter", filter)
                .finish(),
        }
    }
}

fn settings_patch_field_names(patch: &SettingsPatch) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if patch.locale.is_some() {
        fields.push("locale");
    }
    if patch.appearance.is_some() {
        fields.push("appearance");
    }
    if patch.typography.is_some() {
        fields.push("typography");
    }
    if patch.keyboard.is_some() {
        fields.push("keyboard");
    }
    if patch.composer.is_some() {
        fields.push("composer");
    }
    if patch.notifications.is_some() {
        fields.push("notifications");
    }
    if patch.display.is_some() {
        fields.push("display");
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::super::test_support::fake_rid;
    use super::*;
    use koushi_state::{
        ImageUploadCompressionMode, MentionIntent, NativeAttentionCandidate,
        NativeAttentionCapabilities, NativeAttentionCapability, NativeAttentionDispatchState,
        NativeAttentionState, NativeAttentionSummary, NativeAttentionSuppressionReason,
        RoomAttentionKind, ThreadOpenIntent,
    };

    #[test]
    fn open_thread_command_retains_typed_intent_and_redacts_identifiers() {
        let request_id = fake_rid(75);
        let command = AppCommand::OpenThread {
            request_id,
            room_id: "!private-room:example.invalid".to_owned(),
            root_event_id: "$private-root:example.invalid".to_owned(),
            intent: ThreadOpenIntent::NewThreadDraft,
        };

        assert_eq!(CoreCommand::App(command).request_id(), request_id);
        let debug = format!(
            "{:?}",
            AppCommand::OpenThread {
                request_id,
                room_id: "!private-room:example.invalid".to_owned(),
                root_event_id: "$private-root:example.invalid".to_owned(),
                intent: ThreadOpenIntent::NewThreadDraft,
            }
        );
        assert!(debug.contains("NewThreadDraft"), "{debug}");
        assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
        assert!(!debug.contains("$private-root:example.invalid"), "{debug}");
    }

    #[test]
    fn set_room_url_preview_override_debug_redacts_room_id() {
        let command = AppCommand::SetRoomUrlPreviewOverride {
            request_id: fake_rid(14),
            room_id: "!room:example.invalid".to_owned(),
            enabled: false,
        };
        let debug = format!("{command:?}");
        assert!(debug.contains("SetRoomUrlPreviewOverride"), "{debug}");
        assert!(debug.contains("RoomId(..)"), "{debug}");
        assert!(debug.contains("enabled"), "{debug}");
        assert!(!debug.contains("!room:example.invalid"), "{debug}");
    }

    #[test]
    fn activity_commands_debug_redacts_targets_and_carry_request_ids() {
        use koushi_state::{ActivityMarkReadTarget, ActivityTab};

        let set_tab_request_id = fake_rid(21);
        let set_tab = AppCommand::SetActivityTab {
            request_id: set_tab_request_id,
            tab: ActivityTab::Unread,
        };
        let paginate_request_id = fake_rid(22);
        let paginate = AppCommand::PaginateActivity {
            request_id: paginate_request_id,
            tab: ActivityTab::Recent,
            cursor: Some("private-page-token".to_owned()),
        };
        let mark_request_id = fake_rid(23);
        let mark = AppCommand::MarkActivityRead {
            request_id: mark_request_id,
            target: ActivityMarkReadTarget::Room {
                room_id: "!private-room:example.invalid".to_owned(),
                up_to_event_id: "$private-event:example.invalid".to_owned(),
            },
        };

        assert_eq!(CoreCommand::App(set_tab).request_id(), set_tab_request_id);
        assert_eq!(CoreCommand::App(paginate).request_id(), paginate_request_id);
        assert_eq!(
            CoreCommand::App(AppCommand::MarkActivityRead {
                request_id: mark_request_id,
                target: ActivityMarkReadTarget::All,
            })
            .request_id(),
            mark_request_id
        );

        for debug in [
            format!(
                "{:?}",
                AppCommand::PaginateActivity {
                    request_id: fake_rid(24),
                    tab: ActivityTab::Unread,
                    cursor: Some("private-page-token".to_owned()),
                }
            ),
            format!("{mark:?}"),
        ] {
            assert!(!debug.contains("private-page-token"), "{debug}");
            assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
            assert!(!debug.contains("$private-event:example.invalid"), "{debug}");
        }
    }

    #[test]
    fn upload_staging_commands_require_ready_session_and_redact_debug() {
        use koushi_state::{StagedUploadKind, build_formatted_message_draft};

        let set_request_id = fake_rid(24);
        let update_caption_request_id = fake_rid(25);
        let update_compression_request_id = fake_rid(26);
        let clear_request_id = fake_rid(27);
        let target = koushi_state::ComposerTarget::Main {
            room_id: "!private-room:example.invalid".to_owned(),
        };
        let set = AppCommand::SetUploadStaging {
            request_id: set_request_id,
            target: target.clone(),
            items: vec![StagedUploadItem {
                staged_id: "private-staged-id".to_owned(),
                room_id: "!private-room:example.invalid".to_owned(),
                position: 1,
                filename: "private-image.png".to_owned(),
                mime_type: "image/png".to_owned(),
                byte_count: 99,
                kind: StagedUploadKind::Image {
                    width: Some(4),
                    height: Some(2),
                },
                caption: Some(build_formatted_message_draft(
                    "private staged caption",
                    MentionIntent::default(),
                )),
                compression_choice: StagedUploadCompressionChoice::Original,
                preparation: Default::default(),
            }],
        };
        let update_caption = AppCommand::UpdateStagedUploadCaption {
            request_id: update_caption_request_id,
            target: target.clone(),
            staged_id: "private-staged-id".to_owned(),
            caption: Some(build_formatted_message_draft(
                "private staged caption",
                MentionIntent::default(),
            )),
        };
        let update_compression = AppCommand::UpdateStagedUploadCompression {
            request_id: update_compression_request_id,
            target: target.clone(),
            staged_id: "private-staged-id".to_owned(),
            compression_choice: StagedUploadCompressionChoice::Compressed {
                mode: ImageUploadCompressionMode::Always,
            },
        };
        let clear = AppCommand::ClearUploadStaging {
            request_id: clear_request_id,
            target: target.clone(),
        };

        assert_eq!(CoreCommand::App(set).request_id(), set_request_id);
        for command in [
            AppCommand::SetUploadStaging {
                request_id: set_request_id,
                target: target.clone(),
                items: Vec::new(),
            },
            AppCommand::UpdateStagedUploadCaption {
                request_id: update_caption_request_id,
                target: target.clone(),
                staged_id: "private-staged-id".to_owned(),
                caption: None,
            },
            AppCommand::UpdateStagedUploadCompression {
                request_id: update_compression_request_id,
                target: target.clone(),
                staged_id: "private-staged-id".to_owned(),
                compression_choice: StagedUploadCompressionChoice::Original,
            },
            AppCommand::ClearUploadStaging {
                request_id: clear_request_id,
                target: target.clone(),
            },
        ] {
            assert!(CoreCommand::App(command).requires_ready_session());
        }

        for debug in [
            format!("{update_caption:?}"),
            format!("{update_compression:?}"),
            format!("{clear:?}"),
            format!(
                "{:?}",
                AppCommand::SetUploadStaging {
                    request_id: set_request_id,
                    target,
                    items: vec![StagedUploadItem {
                        staged_id: "private-staged-id".to_owned(),
                        room_id: "!private-room:example.invalid".to_owned(),
                        position: 1,
                        filename: "private-image.png".to_owned(),
                        mime_type: "image/png".to_owned(),
                        byte_count: 99,
                        kind: StagedUploadKind::File,
                        caption: None,
                        compression_choice: StagedUploadCompressionChoice::NotApplicable,
                        preparation: Default::default(),
                    }],
                }
            ),
        ] {
            assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
            assert!(!debug.contains("private-staged-id"), "{debug}");
            assert!(!debug.contains("private-image.png"), "{debug}");
            assert!(!debug.contains("private staged caption"), "{debug}");
        }
    }

    #[test]
    fn open_timeline_at_timestamp_requires_ready_session_and_redacts_debug() {
        let request_id = fake_rid(28);
        let command = AppCommand::OpenTimelineAtTimestamp {
            request_id,
            room_id: "!private-room:example.invalid".to_owned(),
            timestamp_ms: 1_718_000_000_000,
        };

        assert_eq!(CoreCommand::App(command).request_id(), request_id);
        assert!(
            CoreCommand::App(AppCommand::OpenTimelineAtTimestamp {
                request_id,
                room_id: "!private-room:example.invalid".to_owned(),
                timestamp_ms: 1_718_000_000_000,
            })
            .requires_ready_session()
        );
        let debug = format!(
            "{:?}",
            AppCommand::OpenTimelineAtTimestamp {
                request_id,
                room_id: "!private-room:example.invalid".to_owned(),
                timestamp_ms: 1_718_000_000_000,
            }
        );
        assert!(debug.contains("RoomId(..)"), "{debug}");
        assert!(debug.contains("Timestamp(..)"), "{debug}");
        assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
        assert!(!debug.contains("1718000000000"), "{debug}");
    }

    #[test]
    fn focused_projection_commands_redact_matrix_identifiers() {
        let request_id = fake_rid(29);
        let key = TimelineKey {
            account_key: AccountKey("@private:example.invalid".to_owned()),
            kind: crate::ids::TimelineKind::Focused {
                room_id: "!private-room:example.invalid".to_owned(),
                event_id: "$private-event:example.invalid".to_owned(),
            },
        };
        let debug = format!(
            "{:?} {:?}",
            AppCommand::OpenAnchoredTimeline {
                request_id,
                room_id: "!private-room:example.invalid".to_owned(),
                event_id: "$private-event:example.invalid".to_owned(),
                allow_live_fallback: false,
            },
            AppCommand::AcknowledgeTimelineProjection {
                request_id,
                projection_request_id: fake_rid(28),
                key,
                generation: TimelineGeneration(3),
                item_count: 7,
                target_present: true,
            }
        );
        assert!(debug.contains("RoomId(..)"), "{debug}");
        assert!(debug.contains("TimelineKey(..)"), "{debug}");
        for private in [
            "@private:example.invalid",
            "!private-room:example.invalid",
            "$private-event:example.invalid",
        ] {
            assert!(!debug.contains(private), "{debug}");
        }
    }

    #[test]
    fn acknowledge_timeline_batch_rendered_preserves_fences_and_redacts_key() {
        let request_id = fake_rid(30);
        let command = AppCommand::AcknowledgeTimelineBatchRendered {
            request_id,
            key: TimelineKey {
                account_key: AccountKey("@private:example.invalid".to_owned()),
                kind: crate::ids::TimelineKind::Room {
                    room_id: "!private-room:example.invalid".to_owned(),
                },
            },
            actor_generation: 9,
            timeline_generation: TimelineGeneration(3),
            repair_generation: 11,
            batch_id: crate::ids::TimelineBatchId(5),
        };

        assert_eq!(CoreCommand::App(command).request_id(), request_id);
        let debug = format!(
            "{:?}",
            AppCommand::AcknowledgeTimelineBatchRendered {
                request_id,
                key: TimelineKey {
                    account_key: AccountKey("@private:example.invalid".to_owned()),
                    kind: crate::ids::TimelineKind::Room {
                        room_id: "!private-room:example.invalid".to_owned(),
                    },
                },
                actor_generation: 9,
                timeline_generation: TimelineGeneration(3),
                repair_generation: 11,
                batch_id: crate::ids::TimelineBatchId(5),
            }
        );
        for expected in [
            "actor_generation: 9",
            "repair_generation: 11",
            "TimelineBatchId(5)",
        ] {
            assert!(debug.contains(expected), "{debug}");
        }
        assert!(debug.contains("TimelineKey(..)"), "{debug}");
        assert!(!debug.contains("@private:example.invalid"), "{debug}");
        assert!(!debug.contains("!private-room:example.invalid"), "{debug}");
    }

    #[test]
    fn native_attention_command_debug_redacts_candidate_labels() {
        let command = AppCommand::UpdateNativeAttentionState {
            request_id: fake_rid(27),
            attention: NativeAttentionState {
                summary: NativeAttentionSummary {
                    unread_count: 4,
                    highlight_count: 1,
                    badge_count: 4,
                    candidate: Some(NativeAttentionCandidate {
                        room_display_name: "Private Room Name".to_owned(),
                        kind: RoomAttentionKind::Mention,
                        unread_count: 4,
                        highlight_count: 1,
                    }),
                    capabilities: NativeAttentionCapabilities {
                        notifications: NativeAttentionCapability::Available,
                        badge: NativeAttentionCapability::Available,
                        overlay_icon: NativeAttentionCapability::Unknown,
                        sound: NativeAttentionCapability::Unknown,
                        tray: NativeAttentionCapability::Unavailable,
                        activation: NativeAttentionCapability::Unknown,
                    },
                },
                dispatch: NativeAttentionDispatchState::Suppressed {
                    reason: NativeAttentionSuppressionReason::WindowFocused,
                },
            },
        };

        let debug = format!("{command:?}");

        assert!(debug.contains("UpdateNativeAttentionState"), "{debug}");
        assert!(debug.contains("unread_count"), "{debug}");
        assert!(debug.contains("suppressed"), "{debug}");
        assert!(!debug.contains("Private Room Name"), "{debug}");
    }

    #[test]
    fn observe_native_window_focus_command_is_correlated_and_private_safe() {
        let request_id = fake_rid(28);
        let command = AppCommand::ObserveNativeWindowFocus {
            request_id,
            focused: false,
            observation_generation: 7,
        };

        assert_eq!(
            CoreCommand::App(command).request_id(),
            request_id,
            "focus observation must preserve command correlation"
        );
        let debug = format!(
            "{:?}",
            AppCommand::ObserveNativeWindowFocus {
                request_id,
                focused: false,
                observation_generation: 7,
            }
        );
        assert!(debug.contains("ObserveNativeWindowFocus"), "{debug}");
        assert!(debug.contains("focused: false"), "{debug}");
        assert!(debug.contains("observation_generation: 7"), "{debug}");
        assert!(!debug.contains("room_id"), "{debug}");
        assert!(!debug.contains("event_id"), "{debug}");
        assert!(!debug.contains("user_id"), "{debug}");
    }
}
