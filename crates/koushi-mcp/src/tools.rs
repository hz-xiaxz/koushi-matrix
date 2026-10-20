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
        },
        ToolDefinition {
            name: "wait_for_events",
            description: "Wait up to timeout_s for new messages or invites in the account. Returns immediately when events are pending, otherwise at the timeout. The returned cursor is the next position.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "account_id": account_id_property(),
                    "cursor": { "type": "string", "description": "Cursor from the previous call; omit to start at the newest event." },
                    "timeout_s": { "type": "integer", "minimum": 0, "maximum": 50 }
                },
                "required": ["account_id"],
                "additionalProperties": false
            }),
            read_only: true,
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
        assert_eq!(tools.len(), 7);
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
}
