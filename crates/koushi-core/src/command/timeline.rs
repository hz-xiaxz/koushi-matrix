use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UploadMediaKind {
    Image {
        width: Option<u64>,
        height: Option<u64>,
    },
    File,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageUploadDimensions {
    pub width: u64,
    pub height: u64,
}

impl ImageUploadDimensions {
    pub fn long_edge(self) -> u64 {
        self.width.max(self.height)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageUploadCompressionPolicy {
    pub threshold_bytes: u64,
    pub threshold_long_edge: u64,
    pub target_long_edge: u64,
    pub quality_percent: u8,
}

impl Default for ImageUploadCompressionPolicy {
    fn default() -> Self {
        Self {
            threshold_bytes: 1_048_576,
            threshold_long_edge: 2560,
            target_long_edge: 2048,
            quality_percent: 82,
        }
    }
}

impl ImageUploadCompressionPolicy {
    pub fn should_skip(self, info: &ImageUploadVariantInfo) -> bool {
        if info.byte_count > self.threshold_bytes {
            return false;
        }
        match info.dimensions {
            Some(dimensions) => dimensions.long_edge() <= self.threshold_long_edge,
            None => true,
        }
    }

    pub fn target_dimensions_for(self, dimensions: ImageUploadDimensions) -> ImageUploadDimensions {
        let long_edge = dimensions.long_edge();
        if long_edge == 0 || long_edge <= self.target_long_edge {
            return dimensions;
        }

        ImageUploadDimensions {
            width: scale_dimension(dimensions.width, long_edge, self.target_long_edge),
            height: scale_dimension(dimensions.height, long_edge, self.target_long_edge),
        }
    }
}

fn scale_dimension(value: u64, source_long_edge: u64, target_long_edge: u64) -> u64 {
    if value == 0 || source_long_edge == 0 {
        return value;
    }
    let numerator = u128::from(value) * u128::from(target_long_edge);
    let denominator = u128::from(source_long_edge);
    let rounded = (numerator + (denominator / 2)) / denominator;
    u64::try_from(rounded.max(1)).unwrap_or(u64::MAX)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageUploadVariantInfo {
    pub mime_type: String,
    pub byte_count: u64,
    pub dimensions: Option<ImageUploadDimensions>,
}

impl ImageUploadVariantInfo {
    pub fn selected(
        mime_type: String,
        byte_count: u64,
        dimensions: Option<ImageUploadDimensions>,
    ) -> Self {
        Self {
            mime_type,
            byte_count,
            dimensions,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ImageUploadVariantKind {
    Original,
    Compressed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageUploadCompressionState {
    pub mode: ImageUploadCompressionMode,
    pub policy: ImageUploadCompressionPolicy,
    pub original: ImageUploadVariantInfo,
    pub selected: ImageUploadVariantInfo,
    pub selected_variant: ImageUploadVariantKind,
    pub skipped_small_image: bool,
    pub metadata_stripped: bool,
    pub thumbnail_refreshed: bool,
}

impl ImageUploadCompressionState {
    pub fn original(
        mode: ImageUploadCompressionMode,
        mime_type: String,
        byte_count: u64,
        dimensions: Option<ImageUploadDimensions>,
    ) -> Self {
        let policy = ImageUploadCompressionPolicy::default();
        let original = ImageUploadVariantInfo::selected(mime_type, byte_count, dimensions);
        let skipped_small_image = policy.should_skip(&original);
        Self {
            mode,
            policy,
            original: original.clone(),
            selected: original,
            selected_variant: ImageUploadVariantKind::Original,
            skipped_small_image,
            metadata_stripped: false,
            thumbnail_refreshed: false,
        }
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct UploadMediaThumbnail {
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub width: u64,
    pub height: u64,
}

impl fmt::Debug for UploadMediaThumbnail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadMediaThumbnail")
            .field("mime_type", &self.mime_type)
            .field("bytes", &"ThumbnailBytes(..)")
            .field("bytes_len", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct UploadMediaRequest {
    pub filename: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub kind: UploadMediaKind,
    pub compression: Option<ImageUploadCompressionState>,
    pub thumbnail: Option<UploadMediaThumbnail>,
    pub caption: Option<FormattedMessageDraft>,
}

impl fmt::Debug for UploadMediaRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadMediaRequest")
            .field("filename", &"MediaFilename(..)")
            .field("mime_type", &self.mime_type)
            .field("bytes", &"MediaBytes(..)")
            .field("bytes_len", &self.bytes.len())
            .field("kind", &self.kind)
            .field("compression", &self.compression)
            .field("thumbnail", &self.thumbnail)
            .field(
                "caption",
                &self.caption.as_ref().map(|_| "MediaCaption(..)"),
            )
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MediaDownloadSelection {
    File,
    Thumbnail { width: u64, height: u64 },
}

/// Presentation origin of a manual room-key request (issue #460): only
/// user-triggered requests publish the "sent" toast; automatic recovery
/// requests stay silent in the UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyRequestOrigin {
    User,
    Automatic,
}

pub enum TimelineCommand {
    Subscribe {
        request_id: RequestId,
        key: TimelineKey,
    },
    EnsureSubscribed {
        request_id: RequestId,
        key: TimelineKey,
        replay_existing: bool,
    },
    ReplaySubscribed {
        request_id: RequestId,
    },
    Unsubscribe {
        request_id: RequestId,
        key: TimelineKey,
    },
    Paginate {
        request_id: RequestId,
        key: TimelineKey,
        direction: crate::event::PaginationDirection,
        event_count: u16,
    },
    CancelPagination {
        request_id: RequestId,
        key: TimelineKey,
    },
    CancelLinkPreviews {
        request_id: RequestId,
        key: TimelineKey,
    },
    RestoreTimelineAnchor {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        max_batches: u16,
        event_count: u16,
    },
    ObserveViewport {
        request_id: RequestId,
        key: TimelineKey,
        observation: TimelineViewportObservation,
    },
    RepairGaps {
        request_id: RequestId,
        key: TimelineKey,
    },
    SendText {
        request_id: RequestId,
        key: TimelineKey,
        transaction_id: String,
        document: ComposerDocument,
    },
    SubmitText {
        request_id: RequestId,
        expected_account: koushi_key::SessionKeyId,
        submission_id: SubmissionId,
        key: TimelineKey,
        transaction_id: String,
        document: ComposerDocument,
        draft_revision: ComposerDraftRevision,
    },
    SendReply {
        request_id: RequestId,
        key: TimelineKey,
        transaction_id: String,
        in_reply_to_event_id: String,
        document: ComposerDocument,
    },
    SubmitReply {
        request_id: RequestId,
        expected_account: koushi_key::SessionKeyId,
        submission_id: SubmissionId,
        key: TimelineKey,
        transaction_id: String,
        in_reply_to_event_id: String,
        document: ComposerDocument,
        draft_revision: ComposerDraftRevision,
    },
    ForwardMessage {
        request_id: RequestId,
        key: TimelineKey,
        source_event_id: String,
        destination_room_id: String,
        transaction_id: String,
    },
    LoadMessageSource {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
    },
    RequestRoomKey {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        origin: KeyRequestOrigin,
    },
    RequestLateDecryption {
        request_id: RequestId,
        key: TimelineKey,
    },
    RetrySend {
        request_id: RequestId,
        key: TimelineKey,
        transaction_id: String,
    },
    CancelSend {
        request_id: RequestId,
        key: TimelineKey,
        transaction_id: String,
    },
    UploadAndSendMedia {
        request_id: RequestId,
        expected_account: koushi_key::SessionKeyId,
        key: TimelineKey,
        transaction_id: String,
        request: UploadMediaRequest,
    },
    DownloadMedia {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        selection: MediaDownloadSelection,
    },
    EditText {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        document: ComposerDocument,
    },
    Redact {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
    },
    ToggleReaction {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        reaction_key: String,
    },
    SendReaction {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        reaction_key: String,
    },
    RedactReaction {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
        reaction_key: String,
        reaction_event_id: String,
    },
    SendReadReceipt {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
    },
    SetFullyRead {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
    },
    SetTyping {
        request_id: RequestId,
        key: TimelineKey,
        is_typing: bool,
    },
    LoadLinkPreviews {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
    },
    HideLinkPreview {
        request_id: RequestId,
        key: TimelineKey,
        event_id: String,
    },
    BroadcastLinkPreviewPolicy {
        unencrypted_global_enabled: bool,
        encrypted_global_enabled: bool,
        room_overrides: std::collections::BTreeMap<String, bool>,
    },
}

impl TimelineCommand {
    /// Complete account owner captured by composer-affecting commands.
    ///
    /// AppActor and AccountActor both revalidate this immediately before
    /// routing so an account switch cannot redirect an already-issued send.
    pub fn composer_account_fence(&self) -> Option<(RequestId, &koushi_key::SessionKeyId)> {
        match self {
            Self::SubmitText {
                request_id,
                expected_account,
                ..
            }
            | Self::SubmitReply {
                request_id,
                expected_account,
                ..
            }
            | Self::UploadAndSendMedia {
                request_id,
                expected_account,
                ..
            } => Some((*request_id, expected_account)),
            _ => None,
        }
    }
}

// Message bodies and reaction keys are visible UI state but must not reach
// logs through Debug (spec: "SendText and EditText redact body in Debug and
// errors").
impl fmt::Debug for TimelineCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Subscribe { request_id, key } => formatter
                .debug_struct("Subscribe")
                .field("request_id", request_id)
                .field("key", key)
                .finish(),
            Self::EnsureSubscribed {
                request_id,
                key,
                replay_existing,
            } => formatter
                .debug_struct("EnsureSubscribed")
                .field("request_id", request_id)
                .field("key", key)
                .field("replay_existing", replay_existing)
                .finish(),
            Self::ReplaySubscribed { request_id } => formatter
                .debug_struct("ReplaySubscribed")
                .field("request_id", request_id)
                .finish(),
            Self::Unsubscribe { request_id, key } => formatter
                .debug_struct("Unsubscribe")
                .field("request_id", request_id)
                .field("key", key)
                .finish(),
            Self::Paginate {
                request_id,
                key,
                direction,
                event_count,
            } => formatter
                .debug_struct("Paginate")
                .field("request_id", request_id)
                .field("key", key)
                .field("direction", direction)
                .field("event_count", event_count)
                .finish(),
            Self::CancelPagination { request_id, key } => formatter
                .debug_struct("CancelPagination")
                .field("request_id", request_id)
                .field("key", key)
                .finish(),
            Self::CancelLinkPreviews { request_id, key } => formatter
                .debug_struct("CancelLinkPreviews")
                .field("request_id", request_id)
                .field("key", key)
                .finish(),
            Self::RestoreTimelineAnchor {
                request_id,
                max_batches,
                event_count,
                ..
            } => formatter
                .debug_struct("RestoreTimelineAnchor")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .field("max_batches", max_batches)
                .field("event_count", event_count)
                .finish(),
            Self::ObserveViewport { request_id, .. } => formatter
                .debug_struct("ObserveViewport")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("first_visible_event_id", &"EventId(..)")
                .field("last_visible_event_id", &"EventId(..)")
                .field("at_bottom", &"ViewportFact(..)")
                .finish(),
            Self::RepairGaps { request_id, .. } => formatter
                .debug_struct("RepairGaps")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .finish(),
            Self::SendText {
                request_id,
                key,
                transaction_id,
                ..
            } => formatter
                .debug_struct("SendText")
                .field("request_id", request_id)
                .field("key", key)
                .field("transaction_id", transaction_id)
                .field("body", &"MessageBody(..)")
                .field("mentions", &"MentionIntent(..)")
                .finish(),
            Self::SendReply {
                request_id,
                key,
                transaction_id,
                ..
            } => formatter
                .debug_struct("SendReply")
                .field("request_id", request_id)
                .field("key", key)
                .field("transaction_id", transaction_id)
                .field("in_reply_to_event_id", &"EventId(..)")
                .field("body", &"MessageBody(..)")
                .field("mentions", &"MentionIntent(..)")
                .finish(),
            Self::SubmitText {
                request_id,
                submission_id,
                transaction_id,
                ..
            } => formatter
                .debug_struct("SubmitText")
                .field("request_id", request_id)
                .field("submission_id", submission_id)
                .field("key", &"TimelineKey(..)")
                .field("transaction_id", transaction_id)
                .field("document", &"ComposerDocument(..)")
                .finish(),
            Self::SubmitReply {
                request_id,
                submission_id,
                transaction_id,
                ..
            } => formatter
                .debug_struct("SubmitReply")
                .field("request_id", request_id)
                .field("submission_id", submission_id)
                .field("key", &"TimelineKey(..)")
                .field("transaction_id", transaction_id)
                .field("in_reply_to_event_id", &"EventId(..)")
                .field("document", &"ComposerDocument(..)")
                .finish(),
            Self::ForwardMessage { request_id, .. } => formatter
                .debug_struct("ForwardMessage")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("source_event_id", &"EventId(..)")
                .field("destination_room_id", &"RoomId(..)")
                .field("transaction_id", &"TransactionId(..)")
                .finish(),
            Self::LoadMessageSource { request_id, .. } => formatter
                .debug_struct("LoadMessageSource")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::RequestRoomKey { request_id, .. } => formatter
                .debug_struct("RequestRoomKey")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::RequestLateDecryption { request_id, .. } => formatter
                .debug_struct("RequestLateDecryption")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .finish(),
            Self::RetrySend { request_id, .. } => formatter
                .debug_struct("RetrySend")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("transaction_id", &"TransactionId(..)")
                .finish(),
            Self::CancelSend { request_id, .. } => formatter
                .debug_struct("CancelSend")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("transaction_id", &"TransactionId(..)")
                .finish(),
            Self::UploadAndSendMedia {
                request_id,
                key,
                transaction_id,
                request,
                ..
            } => formatter
                .debug_struct("UploadAndSendMedia")
                .field("request_id", request_id)
                .field("key", key)
                .field("transaction_id", transaction_id)
                .field("mime_type", &request.mime_type)
                .field("kind", &request.kind)
                .field("filename", &"MediaFilename(..)")
                .field("bytes", &"MediaBytes(..)")
                .field("compression", &request.compression)
                .field("thumbnail", &request.thumbnail)
                .field(
                    "caption",
                    &request.caption.as_ref().map(|_| "MediaCaption(..)"),
                )
                .finish(),
            Self::DownloadMedia {
                request_id,
                key,
                selection,
                ..
            } => formatter
                .debug_struct("DownloadMedia")
                .field("request_id", request_id)
                .field("key", key)
                .field("event_id", &"EventId(..)")
                .field("selection", selection)
                .finish(),
            Self::EditText {
                request_id,
                key,
                event_id,
                ..
            } => formatter
                .debug_struct("EditText")
                .field("request_id", request_id)
                .field("key", key)
                .field("event_id", event_id)
                .field("document", &"ComposerDocument(..)")
                .finish(),
            Self::Redact {
                request_id, key, ..
            } => formatter
                .debug_struct("Redact")
                .field("request_id", request_id)
                .field("key", key)
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::ToggleReaction {
                request_id, key, ..
            } => formatter
                .debug_struct("ToggleReaction")
                .field("request_id", request_id)
                .field("key", key)
                .field("event_id", &"EventId(..)")
                .field("reaction_key", &"ReactionKey(..)")
                .finish(),
            Self::SendReaction {
                request_id, key, ..
            } => formatter
                .debug_struct("SendReaction")
                .field("request_id", request_id)
                .field("key", key)
                .field("event_id", &"EventId(..)")
                .field("reaction_key", &"ReactionKey(..)")
                .finish(),
            Self::RedactReaction {
                request_id, key, ..
            } => formatter
                .debug_struct("RedactReaction")
                .field("request_id", request_id)
                .field("key", key)
                .field("event_id", &"EventId(..)")
                .field("reaction_key", &"ReactionKey(..)")
                .field("reaction_event_id", &"EventId(..)")
                .finish(),
            Self::SendReadReceipt { request_id, .. } => formatter
                .debug_struct("SendReadReceipt")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::SetFullyRead { request_id, .. } => formatter
                .debug_struct("SetFullyRead")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::SetTyping {
                request_id,
                is_typing,
                ..
            } => formatter
                .debug_struct("SetTyping")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("is_typing", is_typing)
                .finish(),
            Self::LoadLinkPreviews { request_id, .. } => formatter
                .debug_struct("LoadLinkPreviews")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::HideLinkPreview { request_id, .. } => formatter
                .debug_struct("HideLinkPreview")
                .field("request_id", request_id)
                .field("key", &"TimelineKey(..)")
                .field("event_id", &"EventId(..)")
                .finish(),
            Self::BroadcastLinkPreviewPolicy {
                unencrypted_global_enabled,
                encrypted_global_enabled,
                room_overrides,
            } => formatter
                .debug_struct("BroadcastLinkPreviewPolicy")
                .field("unencrypted_global_enabled", unencrypted_global_enabled)
                .field("encrypted_global_enabled", encrypted_global_enabled)
                .field("room_override_count", &room_overrides.len())
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::fake_rid;
    use super::*;
    use koushi_state::{ImageUploadCompressionMode, MentionIntent, MentionTarget};

    fn test_session_key() -> koushi_key::SessionKeyId {
        koushi_key::SessionKeyId {
            homeserver: "https://example.test".to_owned(),
            user_id: "@a:test".to_owned(),
            device_id: "DEVICE".to_owned(),
        }
    }

    #[test]
    fn send_text_debug_redacts_body_and_mentions() {
        let command = TimelineCommand::SendText {
            request_id: fake_rid(6),
            key: TimelineKey::room(AccountKey("@a:test".to_owned()), "!room:test"),
            transaction_id: "txn-text".to_owned(),
            document: ComposerDocument::new(vec![
                koushi_state::ComposerInline::Text {
                    text: "secret text body ".to_owned(),
                },
                koushi_state::ComposerInline::Mention {
                    target: MentionTarget::User {
                        user_id: "@alice:example.test".to_owned(),
                        display_label: "Alice".to_owned(),
                    },
                    display_label: "Alice".to_owned(),
                },
            ]),
        };

        let debug = format!("{command:?}");
        assert!(debug.contains("SendText"), "{debug}");
        assert!(debug.contains("txn-text"), "{debug}");
        assert!(!debug.contains("secret text body"), "{debug}");
        assert!(!debug.contains("@alice:example.test"), "{debug}");
        assert!(!debug.contains("Alice"), "{debug}");
    }

    #[test]
    fn send_reply_debug_redacts_body_and_event_ids() {
        let command = TimelineCommand::SendReply {
            request_id: fake_rid(7),
            key: TimelineKey::room(AccountKey("@a:test".to_owned()), "!room:test"),
            transaction_id: "txn-reply".to_owned(),
            in_reply_to_event_id: "$event:test".to_owned(),
            document: koushi_state::ComposerDocument::from_plain_text(
                "secret reply body".to_owned(),
            ),
        };

        let debug = format!("{command:?}");
        assert!(debug.contains("SendReply"), "{debug}");
        assert!(debug.contains("txn-reply"), "{debug}");
        assert!(!debug.contains("secret reply body"), "{debug}");
        assert!(!debug.contains("$event:test"), "{debug}");
    }

    #[test]
    fn forward_message_debug_redacts_source_destination_and_transaction() {
        let request_id = fake_rid(71);
        let command = TimelineCommand::ForwardMessage {
            request_id,
            key: TimelineKey::room(AccountKey("@a:test".to_owned()), "!source-room:test"),
            source_event_id: "$source-event:test".to_owned(),
            destination_room_id: "!destination-room:test".to_owned(),
            transaction_id: "txn-forward-private".to_owned(),
        };

        assert_eq!(CoreCommand::Timeline(command).request_id(), request_id);

        let command = TimelineCommand::ForwardMessage {
            request_id,
            key: TimelineKey::room(AccountKey("@a:test".to_owned()), "!source-room:test"),
            source_event_id: "$source-event:test".to_owned(),
            destination_room_id: "!destination-room:test".to_owned(),
            transaction_id: "txn-forward-private".to_owned(),
        };
        let debug = format!("{command:?}");
        assert!(debug.contains("ForwardMessage"), "{debug}");
        assert!(debug.contains("TimelineKey(..)"), "{debug}");
        assert!(debug.contains("EventId(..)"), "{debug}");
        assert!(debug.contains("RoomId(..)"), "{debug}");
        assert!(debug.contains("TransactionId(..)"), "{debug}");
        assert!(!debug.contains("@a:test"), "{debug}");
        assert!(!debug.contains("!source-room:test"), "{debug}");
        assert!(!debug.contains("$source-event:test"), "{debug}");
        assert!(!debug.contains("!destination-room:test"), "{debug}");
        assert!(!debug.contains("txn-forward-private"), "{debug}");
    }

    #[test]
    fn load_message_source_debug_redacts_timeline_key_and_event_id() {
        let request_id = fake_rid(72);
        let command = TimelineCommand::LoadMessageSource {
            request_id,
            key: TimelineKey::room(AccountKey("@a:test".to_owned()), "!source-room:test"),
            event_id: "$source-event:test".to_owned(),
        };

        assert_eq!(CoreCommand::Timeline(command).request_id(), request_id);

        let command = TimelineCommand::LoadMessageSource {
            request_id,
            key: TimelineKey::room(AccountKey("@a:test".to_owned()), "!source-room:test"),
            event_id: "$source-event:test".to_owned(),
        };
        let debug = format!("{command:?}");
        assert!(debug.contains("LoadMessageSource"), "{debug}");
        assert!(debug.contains("TimelineKey(..)"), "{debug}");
        assert!(debug.contains("EventId(..)"), "{debug}");
        assert!(!debug.contains("@a:test"), "{debug}");
        assert!(!debug.contains("!source-room:test"), "{debug}");
        assert!(!debug.contains("$source-event:test"), "{debug}");
    }

    #[test]
    fn upload_media_debug_redacts_filename_caption_and_bytes() {
        let dimensions = ImageUploadDimensions {
            width: 1200,
            height: 900,
        };
        let compression = ImageUploadCompressionState {
            mode: ImageUploadCompressionMode::Always,
            policy: ImageUploadCompressionPolicy::default(),
            original: ImageUploadVariantInfo {
                mime_type: "image/jpeg".to_owned(),
                byte_count: 3_200_000,
                dimensions: Some(ImageUploadDimensions {
                    width: 4032,
                    height: 3024,
                }),
            },
            selected: ImageUploadVariantInfo {
                mime_type: "image/jpeg".to_owned(),
                byte_count: 128_000,
                dimensions: Some(dimensions),
            },
            selected_variant: ImageUploadVariantKind::Compressed,
            skipped_small_image: false,
            metadata_stripped: true,
            thumbnail_refreshed: true,
        };
        let command = TimelineCommand::UploadAndSendMedia {
            request_id: fake_rid(8),
            expected_account: test_session_key(),
            key: TimelineKey::room(AccountKey("@a:test".to_owned()), "!room:test"),
            transaction_id: "txn-media".to_owned(),
            request: UploadMediaRequest {
                filename: "private-fixture-name.png".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![1, 2, 3, 4],
                kind: UploadMediaKind::Image {
                    width: Some(2),
                    height: Some(2),
                },
                compression: Some(compression),
                thumbnail: Some(UploadMediaThumbnail {
                    mime_type: "image/jpeg".to_owned(),
                    bytes: vec![9, 8, 7, 6],
                    width: 320,
                    height: 240,
                }),
                caption: Some(koushi_state::build_formatted_message_draft(
                    "private caption",
                    MentionIntent::default(),
                )),
            },
        };

        let debug = format!("{command:?}");
        assert!(debug.contains("UploadAndSendMedia"), "{debug}");
        assert!(debug.contains("txn-media"), "{debug}");
        assert!(debug.contains("image/png"), "{debug}");
        assert!(debug.contains("Compressed"), "{debug}");
        assert!(debug.contains("thumbnail"), "{debug}");
        assert!(!debug.contains("private-fixture-name.png"), "{debug}");
        assert!(!debug.contains("private caption"), "{debug}");
        assert!(!debug.contains("1, 2, 3, 4"), "{debug}");
        assert!(!debug.contains("9, 8, 7, 6"), "{debug}");
    }

    #[test]
    fn image_upload_compression_policy_preserves_aspect_ratio_and_skips_small_images() {
        let policy = ImageUploadCompressionPolicy::default();

        assert_eq!(policy.threshold_bytes, 1_048_576);
        assert_eq!(policy.threshold_long_edge, 2560);
        assert_eq!(policy.target_long_edge, 2048);
        assert_eq!(policy.quality_percent, 82);
        assert_eq!(
            policy.target_dimensions_for(ImageUploadDimensions {
                width: 4032,
                height: 3024
            }),
            ImageUploadDimensions {
                width: 2048,
                height: 1536
            }
        );
        assert_eq!(
            policy.target_dimensions_for(ImageUploadDimensions {
                width: 1024,
                height: 768
            }),
            ImageUploadDimensions {
                width: 1024,
                height: 768
            }
        );

        let small = ImageUploadVariantInfo {
            mime_type: "image/png".to_owned(),
            byte_count: 64_000,
            dimensions: Some(ImageUploadDimensions {
                width: 800,
                height: 600,
            }),
        };
        let large_by_size = ImageUploadVariantInfo {
            mime_type: "image/png".to_owned(),
            byte_count: 2_000_000,
            dimensions: Some(ImageUploadDimensions {
                width: 800,
                height: 600,
            }),
        };
        let large_by_dimension = ImageUploadVariantInfo {
            mime_type: "image/png".to_owned(),
            byte_count: 64_000,
            dimensions: Some(ImageUploadDimensions {
                width: 4096,
                height: 512,
            }),
        };

        assert!(policy.should_skip(&small));
        assert!(!policy.should_skip(&large_by_size));
        assert!(!policy.should_skip(&large_by_dimension));
    }

    #[test]
    fn retry_and_cancel_send_debug_redacts_timeline_key_and_transaction_id() {
        let key = TimelineKey::room(AccountKey("@a:test".to_owned()), "!room:test");
        let retry = TimelineCommand::RetrySend {
            request_id: fake_rid(9),
            key: key.clone(),
            transaction_id: "txn-private".to_owned(),
        };
        let cancel = TimelineCommand::CancelSend {
            request_id: fake_rid(10),
            key,
            transaction_id: "txn-private".to_owned(),
        };

        for debug in [format!("{retry:?}"), format!("{cancel:?}")] {
            assert!(!debug.contains("!room:test"), "{debug}");
            assert!(!debug.contains("@a:test"), "{debug}");
            assert!(!debug.contains("txn-private"), "{debug}");
            assert!(debug.contains("TransactionId(..)"), "{debug}");
        }
    }
}
