# Connect an AI assistant (MCP)

Koushi can act as a local [Model Context Protocol](https://modelcontextprotocol.io)
server for Claude Code and Codex. The assistant runs on the same computer and
talks to the Koushi app you already have open, so it can search your messages
and send replies without ever holding your encryption keys.

[Back to the user guide](README.md)

## What the assistant can do

| Tool | What it does | Grant needed |
| --- | --- | --- |
| `list_accounts` | Lists the signed-in accounts and whether each is ready. | none |
| `list_rooms` | Lists rooms and direct messages with names and encryption state. | read |
| `search_messages` | Searches decrypted history and returns room, event id, and a snippet. | read |
| `get_attachment` | Downloads one attachment into Koushi's media cache and returns its path. | attachments |
| `send_message` | Sends a message, optionally as a reply. | send |
| `wait_for_events` | Waits for new activity in the account and returns changed rooms. | read |

Every tool needs an `account_id`. The assistant gets it from `list_accounts`;
Koushi never guesses which account you meant.

## Before you start

- Koushi is running and signed in. If Koushi is closed, tools answer
  `koushi_unavailable` and recover when you reopen it.
- The `koushi-mcp` launcher is on disk. Released installs do not bundle it yet,
  so build it from source in a checkout of this repository:

  ```bash
  cargo build --release -p koushi-mcp
  ```

  The binary is `target/release/koushi-mcp`.
- The assistant runs as the same operating-system user as Koushi. The bridge
  socket and its token are readable only by that user.

## Step 1: register the assistant

Register the launcher with the assistant first. `--client-name` is the name you
will grant access to, so pick one and keep it.

Claude Code:

```bash
claude mcp add --scope user koushi -- /absolute/path/to/koushi-mcp --client-name claude-code
claude mcp list
```

Codex:

```bash
codex mcp add koushi -- /absolute/path/to/koushi-mcp --client-name codex
codex mcp list
```

Both commands also accept an explicit endpoint file with
`--endpoint /path/to/endpoint.json`; the default location is discovered
automatically.

## Step 2: grant access

Until you grant access, the assistant can only list account ids and their
status. Ask it to call `list_accounts` and read the `account_id` values, then
create or edit `connections.json` in Koushi's data directory:

- macOS: `~/Library/Application Support/koushi-desktop/mcp/connections.json`
- Windows: `%LOCALAPPDATA%\koushi-desktop\mcp\connections.json`
- Linux: `~/.local/share/koushi-desktop/mcp/connections.json`

```json
{
  "schema": 1,
  "connections": [
    {
      "client_name": "claude-code",
      "accounts": ["account:@you:example.org"],
      "allow_read": true,
      "allow_send": true,
      "allow_attachments": true
    }
  ]
}
```

- `client_name` must match the assistant's `--client-name`.
- `accounts` lists the account ids the assistant may touch. Nothing outside this
  list is visible, even by guessing a room id.
- `allow_read` covers `list_rooms`, `search_messages`, and `wait_for_events`.
- `allow_send` covers `send_message`.
- `allow_attachments` covers `get_attachment`.

The file is re-read on every tool call, so edits apply immediately. Start with
`allow_send` and `allow_attachments` set to `false` while you test the read
tools. Delete an entry to revoke that assistant completely.

There is no settings screen for this file yet; editing it by hand is the only
way to grant access in this version.

## Step 3: use it

Ask in normal language, for example:

- "List my Koushi accounts and rooms."
- "Search my messages for the invoice from last month."
- "Summarise what was said in the Design room today."
- "Reply to that message saying I will send the file tomorrow."
- "Download the attachment on that message."

Sending is a normal message from your account. Koushi does not mark it as
written by an assistant. Sending and replying work even when the target room
is not open in Koushi. They do not switch your selected room or change a message
you are drafting.

## New-message notification

Koushi cannot wake an idle assistant, so the assistant polls `wait_for_events`.
While Koushi is running, an internal poller checks room summaries every couple
of seconds and records rooms whose latest message changed. `wait_for_events`
returns immediately when something is waiting, otherwise it blocks up to
`timeout_s` (maximum 50 seconds).

The cursor is valid for the current Koushi run only. After restarting Koushi,
ask for events without a cursor to start from the newest position.

## Limitations

- Search results carry the room, the event id, and a snippet. The search index
  has no sender or timestamp fields today, so use `get_attachment` or the room
  timeline in Koushi to see full context.
- Replies are supported with `reply_to_event_id`; starting a new thread is not.
- `get_attachment` returns one file per call, always the first attachment of the
  message. Koushi chooses the destination inside its media cache; the assistant
  cannot write elsewhere.
- The launcher is not included in packaged releases yet.
- Grants are hand-edited JSON, described above.

## Troubleshooting

| Message | Meaning | Fix |
| --- | --- | --- |
| `koushi_unavailable` | Koushi is not running or has not written its endpoint file. | Open Koushi and retry. |
| `consent_required` | No grant exists for the assistant's client name. | Add an entry to `connections.json` with the same `client_name`. |
| `not_permitted` | The account is not listed, or the needed flag is `false`. | Add the account id or turn on the flag. |
| `account_unavailable` | The account is signed out or still restoring. | Sign in or wait for restore to finish. |
| `unsupported` | The assistant asked for a tool this build does not implement. | Update Koushi. |

If the launcher reports it cannot locate the endpoint file, pass
`--endpoint` explicitly. If Koushi refuses to read its own endpoint file,
check that the file is readable only by you (`chmod 600`).
