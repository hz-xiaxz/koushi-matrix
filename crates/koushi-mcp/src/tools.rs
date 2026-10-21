//! Tool definitions and argument validation.
//!
//! Schemas live here, not in the app, so `tools/list` still answers when Koushi
//! is not running. The app validates the same arguments again before dispatch;
//! this layer only stops malformed calls early and gives the model a stable
//! description.

use serde_json::{Value, json};

/// One MCP tool as advertised to the client.
#[derive(Clone, Debug)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub read_only: bool,
    /// The `outputSchema` of a tool whose result is typed structured content.
    pub output_schema: Option<Value>,
}

fn account_id_property() -> Value {
    json!({
        "type": "string",
        "description": "Stable account handle from list_accounts. Required; never inferred from the visible tab."
    })
}

fn limit_property(maximum: u32) -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": maximum,
        "description": "Maximum rows to return."
    })
}

/// The structured result of `wait_for_events` (#1551).
fn wait_for_events_output_schema() -> Value {
    let nullable_string = json!({ "type": ["string", "null"] });
    json!({
        "type": "object",
        "properties": {
            "events": {
                "type": "array",
                "description": "One entry per sampled room change after the cursor, oldest first.",
                "items": {
                    "type": "object",
                    "properties": {
                        "cursor": { "type": "string", "description": "Opaque position of this entry." },
                        "account_id": { "type": "string" },
                        "room_id": { "type": "string" },
                        "room_name": { "type": "string" },
                        "event_id": nullable_string,
                        "sender_id": nullable_string,
                        "sender_label": nullable_string,
                        "unread_count": { "type": "integer", "description": "The room's unread count when sampled; informational." },
                        "highlight_count": { "type": "integer" },
                        "recency_stamp": { "type": ["integer", "null"] }
                    },
                    "required": ["cursor", "account_id", "room_id", "event_id"]
                }
            },
            "cursor": { "type": "string", "description": "Opaque position to pass to the next call." },
            "coverage": {
                "type": "string",
                "enum": ["sampled_room_changes"],
                "description": "Always sampled_room_changes: entries coalesce bursts and are not a complete message log."
            }
        },
        "required": ["events", "cursor", "coverage"]
    })
}

