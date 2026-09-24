use serde_json::json;

use super::room_page::render_room_page;
use super::test_support::*;
use crate::room_history_export::records::{
    AttachmentIndex, AttachmentKind, AttachmentRecord, AttachmentStatus,
};

fn render(events: &[serde_json::Value]) -> String {
    render_with(events, &AttachmentIndex::default())
}

fn render_with(events: &[serde_json::Value], attachments: &AttachmentIndex) -> String {
    String::from_utf8(render_room_page(
        &jsonl(events),
        &meta(),
        attachments,
        &labels(),
    ))
    .unwrap()
}

#[test]
fn page_head_escapes_room_text_and_loads_math_from_files_only() {
    let page = render(&[]);
    assert!(page.starts_with("<!DOCTYPE html>"));
    assert!(page.contains("<html lang=\"en\">"));
    assert!(page.contains("<title>Lab &lt;Room&gt;</title>"));
    assert!(page.contains("Weekly &amp; more"));
    assert!(page.contains("Content-Security-Policy"));
    assert!(page.contains("default-src 'none'"));
    assert!(!page.contains("data:"), "img-src must not allow data:");
    assert!(page.contains("<script defer src=\"../../assets/katex/katex.min.js\"></script>"));
    assert!(page.contains("<script defer src=\"../../assets/koushi-math.js\"></script>"));
    assert!(page.contains("href=\"../../assets/katex/katex.min.css\""));
    assert_eq!(page.matches("<script").count(), 2, "no inline script");
    assert!(page.contains("Times are shown in Asia/Tokyo."));
}

#[test]
fn plain_body_is_escaped_with_line_breaks() {
    let page = render(&[text("$1", "@alice:example.org", 0, "a<b>\nc & d")]);
    assert!(page.contains("a&lt;b&gt;<br>c &amp; d"), "{page}");
    assert!(page.contains("Alice"));
}

#[test]
fn hostile_formatted_body_is_sanitized() {
    let page = render(&[html(
        "$1",
        0,
        "ok",
        "<script>x()</script><img src=\"mxc://h/m\" onerror=\"y()\"><a href=\"javascript:z()\">l</a><b>ok</b>",
    )]);
    assert!(page.contains("<b>ok</b>"), "{page}");
    for forbidden in ["x()", "onerror", "mxc://", "javascript:"] {
        assert!(!page.contains(forbidden), "{forbidden} leaked: {page}");
    }
}

#[test]
fn math_source_survives_sanitizing() {
    let page = render(&[html(
        "$1",
        0,
        "E=mc^2",
        "<span data-mx-maths=\"E=mc^2\">E=mc^2</span> and <div data-mx-maths=\"\\int_0^1 x\\,dx\"><code>x</code></div>",
    )]);
    assert!(page.contains("<span data-mx-maths=\"E=mc^2\">"), "{page}");
    assert!(
        page.contains("data-mx-maths=\"\\int_0^1 x\\,dx\""),
        "{page}"
    );
}

#[test]
fn reactions_are_counted_under_their_target() {
    let reaction = |id: &str, sender: &str, key: &str| {
        json!({
            "type": "m.reaction", "event_id": id, "sender": sender, "origin_server_ts": ts(1),
            "content": { "m.relates_to": { "rel_type": "m.annotation", "event_id": "$1", "key": key } }
        })
    };
    let page = render(&[
        text("$1", "@alice:example.org", 0, "hello"),
        reaction("$r1", "@alice:example.org", "👍"),
        reaction("$r2", "@bob:example.org", "👍"),
        reaction("$r3", "@bob:example.org", "🎉"),
    ]);
    assert!(page.contains("<li>👍 2</li><li>🎉 1</li>"), "{page}");
    assert!(page.contains("aria-label=\"Reactions\""));
    assert_eq!(
        page.matches("<article").count(),
        1,
        "reactions are not messages"
    );
}

