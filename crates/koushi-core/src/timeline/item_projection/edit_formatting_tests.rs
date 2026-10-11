//! Guards that an edit formats its replacement body with the composer
//! formatting options the command carried, instead of the defaults.

use koushi_state::{ComposerDocument, ComposerFormattingOptions};
use matrix_sdk::room::edit::EditedContent;
use matrix_sdk::ruma::events::room::message::{MessageType, TextMessageEventContent};

use super::edited_document_content_for_edit_target;

#[test]
fn edit_replacement_never_forces_math_formatting() {
    // Regression guard: the edit path used the default composer formatting
    // options, so a user with math mode OFF still had `$...$` in an edited body
    // converted to `data-mx-maths` markup. The edit must format with the same
    // options the send path uses, which is what the settings snapshot carries.
    let document = ComposerDocument::from_plain_text("Energy $E=mc^2$");
    let text = MessageType::Text(TextMessageEventContent::plain("old"));
    let EditedContent::RoomMessage(content) = edited_document_content_for_edit_target(
        Some(&text),
        &document,
        ComposerFormattingOptions { math_mode: false },
    ) else {
        panic!("text edit must stay a room message")
    };
    let MessageType::Text(text) = content.msgtype else {
        panic!("text edit must stay text")
    };
    assert_eq!(text.body, "Energy $E=mc^2$");
    assert!(
        text.formatted.is_none(),
        "an edit must not convert math delimiters unless math mode is on"
    );
}