/// Every tool this build advertises. Order is stable so `tools/list` diffs are
/// meaningful across runs.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_accounts",
            description: "List the signed-in accounts this connection may use. Returns a stable account_id, display name, user id, and availability.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            read_only: true,
            output_schema: None,
        },
        ToolDefinition {
            name: "list_rooms",
            description: "List rooms and direct messages the account can see, with encryption state, display name, unread counts, and the latest event's sender and timestamp. unread_only restricts the list to rooms with something unread. Requires account_id.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "account_id": account_id_property(),
                    "limit": limit_property(200),
                    "cursor": { "type": "string", "description": "Opaque cursor from the previous page." },
                    "unread_only": { "type": "boolean", "description": "Return only rooms with unread messages, notifications, mentions, or a manual unread mark. Defaults to false." }
                },
                "required": ["account_id"],
                "additionalProperties": false
            }),
            read_only: true,
            output_schema: None,
        },
        ToolDefinition {
            name: "search_messages",
            description: "Search decrypted message history through Koushi's existing encrypted index. Results carry account provenance, room id, event id, sender, timestamp, and a snippet. Requires account_id.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "account_id": account_id_property(),
                    "query": { "type": "string", "minLength": 1, "description": "Search text." },
                    "room_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Restrict to these rooms. The connection grant narrows this further."
                    },
                    "limit": limit_property(100)
                },
                "required": ["account_id", "query"],
                "additionalProperties": false
            }),
            read_only: true,
            output_schema: None,
        },
        ToolDefinition {
            name: "read_room_messages",
            description: "Read a room's most recent messages, newest first, with sender, timestamp, and text. Works for a room nobody has opened and never changes the GUI. Pass next from a previous page as from to read older messages. Requires account_id and room_id.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "account_id": account_id_property(),
                    "room_id": { "type": "string" },
                    "limit": limit_property(100),
                    "from": { "type": "string", "description": "Opaque token from the previous page's next value." }
                },
                "required": ["account_id", "room_id"],
                "additionalProperties": false
            }),
            read_only: true,
            output_schema: None,
        },
        ToolDefinition {
            name: "get_attachment",
            description: "Download one attachment of a message into Koushi's managed media cache and return its path, mime type, and size. Koushi chooses the destination. Requires account_id, room_id, and event_id.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "account_id": account_id_property(),
                    "room_id": { "type": "string" },
                    "event_id": { "type": "string" },
                    "thumbnail": { "type": "boolean", "description": "Return a thumbnail instead of the original file. Defaults to false." }
                },
                "required": ["account_id", "room_id", "event_id"],
                "additionalProperties": false
            }),
            read_only: true,
            output_schema: None,
        },
        ToolDefinition {
            name: "send_message",
            description: "Send a message through the account's verified encrypted session. body is plain text. attachments are absolute local file paths (at most 10, 50 MB each, 100 MB per call) that Koushi reads and sends as one media message per file, with body as the first file's caption; every file is checked and read before the first is sent. Sending files needs the account's attachments permission as well as send. reply_to_event_id cannot be combined with attachments. Returns event_id and transaction_id of the first message and, when files were sent, an attachments list with one event_id and transaction_id per file. client_request_id makes a retry idempotent.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "account_id": account_id_property(),
                    "room_id": { "type": "string" },
                    "body": { "type": "string", "description": "Plain text body. Required unless attachments are present; with attachments it captions the first file." },
                    "reply_to_event_id": { "type": "string", "description": "Event to reply to. Not allowed together with attachments." },
                    "attachments": {
                        "type": "array",
                        "maxItems": 10,
                        "description": "Local files to send, each as its own media message, in this order.",
                        "items": {
                            "type": "object",
                            "properties": { "path": { "type": "string", "description": "Absolute path of a regular file readable by Koushi." } },
                            "required": ["path"],
                            "additionalProperties": false
                        }
                    },
                    "client_request_id": {
                        "type": "string",
                        "description": "Caller-generated UUID. Reuse it verbatim when retrying so the send is not duplicated."
                    }
                },
                "required": ["account_id", "room_id", "client_request_id"],
                "additionalProperties": false
            }),
            read_only: false,
            output_schema: None,
        },
        ToolDefinition {
            name: "read_recent_activity",
            description: "Read account-wide Activity Recent, including already-read items, without opening Activity or changing GUI navigation, unread counts or receipts. This is bounded observed history, not a complete archive: at most 200 existing Activity rows, muted/low-priority exclusions and notification filters apply. No background history resolver is started. Newest first using Core ordering. Unknown read state is explicit. Default limit 50, maximum 100. Opaque immutable snapshot cursors expire after 120 seconds, restart, account/session/caller/query/access changes or bounded-cache eviction; repeat all query fields when continuing. resync_required means start a fresh read without cursor and use read_room_messages for available surrounding history, deduplicating account/room/event IDs. Exhaustion or an empty result does not imply no messages occurred.",
            input_schema: json!({
                "type":"object", "properties":{
                    "account_id": account_id_property(), "limit":limit_property(100),
                    "room_ids":{"type":"array","items":{"type":"string"},"maxItems":64},
                    "from_timestamp_ms":{"type":"integer","minimum":0,"description":"Inclusive timestamp boundary."},
                    "to_timestamp_ms":{"type":"integer","minimum":0,"description":"Exclusive timestamp boundary."},
                    "cursor":{"type":"string","description":"Opaque continuation; repeat the same filters and limit."}
                },"required":["account_id"],"additionalProperties":false
            }),
            read_only: true,
            output_schema: Some(json!({"type":"object", "properties":{
                "account_id":{"type":"string"}, "items":{"type":"array", "items":{"type":"object", "properties":{
                    "account_id":{"type":"string"},"room_id":{"type":"string"},
                    "event_id":{"type":["string","null"]},"sender_id":{"type":["string","null"]},
                    "thread_root_event_id":{"type":["string","null"]},"timestamp_ms":{"type":"integer"},
                    "preview":{"type":["string","null"]},"content_available":{"type":"boolean"},
                    "preview_truncated":{"type":"boolean"},"metadata_truncated":{"type":"boolean"},"read_state":{"const":"unknown"}
                },"required":["account_id","room_id","event_id","sender_id","timestamp_ms","preview","read_state","content_available","preview_truncated"]}},
                "cursor":{"type":["string","null"]},"snapshot_timestamp_ms":{"type":"integer"},
                "coverage":{"const":"bounded_observed_recent"},"history_complete":{"const":false},
                "retention_limit":{"const":200},"truncated":{"type":"boolean"},"expires_after_s":{"const":120}
            },"required":["account_id","items","cursor","snapshot_timestamp_ms","coverage","history_complete","truncated","expires_after_s","retention_limit"]})),
        },
        ToolDefinition {
            name: "wait_for_events",
            description: "Wait up to timeout_s for sampled room changes in the account. This is a change feed, not a message log: Koushi samples each joined room's latest event about every 2 seconds and records one entry per room whose latest event changed since the previous sample, so several messages posted between samples coalesce into one entry. Invites are not reported. The first sample after Koushi or the bridge starts only sets a baseline. Entries are ordered by cursor, kept in memory only (at most 500 across all accounts), and are not removed when a room is read; unread counts are informational. Returns at once when entries are pending, otherwise at the timeout. An empty events list means no change was sampled, not that no message was posted. Omit cursor to start at the newest position (this does not recover earlier history); pass the returned cursor to continue. A cursor is opaque and valid only for this account and this Koushi run: a malformed, future, expired (older than the retained entries), other-account, or previous-run cursor fails with resync_required. To recover, call again without a cursor and use read_room_messages for the rooms you need, de-duplicating by account, room, and event id.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "account_id": account_id_property(),
                    "cursor": { "type": "string", "description": "Opaque cursor from the previous result. Omit to start at the newest position." },
                    "timeout_s": { "type": "integer", "minimum": 0, "maximum": 50, "description": "Seconds to wait when nothing is pending. Defaults to 25; an approval wait counts against a 60-second call budget." }
                },
                "required": ["account_id"],
                "additionalProperties": false
            }),
            read_only: true,
            output_schema: Some(wait_for_events_output_schema()),
        },
    ]
}

