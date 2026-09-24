//! `rooms/<folder>/index.html`: one room's history as a readable page.
//!
//! Two passes over `events.jsonl`: the first indexes reactions, edits and
//! reply excerpts by event id, the second writes one entry per displayable
//! event. Lines that are not JSON objects are skipped.

use std::collections::HashMap;

use koushi_protocol::HistoryExportLabels;
use serde_json::Value;

use super::{civil, escape, format_size, head, href_path, label, zone};
use crate::room_history_export::layout::fnv1a_hex;
use crate::room_history_export::records::{
    AttachmentIndex, AttachmentKind, AttachmentRecord, AttachmentStatus, RoomMeta,
};
use crate::timeline::html_sanitize::sanitize_matrix_html;

const EXCERPT_CHARS: usize = 80;

struct Edit<'a> {
    timestamp: u64,
    content: &'a Value,
}

#[derive(Default)]
struct Index<'a> {
    /// Target event id → (key, count) in first-seen order.
    reactions: HashMap<&'a str, Vec<(&'a str, u64)>>,
    /// Target event id → latest same-sender replacement.
    edits: HashMap<&'a str, Edit<'a>>,
    /// Message event id → sender.
    senders: HashMap<&'a str, &'a str>,
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn relates_to(event: &Value) -> Option<&Value> {
    event.get("content")?.get("m.relates_to")
}

fn relation(event: &Value) -> Option<(&str, &str)> {
    let relates_to = relates_to(event)?;
    Some((
        str_at(relates_to, "rel_type")?,
        str_at(relates_to, "event_id")?,
    ))
}

fn is_state(event: &Value) -> bool {
    event.get("state_key").is_some()
}

fn timestamp(event: &Value) -> u64 {
    event
        .get("origin_server_ts")
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

fn parse_lines(events_jsonl: &[u8]) -> Vec<Value> {
    events_jsonl
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter(Value::is_object)
        .collect()
}

fn build_index(events: &[Value]) -> Index<'_> {
    let mut index = Index::default();
    for event in events {
        if let (Some(event_id), Some(sender)) = (str_at(event, "event_id"), str_at(event, "sender"))
        {
            index.senders.insert(event_id, sender);
        }
    }
    for event in events {
        let (Some(event_type), Some(sender)) = (str_at(event, "type"), str_at(event, "sender"))
        else {
            continue;
        };
        match (event_type, relation(event)) {
            ("m.reaction", Some(("m.annotation", target))) => {
                let Some(key) = relates_to(event).and_then(|relates| str_at(relates, "key")) else {
                    continue;
                };
                let counts = index.reactions.entry(target).or_default();
                match counts.iter_mut().find(|(existing, _)| *existing == key) {
                    Some((_, count)) => *count += 1,
                    None => counts.push((key, 1)),
                }
            }
            (_, Some(("m.replace", target))) => {
                // Only the original sender may edit a message.
                if index.senders.get(target) != Some(&sender) {
                    continue;
                }
                let Some(content) = event.get("content").and_then(|c| c.get("m.new_content"))
                else {
                    continue;
                };
                let edit = Edit {
                    timestamp: timestamp(event),
                    content,
                };
                if index
                    .edits
                    .get(target)
                    .is_none_or(|existing| existing.timestamp <= edit.timestamp)
                {
                    index.edits.insert(target, edit);
                }
            }
            _ => {}
        }
    }
    index
}

fn anchor(event_id: &str) -> String {
    format!("e-{}", fnv1a_hex(event_id))
}

fn excerpt(content: &Value) -> String {
    let body = str_at(content, "body").unwrap_or_default();
    let body = strip_reply_fallback(body);
    let line = body.lines().next().unwrap_or_default();
    let mut text: String = line.chars().take(EXCERPT_CHARS).collect();
    if line.chars().count() > EXCERPT_CHARS {
        text.push('…');
    }
    text
}

