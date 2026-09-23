//! Matrix SDK adapter for the export driver: the `timestamp_to_event` seek,
//! forward `/messages` pages, and Element's header metadata.

use std::future::IntoFuture;

use matrix_sdk::deserialized_responses::{TimelineEvent, TimelineEventKind, UnableToDecryptReason};
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::api::Direction;
use matrix_sdk::ruma::api::client::room::get_event_by_timestamp;
use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, UInt};
use serde_json::Value;

use crate::account_work::{AccountWorkKind, AccountWorkScheduler};

use super::driver::{HistoryPage, HistoryPageError, HistoryPageSource, PAGE_LIMIT};
use super::element::{
    ExportDateLocale, ExportHeader, ExportSourceEvent, UndecryptableReason, format_export_date,
};

pub(crate) struct MatrixRoomHistorySource {
    room: matrix_sdk::Room,
    account_work: AccountWorkScheduler,
}

impl MatrixRoomHistorySource {
    pub(crate) fn new(room: matrix_sdk::Room, account_work: AccountWorkScheduler) -> Self {
        Self { room, account_work }
    }
}

fn parse_raw(json: &str) -> Option<Value> {
    serde_json::from_str::<Value>(json)
        .ok()
        .filter(Value::is_object)
}

fn undecryptable_reason(reason: &UnableToDecryptReason) -> UndecryptableReason {
    match reason {
        UnableToDecryptReason::MissingMegolmSession { .. } => UndecryptableReason::MissingRoomKey,
        UnableToDecryptReason::UnknownMegolmMessageIndex => {
            UndecryptableReason::UnknownMessageIndex
        }
        _ => UndecryptableReason::Other,
    }
}

fn classify(kind: &TimelineEventKind) -> Option<ExportSourceEvent> {
    Some(match kind {
        TimelineEventKind::Decrypted(decrypted) => {
            ExportSourceEvent::Decrypted(parse_raw(decrypted.event.json().get())?)
        }
        TimelineEventKind::UnableToDecrypt { event, utd_info } => {
            ExportSourceEvent::Undecryptable {
                wire: parse_raw(event.json().get())?,
                reason: undecryptable_reason(&utd_info.reason),
            }
        }
        TimelineEventKind::PlainText { event } => {
            ExportSourceEvent::Plain(parse_raw(event.json().get())?)
        }
    })
}

/// Run one request as background account work. Visible timeline work
/// preempts the export; the request is retried once the account gate
/// readmits background work.
async fn background<F, R>(account_work: &AccountWorkScheduler, mut request: F) -> R::Output
where
    F: FnMut() -> R,
    R: IntoFuture,
{
    loop {
        let permit = account_work.acquire(AccountWorkKind::SearchCrawl).await;
        tokio::select! {
            biased;
            _ = permit.cancelled() => continue,
            result = request() => return result,
        }
    }
}

fn sdk_error(error: matrix_sdk::Error) -> HistoryPageError {
    match error {
        matrix_sdk::Error::Http(_) => HistoryPageError::Network,
        _ => HistoryPageError::Sdk,
    }
}

fn classify_all<'a>(events: impl Iterator<Item = &'a TimelineEvent>) -> Vec<ExportSourceEvent> {
    events.filter_map(|event| classify(&event.kind)).collect()
}

impl HistoryPageSource for MatrixRoomHistorySource {
    async fn seek(&mut self, at_ms: u64) -> Result<Option<HistoryPage>, HistoryPageError> {
        let Ok(ts) = UInt::try_from(at_ms) else {
            return Ok(None);
        };
        let request = get_event_by_timestamp::v1::Request::new(
            self.room.room_id().to_owned(),
            MilliSecondsSinceUnixEpoch(ts),
            Direction::Forward,
        );
        let client = self.room.client();
        // Any server answer (unsupported endpoint, no event, forbidden, or a
        // server error) falls back to reading from the first visible event;
        // only a transport failure fails the export.
        let found = match background(&self.account_work, || client.send(request.clone())).await {
            Ok(found) => found,
            Err(error) if error.as_client_api_error().is_some() => return Ok(None),
            Err(_) => return Err(HistoryPageError::Network),
        };
        let context = match background(&self.account_work, || {
            self.room
                .event_with_context(&found.event_id, true, UInt::from(0_u32), None)
        })
        .await
        {
            Ok(context) => context,
            Err(matrix_sdk::Error::Http(error)) if error.as_client_api_error().is_some() => {
                return Ok(None);
            }
            Err(error) => return Err(sdk_error(error)),
        };
        // The found event is emitted here and paging continues after it, so
        // it is never skipped. Without a forward token the history cannot be
        // continued safely.
        let Some(end) = context.next_batch_token else {
            return Ok(None);
        };
        Ok(Some(HistoryPage {
            events: classify_all(
                context
                    .events_before
                    .iter()
                    .rev()
                    .chain(context.event.iter())
                    .chain(context.events_after.iter()),
            ),
            end: Some(end),
        }))
    }

    async fn next_page(&mut self, from: Option<String>) -> Result<HistoryPage, HistoryPageError> {
        let messages = background(&self.account_work, || {
            let mut options = MessagesOptions::forward().from(from.as_deref());
            options.limit = UInt::from(PAGE_LIMIT);
            self.room.messages(options)
        })
        .await
        .map_err(sdk_error)?;
        Ok(HistoryPage {
            events: classify_all(messages.chunk.iter()),
            end: messages.end,
        })
    }
}

/// A member's room display name, falling back to the Matrix ID like
/// Element's `getMember(userId)?.rawDisplayName || userId`.
async fn member_label(room: &matrix_sdk::Room, user_id: &matrix_sdk::ruma::UserId) -> String {
    room.get_member_no_sync(user_id)
        .await
        .ok()
        .flatten()
        .and_then(|member| member.display_name().map(str::to_owned))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| user_id.to_string())
}

pub(crate) async fn room_export_header(
    room: &matrix_sdk::Room,
    now_ms: u64,
    utc_offset_minutes: i32,
    locale: koushi_state::CatalogLocale,
) -> ExportHeader {
    let room_name = match room.display_name().await {
        Ok(name) => name.to_string(),
        Err(_) => room
            .cached_display_name()
            .map(|name| name.to_string())
            .or_else(|| room.name())
            .unwrap_or_else(|| room.room_id().to_string()),
    };
    let room_creator = match room
        .creators()
        .and_then(|creators| creators.into_iter().next())
    {
        Some(creator) => Some(member_label(room, &creator).await),
        None => None,
    };
    ExportHeader {
        room_name,
        room_creator,
        topic: room.topic().unwrap_or_default(),
        export_date: format_export_date(now_ms, utc_offset_minutes, ExportDateLocale::from(locale)),
        exported_by: member_label(room, room.own_user_id()).await,
    }
}
