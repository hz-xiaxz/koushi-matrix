use koushi_state::{public_room_address_required, suggest_room_alias_localpart};

#[test]
fn room_name_suggestions_are_editable_local_parts_not_full_aliases() {
    assert_eq!(suggest_room_alias_localpart("Example Room"), "example-room");
    assert_eq!(
        suggest_room_alias_localpart("  Example   Room  "),
        "example-room"
    );
    assert_eq!(suggest_room_alias_localpart("設計の相談"), "設計の相談");
    assert_eq!(
        suggest_room_alias_localpart("Example / 設計"),
        "example-設計"
    );
    assert_eq!(suggest_room_alias_localpart(""), "");
    assert_eq!(suggest_room_alias_localpart(" # : / 😀 "), "");
}

/// #1023 exempts only unnamed rooms from the #1006 address requirement; a
/// named room whose name offers no suggestion still needs a manual address.
#[test]
fn only_unnamed_public_rooms_may_skip_the_address() {
    for unnamed in ["", "   ", "\t\n"] {
        assert!(!public_room_address_required(unnamed), "{unnamed:?}");
    }
    for named in ["papers", "設計の相談", "🎉", "!!!", " # : / 😀 "] {
        assert!(public_room_address_required(named), "{named:?}");
    }
}