/// Drop the `> ` quote lines a client prepends to a reply's plain body.
fn strip_reply_fallback(body: &str) -> &str {
    if !body.starts_with("> ") {
        return body;
    }
    let mut rest = body;
    while rest.starts_with('>') {
        match rest.find('\n') {
            Some(end) => rest = &rest[end + 1..],
            None => return "",
        }
    }
    rest.strip_prefix('\n').unwrap_or(rest)
}

fn is_redacted(event: &Value) -> bool {
    event
        .get("unsigned")
        .and_then(|unsigned| unsigned.get("redacted_because"))
        .is_some()
}

struct Page<'a> {
    out: String,
    meta: &'a RoomMeta,
    labels: &'a HistoryExportLabels,
    attachments: std::collections::BTreeMap<&'a str, &'a AttachmentRecord>,
    events_by_id: HashMap<&'a str, &'a Value>,
    zone: jiff::tz::TimeZone,
    current_date: Option<String>,
}

pub(crate) fn render_room_page(
    events_jsonl: &[u8],
    meta: &RoomMeta,
    attachments: &AttachmentIndex,
    labels: &HistoryExportLabels,
) -> Vec<u8> {
    let events = parse_lines(events_jsonl);
    let index = build_index(&events);
    let events_by_id = events
        .iter()
        .filter_map(|event| str_at(event, "event_id").map(|id| (id, event)))
        .collect();

    let mut page = Page {
        out: String::new(),
        meta,
        labels,
        attachments: attachments.by_event(),
        events_by_id,
        zone: zone(&meta.time_zone),
        current_date: None,
    };
    page.header();
    for event in &events {
        page.event(event, &index);
    }
    page.out.push_str("</main>\n</body>\n</html>\n");
    page.out.into_bytes()
}

