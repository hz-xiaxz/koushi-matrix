//! Attachment retrieval for out-of-band callers (#1524).
//!
//! The GUI downloads attachments through a resident `TimelineActor`, which
//! knows the media sources of items it has projected. An MCP caller must not
//! depend on that presentation lifetime: an unviewed room has no actor at all.
//! This module resolves the room, the event and its media source from the
//! account session directly, then reuses the GUI's download and cache path.

use std::path::PathBuf;
use std::sync::Arc;

use koushi_protocol::command::MediaDownloadSelection;
use koushi_sdk::MatrixClientSession;

use super::item_projection::private_media_entry_from_msgtype;
use super::media::{PrivateMediaEntry, download_media_to_cache};

/// Bound on fetching one event through the SDK before downloading its media.
const ATTACHMENT_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// One attachment written into the account's managed media cache.
#[derive(Debug)]
pub(crate) struct CachedAttachment {
    pub(crate) path: PathBuf,
    pub(crate) mime_type: Option<String>,
    pub(crate) size_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttachmentFailure {
    /// The room is unknown to this account or not joined.
    RoomNotFound,
    /// The event is unknown to the room.
    EventNotFound,
    /// The event carries no downloadable media.
    Unsupported,
    /// The download itself failed (network, key, or write).
    DownloadFailed,
}

/// Fetch one event's attachment into the account's managed cache.
pub(crate) async fn fetch_attachment(
    session: Arc<MatrixClientSession>,
    data_dir: PathBuf,
    room_id: &str,
    event_id: &str,
    selection: MediaDownloadSelection,
) -> Result<CachedAttachment, AttachmentFailure> {
    let parsed_room =
        matrix_sdk::ruma::RoomId::parse(room_id).map_err(|_| AttachmentFailure::RoomNotFound)?;
    let room = session
        .client()
        .get_room(&parsed_room)
        .ok_or(AttachmentFailure::RoomNotFound)?;
    if room.state() != matrix_sdk::RoomState::Joined {
        return Err(AttachmentFailure::RoomNotFound);
    }
    let parsed_event =
        matrix_sdk::ruma::EventId::parse(event_id).map_err(|_| AttachmentFailure::EventNotFound)?;
    let event = crate::executor::timeout(ATTACHMENT_EVENT_TIMEOUT, room.event(&parsed_event, None))
        .await
        .map_err(|_| AttachmentFailure::EventNotFound)?
        .map_err(|_| AttachmentFailure::EventNotFound)?;
    let entry = entry_for_event(&event).ok_or(AttachmentFailure::Unsupported)?;
    let (path, size_bytes, mime_type) = download_media_to_cache(
        session,
        data_dir,
        room_id.to_owned(),
        event_id.to_owned(),
        entry,
        &selection,
    )
    .await
    .map_err(|_| AttachmentFailure::DownloadFailed)?;
    Ok(CachedAttachment {
        path,
        mime_type,
        size_bytes,
    })
}

/// Derive the media source of one event, encrypted or not.
///
/// The session's `Room::event` already decrypts, so a plaintext message-like
/// event is what reaches this point; an event that could not be decrypted or
/// that carries no media fails closed as unsupported.
fn entry_for_event(
    event: &matrix_sdk::deserialized_responses::TimelineEvent,
) -> Option<PrivateMediaEntry> {
    use matrix_sdk::ruma::events::{
        AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
    };
    let parsed: AnySyncTimelineEvent = event.raw().deserialize().ok()?;
    let AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(message)) = parsed
    else {
        return None;
    };
    let SyncMessageLikeEvent::Original(original) = message else {
        return None;
    };
    private_media_entry_from_msgtype(&original.content.msgtype)
}

#[cfg(test)]
mod tests {
    use super::*;
    use koushi_state::SessionInfo;

    async fn synthetic_session() -> (Arc<MatrixClientSession>, tempfile::TempDir) {
        let server = matrix_sdk::test_utils::mocks::MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let session = MatrixClientSession::from_client_for_testing(
            client.clone(),
            SessionInfo {
                homeserver: server.server().uri(),
                user_id: client.user_id().expect("user id").to_string(),
                device_id: client.device_id().expect("device id").to_string(),
                authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
            },
        );
        (Arc::new(session), tempfile::tempdir().expect("data dir"))
    }

    /// #1524: retrieval must fail closed for a room the account has not
    /// joined, even though no timeline actor is involved at all.
    #[tokio::test]
    async fn an_unjoined_room_fails_closed() {
        let (session, data_dir) = synthetic_session().await;
        let failure = fetch_attachment(
            session,
            data_dir.path().to_path_buf(),
            "!not-joined:example.invalid",
            "$event:example.invalid",
            MediaDownloadSelection::File,
        )
        .await
        .expect_err("an unjoined room must fail");
        assert_eq!(failure, AttachmentFailure::RoomNotFound);
    }

    /// #1524: a failed fetch leaves the managed cache untouched.
    #[tokio::test]
    async fn a_failed_fetch_writes_nothing_into_the_cache() {
        let (session, data_dir) = synthetic_session().await;
        let failure = fetch_attachment(
            session,
            data_dir.path().to_path_buf(),
            "!not-joined:example.invalid",
            "$event:example.invalid",
            MediaDownloadSelection::File,
        )
        .await
        .expect_err("an unjoined room must fail");
        assert_eq!(failure, AttachmentFailure::RoomNotFound);
        assert!(
            !data_dir.path().join("media-downloads").exists(),
            "a failed fetch must not create cache entries"
        );
    }
}