#[test]
fn latest_edit_from_the_same_sender_is_shown_and_marked() {
    let edit = |id: &str, sender: &str, minutes: u64, body: &str| {
        message(
            id,
            sender,
            minutes,
            json!({
                "msgtype": "m.text", "body": format!("* {body}"),
                "m.new_content": { "msgtype": "m.text", "body": body },
                "m.relates_to": { "rel_type": "m.replace", "event_id": "$1" }
            }),
        )
    };
    let page = render(&[
        text("$1", "@alice:example.org", 0, "v1"),
        edit("$e2", "@alice:example.org", 2, "v3"),
        edit("$e1", "@alice:example.org", 1, "v2"),
        edit("$e3", "@bob:example.org", 3, "forged"),
    ]);
    assert!(page.contains("v3"), "{page}");
    assert!(
        !page.contains(">v1<") && !page.contains("v2") && !page.contains("forged"),
        "{page}"
    );
    assert!(page.contains("(edited)"));
    assert_eq!(page.matches("<article").count(), 1);
}

#[test]
fn reply_links_to_its_target_and_strips_the_fallback() {
    let reply = message(
        "$2",
        "@bob:example.org",
        1,
        json!({
            "msgtype": "m.text", "body": "> <@alice:example.org> first\n\nsecond",
            "m.relates_to": { "m.in_reply_to": { "event_id": "$1" } }
        }),
    );
    let orphan = message(
        "$3",
        "@bob:example.org",
        2,
        json!({
            "msgtype": "m.text", "body": "third",
            "m.relates_to": { "m.in_reply_to": { "event_id": "$gone" } }
        }),
    );
    let page = render(&[text("$1", "@alice:example.org", 0, "first"), reply, orphan]);
    let anchor = format!("#e-{}", crate::room_history_export::layout::fnv1a_hex("$1"));
    assert!(
        page.contains(&format!("<a href=\"{anchor}\">In reply to Alice</a>")),
        "{page}"
    );
    assert!(page.contains("second"));
    assert_eq!(
        page.matches("&lt;@alice:example.org&gt; first").count(),
        0,
        "{page}"
    );
    assert!(page.contains("In reply to a message outside this export"));
}

#[test]
fn thread_reply_links_to_root_without_a_fallback_reply() {
    let thread = message(
        "$2",
        "@bob:example.org",
        1,
        json!({
            "msgtype": "m.text", "body": "in thread",
            "m.relates_to": { "rel_type": "m.thread", "event_id": "$1", "is_falling_back": true,
                "m.in_reply_to": { "event_id": "$1" } }
        }),
    );
    let page = render(&[text("$1", "@alice:example.org", 0, "root"), thread]);
    assert!(page.contains("Thread reply"));
    assert!(page.contains("Thread start"));
    assert!(!page.contains("In reply to"), "{page}");
}

#[test]
fn redacted_and_undecryptable_placeholders() {
    let redacted = json!({
        "type": "m.room.message", "event_id": "$1", "sender": "@alice:example.org",
        "origin_server_ts": ts(0), "content": {},
        "unsigned": { "redacted_because": { "type": "m.room.redaction" } }
    });
    let utd = message(
        "$2",
        "@bob:example.org",
        1,
        json!({
            "msgtype": "m.bad.encrypted", "body": "** Unable to decrypt: DecryptionError **"
        }),
    );
    let page = render(&[redacted, utd]);
    assert!(page.contains("Message deleted"));
    assert!(page.contains("Unable to decrypt message"));
    assert!(!page.contains("DecryptionError"));
}

fn record(
    event_id: &str,
    kind: AttachmentKind,
    name: &str,
    file: Option<&str>,
    thumb: Option<&str>,
) -> AttachmentRecord {
    AttachmentRecord {
        event_id: event_id.to_owned(),
        kind,
        name: name.to_owned(),
        size: Some(1_536),
        mimetype: None,
        file: file.map(str::to_owned),
        thumb: thumb.map(str::to_owned),
        status: if file.is_some() {
            AttachmentStatus::Retrieved
        } else {
            AttachmentStatus::Failed
        },
    }
}

