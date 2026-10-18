# Connect an AI assistant (MCP)

Koushi can act as a local [Model Context Protocol](https://modelcontextprotocol.io)
server for Claude Code, Codex, and any other stdio MCP client. The assistant
runs on the same computer and
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

## Register another MCP client

Any client that can start a stdio server works. The entry is the launcher's
absolute path plus the client name you will grant:

```json
{
  "mcpServers": {
    "koushi": {
      "command": "/Users/you/bin/koushi-mcp",
      "args": ["--client-name", "my-client"]
    }
  }
}
```

Clients launched from the Dock often do not inherit your shell `PATH`, so give
the full path. If the client runs under a different home directory, pass
`--endpoint` explicitly.

## Clients that can only reach a URL

Some assistants accept nothing but an `https://` MCP URL: hosted connectors such
as ChatGPT's, and chat surfaces that never run a process on your machine. They
cannot start the launcher and cannot open Koushi's socket. Bridge the gap with
a tunnel instead of exposing the socket:

```text
remote assistant -> OpenAI-hosted tunnel endpoint -> tunnel-client (your Mac)
                 -> koushi-mcp --client-name <name> -> Koushi
```

1. Create a tunnel in [Platform tunnel settings](https://platform.openai.com/settings/organization/tunnels)
   and note the `tunnel_id`. Associate it with the ChatGPT workspace and the
   Platform organization the assistant uses; a tunnel tied to only one of them
   does not appear in the connector.
2. Install `tunnel-client` from that page or from
   [openai/tunnel-client releases](https://github.com/openai/tunnel-client/releases/latest),
   with a runtime API key that has Tunnels **Read + Use**.
3. Point a profile at the launcher and keep it running while the assistant
   discovers and calls the tools:

   ```bash
   tunnel-client init --sample sample_mcp_stdio_local --profile koushi \
     --tunnel-id tunnel_0123456789abcdef0123456789abcdef \
     --mcp-command "/Users/you/bin/koushi-mcp --client-name my-client"
   tunnel-client doctor --profile koushi --explain
   tunnel-client run --profile koushi
   ```

4. Register the assistant against the tunnel. In ChatGPT that is
   **Plugins → Add custom MCP server → Connection: Tunnel**. A client that
   only takes a URL and a header needs the endpoint the tunnel exposes; use
   header authentication, because connectors that only support OAuth servers
   cannot reach Koushi.
5. Grant the same `client_name` in `connections.json`, starting with
   `allow_send` and `allow_attachments` set to `false`.

The tunnel opens no inbound port, but everything it exposes stays reachable
while `tunnel-client run` is alive: stop it when you are done. Each request has
a 120-second deadline and a 10 MiB payload limit, and the assistant's vendor can
call the tools for as long as the connector is authorized. Not every hosted
connector accepts a tunnel endpoint; one that only takes a public `https://`
URL cannot reach Koushi, and no local workaround makes it reachable.

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

## What assistant calls never change

Tool calls run inside Koushi's own runtime, but they are not GUI actions:

- `search_messages` does not open the search pane, replace its query or
  results, or cancel a search you are typing. Each call runs its own search
  with the room filter you passed and returns only to that caller.
- `get_attachment` reads the message from the account, not from an open
  timeline, so it works for rooms you have never opened. Files land in Koushi's
  media cache.
- `send_message` sends without touching a draft or the composer. The message
  appears in an open room through normal sync; the assistant's request and its
  errors stay with the assistant.

## Checking a connection

`initialize` and `tools/list` are answered by the launcher itself: they succeed
whether or not Koushi is running, so they do not show that the app is reachable.
Only a tool call crosses the socket. Confirm a connection with the read-only
`list_accounts` before reporting success or failure.

- If the execution environment already exposes Koushi's tools, use them rather
  than starting a second launcher.
- `koushi-mcp --client-name <name>` started by hand prints nothing until a
  request arrives on stdin. Silence is the waiting state, not a crash.

## Sandboxes and approvals

Two failures look similar and mean different things:

| Symptom | Likely cause | Next step |
| --- | --- | --- |
| `koushi_unavailable` | Koushi is closed, its socket is gone, or the endpoint file is unreadable. | Open Koushi and retry. On macOS the socket is `~/Library/Application Support/koushi-desktop/mcp/mcp.sock`. |
| `Operation not permitted` | The assistant's execution environment refused the local connection; Koushi was never asked. | Use the environment's own escalated execution for this call. |

- Request escalation through the mechanism the environment documents, for
  example `require_escalated` where it is supported. Name the exact operation and
  keep the scope to the socket access that call needs.
- Do not route the same call through another assistant or process to get around a
  refusal, and do not disable a sandbox or approval prompt wholesale to make the
  tools reachable.
- Koushi's grant, the environment's permission, and the user's request are three
  separate decisions. A working connection does not authorize sending, editing,
  or changing settings, and one successful test call is not ongoing access.

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
| `Operation not permitted` | The execution environment blocked the local socket connection before Koushi saw it. | Ask the environment for its escalated execution on that call; do not disable its sandbox. |
| `consent_required` | No grant exists for the assistant's client name. | Add an entry to `connections.json` with the same `client_name`. |
| `not_permitted` | The account is not listed, or the needed flag is `false`. | Add the account id or turn on the flag. |
| `account_unavailable` | The account is signed out or still restoring. | Sign in or wait for restore to finish. |
| `room_not_found` | The room is unknown to that account or you have left it. | List rooms again and use a room id the account is joined to. |
| `event_not_found` | The event id is unknown to the room. | Re-run `search_messages` for a current event id. |
| `busy` | This account already runs the maximum number of concurrent searches from assistants. | Wait for an earlier call to finish and retry. |
| `unsupported` | The assistant asked for a tool this build does not implement. | Update Koushi. |

If the launcher reports it cannot locate the endpoint file, pass
`--endpoint` explicitly. If Koushi refuses to read its own endpoint file,
check that the file is readable only by you (`chmod 600`).
