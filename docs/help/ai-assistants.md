# Connect an AI assistant (MCP)

Koushi can act as a local [Model Context Protocol](https://modelcontextprotocol.io)
server for Claude Code, Codex, pi, DeepSeek Harness, and any other stdio MCP
client. The assistant runs on the same computer and talks to the Koushi app you
already have open, so it can search your messages and send replies without ever
holding your encryption keys. Koushi asks you before an assistant can use each
account and each permission.

[Back to the user guide](README.md)

## What the assistant can do

| Tool | What it does | Permission |
| --- | --- | --- |
| `list_accounts` | Lists the signed-in accounts and whether each is ready. | none |
| `list_rooms` | Lists rooms and direct messages with names, encryption state, unread counts, and the latest event's sender and time. | read |
| `read_room_messages` | Reads a room's recent messages, newest first, with sender, time, and text. | read |
| `search_messages` | Searches decrypted history and returns room, event id, sender, timestamp, and a snippet. | read |
| `get_attachment` | Downloads one attachment into Koushi's media cache and returns its path. | attachments |
| `send_message` | Sends a message, optionally as a reply. | send |
| `wait_for_events` | Waits for new activity in the account and returns changed rooms. | read |

Every tool needs an `account_id`. The assistant gets it from `list_accounts`;
Koushi never guesses which account you meant.

`list_rooms` answers the "what is unread" question: `unread_messages`,
`unread_notifications`, `unread_mentions`, the thread equivalents, and
`marked_unread` come from the room list, and `last_event` carries the newest
event's sender, time, and preview. Pass `unread_only: true` to get only rooms
with something waiting. `read_room_messages` then reads a room whether or not it
is open, and `search_messages` returns a timestamp for every hit, so an assistant
can date a conversation instead of guessing.

## Before you start

- Koushi is running and signed in. If Koushi is closed, tools answer
  `koushi_unavailable` and recover when you reopen it.
- **App Settings → AI assistants → Let AI assistants connect** is on. It is on
  by default; turning it off disconnects every assistant at once.
- The `koushi-mcp` launcher ships with Koushi. The settings page shows its
  path inside the registration text, so you never need to find it yourself.
  - On macOS, run Koushi from the Applications folder. A copy opened from the
    disk image or a temporary location cannot be used, and the page says so.
  - The Linux AppImage copies the launcher to
    `~/.local/share/koushi-desktop/mcp/bin/koushi-mcp`, because the AppImage's
    own path changes every time it runs.
  - A build from source without the launcher shows "does not include the
    koushi-mcp launcher". Build it in a checkout of this repository with
    `cargo build --release -p koushi-mcp`; the binary is
    `target/release/koushi-mcp`.
- The assistant runs as the same operating-system user as Koushi. The bridge
  socket and its token are readable only by that user.

## Step 1: register the assistant

Open **App Settings → AI assistants**. Under **Connect an assistant**, pick your
assistant, select **Copy**, and run or paste the text where the page says.
Koushi never edits another tool's configuration itself.

| Assistant | What to do with the copied text |
| --- | --- |
| Claude Code | Run it in a terminal: `claude mcp add --scope user koushi -- <launcher> --client-name claude-code` |
| Codex | Run it in a terminal: `codex mcp add koushi -- <launcher> --client-name codex` |
| pi (1.0 or later) | Run it in a terminal: `pi mcp add koushi -- <launcher> --client-name pi` |
| DeepSeek Harness | Add the YAML entry (a `@deepseek-ai/dsh-mcp-client` plugin with `transport: stdio`) to `~/.dsh/profiles/<profile>/cordis.patch.yml`. Install the client plugin in that profile first. |
| Other | Add the `mcpServers` JSON entry to the client's MCP configuration. |

`--client-name` is the name Koushi shows when the assistant asks for access.
Keep it the same, or Koushi treats the assistant as a new one. Every command also
accepts an explicit endpoint file with `--endpoint /path/to/endpoint.json`; the
default location is discovered automatically.

Then ask the assistant something, for example "List my Koushi accounts and
rooms." `claude mcp list`, `codex mcp list`, or `pi mcp list` show that the
registration exists; only a tool call shows that Koushi is reachable.

## Register another MCP client

Any client that can start a stdio server works. The **Other** text is the
launcher's absolute path plus a client name:

```json
{
  "mcpServers": {
    "koushi": {
      "command": "/Applications/Koushi.app/Contents/MacOS/koushi-mcp",
      "args": ["--client-name", "my-client"]
    }
  }
}
```

Client names may use letters, digits, `.`, `_`, and `-`, up to 64 characters.
Clients launched from the Dock often do not inherit your shell `PATH`, so keep
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
     --mcp-command "/Applications/Koushi.app/Contents/MacOS/koushi-mcp --client-name my-client"
   tunnel-client doctor --profile koushi --explain
   tunnel-client run --profile koushi
   ```

4. Register the assistant against the tunnel. In ChatGPT that is
   **Plugins → Add custom MCP server → Connection: Tunnel**. A client that
   only takes a URL and a header needs the endpoint the tunnel exposes; use
   header authentication, because connectors that only support OAuth servers
   cannot reach Koushi.
5. Ask the assistant something. Koushi asks you to approve the client name you
   passed to `--client-name`, as for any other assistant.

The tunnel opens no inbound port, but everything it exposes stays reachable
while `tunnel-client run` is alive: stop it when you are done. Each request has
a 120-second deadline and a 10 MiB payload limit, and the assistant's vendor can
call the tools for as long as the connector is authorized. Not every hosted
connector accepts a tunnel endpoint; one that only takes a public `https://`
URL cannot reach Koushi, and no local workaround makes it reachable.

## Step 2: approve it in Koushi

The first time an assistant needs something, Koushi shows a dialog and the
assistant's call waits for your answer (up to 40 seconds):

- **Per account.** The first call that touches an account asks about that
  account only. Other accounts stay hidden until the assistant uses them and you
  approve them too.
- **Per permission.** Reading, sending, and downloading attachments are asked
  separately, when first needed. An assistant you allowed to read asks again the
  first time it sends.
- **Allow** keeps that permission for that account. After you allow sending,
  later messages are sent without asking again.
- **Deny** refuses that permission. Koushi remembers the denial until you turn
  the permission on in settings, remove the assistant, turn the bridge off, or
  restart Koushi, so a retrying assistant cannot ask again and again.
- Closing the dialog without answering leaves the request open for five
  minutes. If the assistant gave up waiting, it gets `consent_required`; once
  you allow, its next try works.

For a send, the dialog shows the message the assistant is waiting to send, as
context. If the assistant stopped waiting, that message is not sent; your answer
still applies to its later messages.

The name in the dialog is the one the assistant passed with `--client-name`.
Koushi cannot verify it: any program running as you could use the same name.
Allow a request only when you just set up or asked that assistant.

### Change or remove access

**App Settings → AI assistants → Connected assistants** lists every assistant
that has asked, with a **Read**, **Send**, and **Attachments** switch for each
account. A permission you denied shows **Denied**; turning its switch on allows
it. **Remove access** deletes everything for that assistant. Changes apply on
the assistant's next call.

### Where permissions are stored

Koushi keeps them in `connections.json` in its data directory:

- macOS: `~/Library/Application Support/koushi-desktop/mcp/connections.json`
- Windows: `%LOCALAPPDATA%\koushi-desktop\mcp\connections.json`
- Linux: `~/.local/share/koushi-desktop/mcp/connections.json`

You normally never edit it. The file is re-read on every tool call, so a hand
edit applies immediately:

```json
{
  "schema": 2,
  "connections": [
    {
      "client_name": "claude-code",
      "accounts": [
        { "account_id": "account:@you:example.org", "read": true, "send": false, "attachments": false }
      ]
    }
  ]
}
```

Files written by earlier versions (`accounts` as a list of ids with
`allow_read`, `allow_send`, and `allow_attachments`) still work and are upgraded
the next time Koushi saves a change. If Koushi cannot read the file, no assistant
has access and the settings page says so; Koushi does not overwrite a file it
cannot read, so fix or remove it.

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

- `read_room_messages` decrypts each page before returning it, so a message
  this device cannot decrypt comes back with its sender and time but no text.
  Only message-like events are returned; membership and other state events are
  skipped.
- Search results cover what the local index has crawled; a very recent message
  may not be searchable yet. `read_room_messages` reads the room directly and
  does not depend on the crawl.
- Replies are supported with `reply_to_event_id`; starting a new thread is not.
- `get_attachment` returns one file per call, always the first attachment of the
  message. Koushi chooses the destination inside its media cache; the assistant
  cannot write elsewhere.
- Permissions are per account, not per room.
- On Windows, the bundled `koushi-mcp.exe` is unsigned, like the installer.

## Troubleshooting

| Message | Meaning | Fix |
| --- | --- | --- |
| `koushi_unavailable` | Koushi is not running, **Let AI assistants connect** is off, or the bridge stopped while the call waited. | Open Koushi, check **App Settings → AI assistants**, and retry. |
| `Operation not permitted` | The execution environment blocked the local socket connection before Koushi saw it. | Ask the environment for its escalated execution on that call; do not disable its sandbox. |
| `consent_required` | Nobody answered the approval in Koushi in time. | Answer the dialog in Koushi, then retry. |
| `not_permitted` | You denied this permission, or removed the assistant while it waited. | Turn the permission on in **App Settings → AI assistants**, then retry. |
| `account_unavailable` | The account is signed out or still restoring. | Sign in or wait for restore to finish. |
| `room_not_found` | The room is unknown to that account or you have left it. | List rooms again and use a room id the account is joined to. |
| `event_not_found` | The event id is unknown to the room. | Re-run `search_messages` for a current event id. |
| `busy` | This account already runs the maximum number of concurrent searches from assistants, or too many approvals are waiting. | Answer or close waiting approvals, wait for earlier calls, and retry. |
| `internal: Koushi could not save the approval` | `connections.json` could not be written or read. | See the message in **App Settings → AI assistants**; fix or remove the file. |
| `unsupported` | The assistant asked for a tool this build does not implement. | Update Koushi. |

If the launcher exits with "client name must be 1-64 characters", choose a
`--client-name` made of letters, digits, `.`, `_`, and `-`.
If the launcher reports it cannot locate the endpoint file, pass
`--endpoint` explicitly. If Koushi refuses to read its own endpoint file,
check that the file is readable only by you (`chmod 600`).