#[test]
fn attachments_link_files_and_thumbnails() {
    let image = message(
        "$1",
        "@alice:example.org",
        0,
        json!({ "msgtype": "m.image", "body": "cat #1.png", "url": "mxc://h/a" }),
    );
    let pdf = message(
        "$2",
        "@alice:example.org",
        1,
        json!({ "msgtype": "m.file", "body": "paper.pdf", "url": "mxc://h/b" }),
    );
    let lost = message(
        "$3",
        "@alice:example.org",
        2,
        json!({ "msgtype": "m.file", "body": "lost.zip", "url": "mxc://h/c" }),
    );
    let attachments = AttachmentIndex {
        attachments: vec![
            record(
                "$1",
                AttachmentKind::Image,
                "cat #1.png",
                Some("files/0001_cat #1.png"),
                Some("thumbs/0001.jpg"),
            ),
            record(
                "$2",
                AttachmentKind::File,
                "paper.pdf",
                Some("files/0002_paper.pdf"),
                None,
            ),
            record("$3", AttachmentKind::File, "lost.zip", None, None),
        ],
    };
    let page = render_with(&[image, pdf, lost], &attachments);
    assert!(page.contains("<a href=\"files/0001_cat%20%231.png\"><img src=\"thumbs/0001.jpg\" alt=\"cat #1.png\" loading=\"lazy\"></a>"), "{page}");
    assert!(
        page.contains("<a href=\"files/0002_paper.pdf\" download>paper.pdf</a>"),
        "{page}"
    );
    assert!(page.contains("1.5 KB"));
    assert!(page.contains("lost.zip — not retrieved"));
    assert!(!page.contains("mxc://"));
}

#[test]
fn state_events_become_one_line_descriptions() {
    let state = |id: &str,
                 kind: &str,
                 key: &str,
                 sender: &str,
                 content: serde_json::Value,
                 prev: serde_json::Value| {
        json!({ "type": kind, "event_id": id, "sender": sender, "state_key": key,
            "origin_server_ts": ts(0), "content": content, "unsigned": { "prev_content": prev } })
    };
    let page = render(&[
        state(
            "$1",
            "m.room.member",
            "@carol:example.org",
            "@carol:example.org",
            json!({ "membership": "join", "displayname": "Carol" }),
            json!({}),
        ),
        state(
            "$2",
            "m.room.member",
            "@carol:example.org",
            "@alice:example.org",
            json!({ "membership": "leave" }),
            json!({ "membership": "join", "displayname": "Carol" }),
        ),
        state(
            "$3",
            "m.room.name",
            "",
            "@alice:example.org",
            json!({ "name": "New <name>" }),
            json!({}),
        ),
        state(
            "$4",
            "m.room.power_levels",
            "",
            "@bob:example.org",
            json!({}),
            json!({}),
        ),
        state(
            "$5",
            "m.room.member",
            "@alice:example.org",
            "@alice:example.org",
            json!({ "membership": "join", "displayname": "Alicia" }),
            json!({ "membership": "join", "displayname": "Alice" }),
        ),
    ]);
    assert!(page.contains("Carol joined"), "{page}");
    assert!(page.contains("Alice removed Carol"), "{page}");
    assert!(page.contains("Alice changed the room name to New &lt;name&gt;"));
    assert!(page.contains("Bob changed m.room.power_levels"));
    assert!(
        !page.contains("Alicia"),
        "profile changes are not listed: {page}"
    );
}

#[test]
fn date_separators_follow_the_page_time_zone() {
    // 14:59Z and 15:01Z on 2025-09-25 fall on two different days in Tokyo.
    let page = render(&[
        text("$1", "@alice:example.org", 14 * 60 + 59, "late"),
        text("$2", "@alice:example.org", 15 * 60 + 1, "next day"),
    ]);
    assert!(page.contains("2025-09-25"), "{page}");
    assert!(page.contains("2025-09-26"), "{page}");
    assert!(
        page.contains(">23:59<") && page.contains(">00:01<"),
        "{page}"
    );
}

#[test]
fn a_quote_in_a_message_that_is_not_a_reply_is_kept() {
    let page = render(&[
        text("$1", "@alice:example.org", 0, "> quoted line\n\nmy answer"),
        text("$2", "@alice:example.org", 1, "> only a quote"),
    ]);
    assert!(page.contains("&gt; quoted line"), "{page}");
    assert!(page.contains("my answer"));
    assert!(page.contains("&gt; only a quote"), "{page}");
}
