use koushi_state::{
    ComposerDocument, ComposerFormattingOptions, ComposerSendIntent, FormattedMessageDraft,
    MentionIntent, SlashCommandIntent, resolve_composer_send_intent,
    resolve_composer_send_intent_with_options,
};

use matrix_sdk::ruma::UserId;
use matrix_sdk::ruma::events::Mentions;
use matrix_sdk::ruma::events::room::message::{
    RoomMessageEventContent, RoomMessageEventContentWithoutRelation, TextMessageEventContent,
};

use crate::failure::TimelineFailureKind;

pub(crate) fn validate_composer_body_for_timeline_send(
    body: &str,
) -> Result<(), TimelineFailureKind> {
    match resolve_composer_send_intent(body, MentionIntent::default()) {
        ComposerSendIntent::LocalFailure { .. }
        | ComposerSendIntent::SlashCommand {
            command:
                SlashCommandIntent::Join { .. }
                | SlashCommandIntent::Invite { .. }
                | SlashCommandIntent::PlainText { .. }
                | SlashCommandIntent::Unsupported { .. },
        } => Err(TimelineFailureKind::UnsupportedSlashCommand),
        ComposerSendIntent::Message { .. }
        | ComposerSendIntent::SlashCommand {
            command: SlashCommandIntent::Me { .. },
        } => Ok(()),
    }
}

#[cfg(test)]
pub(crate) fn build_room_message_content_from_composer_document(
    document: ComposerDocument,
) -> Result<RoomMessageEventContent, TimelineFailureKind> {
    build_room_message_content_from_composer_document_with_options(
        document,
        ComposerFormattingOptions::default(),
    )
}

pub(super) fn build_room_message_content_from_composer_document_with_options(
    document: ComposerDocument,
    formatting_options: ComposerFormattingOptions,
) -> Result<RoomMessageEventContent, TimelineFailureKind> {
    build_room_message_content_without_relation_from_composer_document_with_options(
        document,
        formatting_options,
    )
    .map(|content| content.with_relation(None))
}

pub(super) fn build_room_message_content_without_relation_from_composer_document_with_options(
    document: ComposerDocument,
    formatting_options: ComposerFormattingOptions,
) -> Result<RoomMessageEventContentWithoutRelation, TimelineFailureKind> {
    let body = document.plain_body();
    if body.starts_with('/') {
        return build_room_message_content_without_relation_from_composer_body_with_options(
            &body,
            document.mention_intent(),
            formatting_options,
        );
    }
    let mut content = match document.formatted_body_with_options(formatting_options) {
        Some(formatted_body) => {
            RoomMessageEventContentWithoutRelation::text_html(body, formatted_body)
        }
        None => RoomMessageEventContentWithoutRelation::text_plain(body),
    };
    if let Some(mentions) = ruma_mentions_from_intent(&document.mention_intent()) {
        content = content.add_mentions(mentions);
    }
    Ok(content)
}

pub(crate) fn build_room_message_content_from_composer_body(
    body: &str,
    mentions: MentionIntent,
) -> Result<RoomMessageEventContent, TimelineFailureKind> {
    build_room_message_content_from_composer_body_with_options(
        body,
        mentions,
        ComposerFormattingOptions::default(),
    )
}

pub(crate) fn build_room_message_content_from_composer_body_with_options(
    body: &str,
    mentions: MentionIntent,
    formatting_options: ComposerFormattingOptions,
) -> Result<RoomMessageEventContent, TimelineFailureKind> {
    build_room_message_content_without_relation_from_composer_body_with_options(
        body,
        mentions,
        formatting_options,
    )
    .map(|content| content.with_relation(None))
}

fn build_room_message_content_without_relation_from_composer_body_with_options(
    body: &str,
    mentions: MentionIntent,
    formatting_options: ComposerFormattingOptions,
) -> Result<RoomMessageEventContentWithoutRelation, TimelineFailureKind> {
    match resolve_composer_send_intent_with_options(body, mentions, formatting_options) {
        ComposerSendIntent::Message { draft } => {
            Ok(without_relation_content_from_formatted_draft(draft, false))
        }
        ComposerSendIntent::SlashCommand {
            command: SlashCommandIntent::Me { body },
        } => Ok(without_relation_content_from_formatted_draft(
            koushi_state::build_formatted_message_draft_with_options(
                body,
                MentionIntent::default(),
                formatting_options,
            ),
            true,
        )),
        ComposerSendIntent::SlashCommand { .. } | ComposerSendIntent::LocalFailure { .. } => {
            Err(TimelineFailureKind::UnsupportedSlashCommand)
        }
    }
}

fn without_relation_content_from_formatted_draft(
    draft: FormattedMessageDraft,
    emote: bool,
) -> RoomMessageEventContentWithoutRelation {
    let mut content = match (emote, draft.formatted_body) {
        (true, Some(formatted_body)) => {
            RoomMessageEventContentWithoutRelation::emote_html(draft.plain_body, formatted_body)
        }
        (true, None) => RoomMessageEventContentWithoutRelation::emote_plain(draft.plain_body),
        (false, Some(formatted_body)) => {
            RoomMessageEventContentWithoutRelation::text_html(draft.plain_body, formatted_body)
        }
        (false, None) => RoomMessageEventContentWithoutRelation::text_plain(draft.plain_body),
    };

    if let Some(mentions) = ruma_mentions_from_intent(&draft.mentions) {
        content = content.add_mentions(mentions);
    }
    content
}

pub(super) fn media_caption_content_from_draft(
    draft: &FormattedMessageDraft,
) -> TextMessageEventContent {
    match &draft.formatted_body {
        Some(formatted_body) => {
            TextMessageEventContent::html(draft.plain_body.clone(), formatted_body.clone())
        }
        None => TextMessageEventContent::plain(draft.plain_body.clone()),
    }
}

pub(super) fn ruma_mentions_from_intent(intent: &MentionIntent) -> Option<Mentions> {
    let user_ids = intent
        .user_ids()
        .into_iter()
        .filter_map(|user_id| UserId::parse(user_id).ok().map(Into::into))
        .collect::<Vec<_>>();
    let mentions_room = intent.mentions_room();

    if user_ids.is_empty() && !mentions_room {
        return None;
    }

    let mut mentions = if user_ids.is_empty() {
        Mentions::new()
    } else {
        Mentions::with_user_ids(user_ids)
    };
    mentions.room = mentions_room;
    Some(mentions)
}

#[cfg(test)]
mod tests;
