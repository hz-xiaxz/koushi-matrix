//! Guards that the edit path forwards the composer formatting options.
//!
//! The edit actor re-formats the replacement body, so it must receive the
//! options the command was handled with instead of the defaults. This module
//! lives outside `manager`'s inline test module, which is close to the inline
//! test-size limit.

use std::collections::HashMap;

use koushi_protocol::ids::TimelineKey;
use koushi_state::{ComposerDocument, ComposerFormattingOptions};
use tokio::sync::mpsc;

use super::super::test_support::{
    fake_rid, live_tail_test_manager, room_key, test_timeline_actor_handle,
};
use super::{TimelineActorMessage, TimelineCommand};

#[tokio::test]
async fn edit_text_forwards_the_command_composer_formatting_options() {
    // Guards the manager wiring that once dropped math mode for edits.
    let key: TimelineKey = room_key();
    let mut handle = test_timeline_actor_handle();
    let (tx, mut rx) = mpsc::channel(4);
    handle.tx = tx;
    let mut manager = live_tail_test_manager(HashMap::from([(key.clone(), handle)]));

    manager
        .handle_command_with_formatting_options(
            TimelineCommand::EditText {
                request_id: fake_rid(9_100),
                key,
                event_id: "$edit:example.invalid".to_owned(),
                document: ComposerDocument::from_plain_text("Energy $E=mc^2$"),
            },
            ComposerFormattingOptions { math_mode: false },
        )
        .await;

    let message = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("the edit must reach the timeline actor")
        .expect("the timeline actor channel stays open");
    assert!(
        matches!(
            message,
            TimelineActorMessage::EditText {
                formatting_options: ComposerFormattingOptions { math_mode: false },
                ..
            }
        ),
        "the edit must carry the formatting options the command was handled with"
    );
}
