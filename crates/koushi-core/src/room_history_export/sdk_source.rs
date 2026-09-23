//! Matrix SDK adapter for the export driver: forward `/messages` pages and
//! Element's header metadata.

use matrix_sdk::deserialized_responses::{TimelineEventKind, UnableToDecryptReason};
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::UInt;
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

impl HistoryPageSource for MatrixRoomHistorySource {
    async fn next_page(&mut self, from: Option<String>) -> Result<HistoryPage, HistoryPageError> {
        loop {
            let permit = self
                .account_work
                .acquire(AccountWorkKind::SearchCrawl)
                .await;
            let mut options = MessagesOptions::forward().from(from.as_deref());
            options.limit = UInt::from(PAGE_LIMIT);
            let result = tokio::select! {
                biased;
                // Visible timeline work preempts the export; retry this page
                // once the account gate readmits background work.
                _ = permit.cancelled() => continue,
                result = self.room.messages(options) => result,
            };
            drop(permit);
            let messages = result.map_err(|error| match error {
                matrix_sdk::Error::Http(_) => HistoryPageError::Network,
                _ => HistoryPageError::Sdk,
            })?;
            return Ok(HistoryPage {
                events: messages
                    .chunk
                    .iter()
                    .filter_map(|event| classify(&event.kind))
                    .collect(),
                end: messages.end,
            });
        }
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