/// Looks a tool up by name.
pub fn find(name: &str) -> Option<ToolDefinition> {
    definitions().into_iter().find(|tool| tool.name == name)
}

/// Validates the shape of a `tools/call` argument object.
///
/// This is deliberately shallow: required properties, scalar types, and the
/// account-id rule. Semantic checks (membership, grant, account state) belong
/// to the app, which owns that state.
pub fn validate_arguments(name: &str, arguments: &Value) -> Result<(), String> {
    let Some(tool) = find(name) else {
        return Err(format!("unknown tool: {name}"));
    };
    let Value::Object(map) = arguments else {
        return Err("arguments must be an object".to_owned());
    };
    let schema = &tool.input_schema;
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required {
            let Some(key) = key.as_str() else { continue };
            if !map.contains_key(key) {
                return Err(format!("missing required argument: {key}"));
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, value) in map {
            let Some(property) = properties.get(key) else {
                return Err(format!("unknown argument: {key}"));
            };
            let expected = property.get("type").and_then(Value::as_str);
            let matches = match expected {
                Some("string") => value.is_string(),
                Some("integer") => value.is_i64() || value.is_u64(),
                Some("array") => value.is_array(),
                Some("object") => value.is_object(),
                Some("boolean") => value.is_boolean(),
                _ => true,
            };
            if !matches {
                return Err(format!("argument {key} has the wrong type"));
            }
        }
    }
    if map.contains_key("account_id")
        && map
            .get("account_id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err("account_id must be a non-empty string".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_are_unique_and_valid() {
        let tools = definitions();
        assert_eq!(tools.len(), 8);
        let mut names: Vec<&str> = tools.iter().map(|tool| tool.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), tools.len());
        for tool in &tools {
            assert_eq!(tool.input_schema["type"], "object");
            assert!(tool.input_schema.get("properties").is_some());
        }
    }

    #[test]
    fn account_scoped_tools_require_account_id() {
        for name in [
            "list_rooms",
            "search_messages",
            "wait_for_events",
            "read_room_messages",
            "read_recent_activity",
        ] {
            let tool = find(name).expect("tool");
            let required = tool.input_schema["required"].as_array().expect("required");
            assert!(required.iter().any(|value| value == "account_id"), "{name}");
        }
    }

    #[test]
    fn validation_rejects_missing_and_unknown_arguments() {
        assert!(validate_arguments("list_accounts", &json!({})).is_ok());
        assert!(validate_arguments("search_messages", &json!({"account_id": "a"})).is_err());
        assert!(
            validate_arguments(
                "search_messages",
                &json!({"account_id": "a", "query": "x", "bogus": 1})
            )
            .is_err()
        );
        assert!(
            validate_arguments("search_messages", &json!({"account_id": "", "query": "x"}))
                .is_err()
        );
        assert!(
            validate_arguments("search_messages", &json!({"account_id": "a", "query": "x"}))
                .is_ok()
        );
    }

    #[test]
    fn wait_for_events_publishes_its_coverage_and_cursor_contract() {
        let tool = find("wait_for_events").expect("tool");
        for phrase in [
            "not a message log",
            "coalesce",
            "Invites are not reported",
            "only sets a baseline",
            "at most 500",
            "not removed when a room is read",
            "not that no message was posted",
            "resync_required",
            "read_room_messages",
        ] {
            assert!(tool.description.contains(phrase), "{phrase}");
        }
        let schema = tool.output_schema.expect("output schema");
        assert_eq!(
            schema["properties"]["coverage"]["enum"],
            json!(["sampled_room_changes"])
        );
        let others_typed = definitions()
            .iter()
            .filter(|tool| tool.output_schema.is_some())
            .count();
        assert_eq!(others_typed, 2);
    }

    #[test]
    fn send_message_describes_its_attachment_contract() {
        let tool = find("send_message").expect("tool");
        for phrase in [
            "absolute local file paths",
            "attachments permission",
            "caption",
        ] {
            assert!(tool.description.contains(phrase), "{phrase}");
        }
        let path = &tool.input_schema["properties"]["attachments"]["items"]["properties"]["path"];
        assert!(
            path["description"]
                .as_str()
                .is_some_and(|text| text.starts_with("Absolute"))
        );
    }

    #[test]
    fn read_only_flags_match_the_write_tool() {
        let writes: Vec<&str> = definitions()
            .iter()
            .filter(|tool| !tool.read_only)
            .map(|tool| tool.name)
            .collect();
        assert_eq!(writes, vec!["send_message"]);
    }
    #[test]
    fn recent_activity_discovery_is_read_only_and_describes_bounded_history() {
        let tool = find("read_recent_activity").expect("Recent tool");
        assert!(tool.read_only);
        assert!(tool.description.contains("read"));
        assert!(tool.output_schema.is_some());
        assert_eq!(tool.input_schema["properties"]["limit"]["minimum"], 1);
        assert_eq!(tool.input_schema["properties"]["limit"]["maximum"], 100);
    }
}