impl Page<'_> {
    fn header(&mut self) {
        head(
            &mut self.out,
            &self.labels.lang,
            &self.meta.name,
            "../../assets/",
            true,
        );
        let (date, time, _) = civil(self.meta.exported_at_ms, &self.zone);
        self.out.push_str("<body>\n<header>\n<h1>");
        self.out.push_str(&escape(&self.meta.name));
        self.out.push_str("</h1>\n");
        if !self.meta.topic.trim().is_empty() {
            self.out.push_str("<p class=\"topic\">");
            self.out.push_str(&escape(&self.meta.topic));
            self.out.push_str("</p>\n");
        }
        self.out.push_str("<p class=\"meta\">");
        self.out.push_str(&label(
            &self.labels.exported_at,
            &[("date", &format!("{date} {time}"))],
        ));
        self.out.push_str(" · ");
        self.out.push_str(&label(
            &self.labels.times_in_zone,
            &[("timeZone", &self.meta.time_zone)],
        ));
        self.out.push_str("</p>\n</header>\n<main>\n");
    }

    fn date_separator(&mut self, date: &str) {
        if self.current_date.as_deref() != Some(date) {
            self.out.push_str("<h2 class=\"date\">");
            self.out.push_str(&escape(date));
            self.out.push_str("</h2>\n");
            self.current_date = Some(date.to_owned());
        }
    }

    fn event(&mut self, event: &Value, index: &Index<'_>) {
        let (Some(event_type), Some(event_id), Some(sender)) = (
            str_at(event, "type"),
            str_at(event, "event_id"),
            str_at(event, "sender"),
        ) else {
            return;
        };
        if matches!(event_type, "m.reaction" | "m.room.redaction")
            || matches!(relation(event), Some(("m.replace", _)))
        {
            return;
        }
        if is_state(event) {
            if let Some(line) = self.state_line(event, event_type, sender) {
                let (date, _, _) = civil(timestamp(event), &self.zone);
                self.date_separator(&date);
                self.out.push_str(&format!(
                    "<p class=\"state\" id=\"{}\">{line}</p>\n",
                    anchor(event_id)
                ));
            }
            return;
        }
        let content = event.get("content").cloned().unwrap_or(Value::Null);
        let displayable = matches!(
            event_type,
            "m.room.message" | "m.sticker" | "m.room.encrypted"
        ) || str_at(&content, "body").is_some()
            || is_redacted(event);
        if !displayable {
            return;
        }

        let (date, time, datetime) = civil(timestamp(event), &self.zone);
        self.date_separator(&date);
        let sender_name = self.meta.sender_name(sender).to_owned();
        self.out.push_str(&format!(
            "<article id=\"{}\">\n<div class=\"head\"><span class=\"sender\">{}</span><time datetime=\"{}\">{}</time>",
            anchor(event_id),
            escape(&sender_name),
            escape(&datetime),
            escape(&time)
        ));
        let thread_root = match relation(event) {
            Some(("m.thread", root)) => Some(root),
            _ => None,
        };
        if let Some(root) = thread_root {
            self.out.push_str(&format!(
                "<span class=\"thread\">{} · <a href=\"#{}\">{}</a></span>",
                escape(&self.labels.thread_reply),
                anchor(root),
                escape(&self.labels.thread_root_link)
            ));
        }
        self.out.push_str("</div>\n");

        if is_redacted(event) {
            self.placeholder(&self.labels.redacted.clone());
            self.out.push_str("</article>\n");
            return;
        }
        if event_type == "m.room.encrypted"
            || str_at(&content, "msgtype") == Some("m.bad.encrypted")
        {
            self.placeholder(&self.labels.undecryptable.clone());
            self.out.push_str("</article>\n");
            return;
        }

        self.reply(&content, thread_root.is_some());
        let edit = index.edits.get(event_id);
        let shown = edit.map_or(&content, |edit| edit.content);
        self.body(shown, str_at(&content, "msgtype"), &sender_name);
        if let Some(record) = self.attachments.get(event_id).copied() {
            self.attachment(record);
        }
        if edit.is_some() {
            self.out.push_str(&format!(
                "<p class=\"edited\">{}</p>\n",
                escape(&self.labels.edited)
            ));
        }
        if let Some(reactions) = index.reactions.get(event_id) {
            self.out.push_str(&format!(
                "<ul class=\"reactions\" aria-label=\"{}\">",
                escape(&self.labels.reactions)
            ));
            for (key, count) in reactions {
                self.out
                    .push_str(&format!("<li>{} {count}</li>", escape(key)));
            }
            self.out.push_str("</ul>\n");
        }
        self.out.push_str("</article>\n");
    }

    fn placeholder(&mut self, text: &str) {
        self.out
            .push_str(&format!("<p class=\"placeholder\">{}</p>\n", escape(text)));
    }

    fn reply(&mut self, content: &Value, in_thread: bool) {
        let Some(relates_to) = content.get("m.relates_to") else {
            return;
        };
        if in_thread
            && relates_to
                .get("is_falling_back")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            return;
        }
        let Some(target) = relates_to
            .get("m.in_reply_to")
            .and_then(|reply| str_at(reply, "event_id"))
        else {
            return;
        };
        match self.events_by_id.get(target).copied() {
            Some(original) => {
                let name = str_at(original, "sender")
                    .map(|sender| self.meta.sender_name(sender).to_owned())
                    .unwrap_or_default();
                let quoted = original.get("content").map(excerpt).unwrap_or_default();
                self.out.push_str(&format!(
                    "<blockquote class=\"reply\"><a href=\"#{}\">{}</a> {}</blockquote>\n",
                    anchor(target),
                    label(&self.labels.in_reply_to, &[("name", &name)]),
                    escape(&quoted)
                ));
            }
            None => self.out.push_str(&format!(
                "<blockquote class=\"reply\">{}</blockquote>\n",
                escape(&self.labels.reply_unavailable)
            )),
        }
    }

    fn body(&mut self, content: &Value, msgtype: Option<&str>, sender_name: &str) {
        if matches!(msgtype, Some("m.image" | "m.file" | "m.video" | "m.audio")) {
            return;
        }
        let formatted = (str_at(content, "format") == Some("org.matrix.custom.html"))
            .then(|| str_at(content, "formatted_body"))
            .flatten()
            .map(|formatted| sanitize_matrix_html(formatted, &["img"]))
            .filter(|sanitized| !sanitized.trim().is_empty());
        let html = match formatted {
            Some(html) => html,
            None => {
                let body = strip_reply_fallback(str_at(content, "body").unwrap_or_default());
                escape(body).replace('\n', "<br>")
            }
        };
        let prefix = if msgtype == Some("m.emote") {
            format!("* {} ", escape(sender_name))
        } else {
            String::new()
        };
        self.out
            .push_str(&format!("<div class=\"body\">{prefix}{html}</div>\n"));
    }

    fn attachment(&mut self, record: &AttachmentRecord) {
        self.out.push_str("<div class=\"attachment\">");
        match (&record.status, &record.file) {
            (AttachmentStatus::Retrieved, Some(file)) => {
                let href = href_path(file);
                let is_image =
                    matches!(record.kind, AttachmentKind::Image | AttachmentKind::Sticker);
                match (&record.thumb, is_image) {
                    (Some(thumb), true) => self.out.push_str(&format!(
                        "<a href=\"{href}\"><img src=\"{}\" alt=\"{}\" loading=\"lazy\"></a>",
                        href_path(thumb),
                        escape(&record.name)
                    )),
                    (_, true) => self
                        .out
                        .push_str(&format!("<a href=\"{href}\">{}</a>", escape(&record.name))),
                    (_, false) => self.out.push_str(&format!(
                        "<a href=\"{href}\" download>{}</a>",
                        escape(&record.name)
                    )),
                }
                if let Some(size) = record.size {
                    self.out.push_str(&format!(
                        "<span class=\"size\">{}</span>",
                        format_size(size)
                    ));
                }
            }
            _ => self.out.push_str(&format!(
                "<span class=\"missing\">{} — {}</span>",
                escape(&record.name),
                escape(&self.labels.not_retrieved)
            )),
        }
        self.out.push_str("</div>\n");
    }

    fn state_line(&self, event: &Value, event_type: &str, sender: &str) -> Option<String> {
        let name = self.meta.sender_name(sender);
        let content = event.get("content").cloned().unwrap_or(Value::Null);
        let labels = self.labels;
        Some(match event_type {
            "m.room.member" => {
                let target = str_at(event, "state_key").unwrap_or_default();
                let previous = event
                    .get("unsigned")
                    .and_then(|unsigned| unsigned.get("prev_content"))
                    .and_then(|previous| str_at(previous, "membership"));
                let target_name = str_at(&content, "displayname")
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| {
                        event
                            .get("unsigned")
                            .and_then(|unsigned| unsigned.get("prev_content"))
                            .and_then(|previous| str_at(previous, "displayname"))
                    })
                    .unwrap_or_else(|| self.meta.sender_name(target));
                let own = sender == target;
                match str_at(&content, "membership")? {
                    "join" if previous == Some("join") => return None,
                    "join" => label(&labels.state_joined, &[("name", target_name)]),
                    "leave" if own => label(&labels.state_left, &[("name", target_name)]),
                    "leave" => label(
                        &labels.state_removed,
                        &[("name", name), ("target", target_name)],
                    ),
                    "invite" => label(
                        &labels.state_invited,
                        &[("name", name), ("target", target_name)],
                    ),
                    "ban" => label(
                        &labels.state_banned,
                        &[("name", name), ("target", target_name)],
                    ),
                    _ => return None,
                }
            }
            "m.room.name" => label(
                &labels.state_renamed,
                &[
                    ("name", name),
                    ("value", str_at(&content, "name").unwrap_or_default()),
                ],
            ),
            "m.room.topic" => label(
                &labels.state_topic,
                &[
                    ("name", name),
                    ("value", str_at(&content, "topic").unwrap_or_default()),
                ],
            ),
            "m.room.avatar" => label(&labels.state_avatar, &[("name", name)]),
            other => label(&labels.state_other, &[("name", name), ("type", other)]),
        })
    }
}
