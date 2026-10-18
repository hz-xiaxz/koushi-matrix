# #1520 - Local MCP bridge: encrypted search, sending, attachment handoff

Status: implementation plan. Canon: `docs/architecture/overview.md`, "Layers",
"Runtime Model", "External MCP Bridge", "Security Model";
`docs/policies/engineering-rules.md`; `docs/agents/state-ownership.md`, "The
boundary" and "Timeline items and the outbound send queue". Related: #1519
(token display, separate feature). Scope narrowed with the user to message
search, sending (including attachments), and polling-based new-message
delivery.

## Problem

A signed-in, verified Koushi account can decrypt and search history that no
assistant can reach. Handing the assistant Matrix tokens or a second crypto
store is unacceptable: the same crypto database cannot be opened twice, and
tokens/keys must stay inside Koushi. There is no local surface today that an
external process can use, and no MCP server of any kind in the repository.

The user's requirement is narrower than #1520's original text: search, send
(with attachments), and being told about new messages. Immediate host wake-up
is not achievable over MCP — a stdio server only runs while the client calls
it — so new-message delivery is a user-configured poll, and the plan says so
instead of promising push.

## Scope

In v1:

- `list_accounts`, `list_rooms`, `search_messages`, `get_attachment`
- `send_message` with text, reply relation, and staged attachments
- `wait_for_events` (long-poll) with an opaque cursor, for `new_message` and
  `room_invite`
- One connection policy per client: accounts, room scope, read/write,
  attachment send, poll wait

Out of v1: `send_direct_message` (existing DMs send through `send_message`),
MCP resources/prompts/sampling, Streamable HTTP, DeepSeek Harness, automatic
`mcp add` execution, event-triggered execution, read receipts or unread
mutation.

## Design

### Process model

```text
Claude Code / Codex (host)
  └─ koushi-mcp            stdio child, one per client session, no Matrix client
       └─ Unix socket / named pipe
            └─ Koushi.app  Tauri adapter: endpoint file, socket, consent
                 └─ CoreConnection per account (koushi-core)
```

- `koushi-mcp` (new `crates/koushi-mcp`, lib + bin) translates MCP JSON-RPC to
  the local IPC protocol and back, and owns the tool schemas so `tools/list`
  answers while Koushi is down. It holds no Matrix credentials, opens no crypto
  store, and knows nothing about accounts beyond what the app returns.
- The socket listener lives in `apps/desktop/src-tauri` because it is a
  transport adapter, and because the endpoint file, consent UI, and app
  lifecycle already live there. It dispatches `CoreCommand`s through a
  dedicated `CoreConnection` attached to the account runtime, exactly as the
  non-Tauri renderer contract in `state-ownership.md` describes.
- Koushi is not a child of the client: quitting Codex leaves Koushi running,
  and quitting Koushi turns later calls into `koushi_unavailable` without
  killing the client session.

### Discovery and token

The app writes an endpoint file on startup and removes it on shutdown:

```text
macOS   ~/Library/Application Support/chat.koushi.desktop/mcp/endpoint.json
Linux   $XDG_RUNTIME_DIR/koushi/mcp.sock (fallback: app data dir)
Windows \\.\pipe\koushi-mcp-<hash of user SID>
```

```json
{ "schema": 1, "socket": "/…/mcp.sock", "token": "<base64url 32 bytes>",
  "pid": 12345, "app_version": "0.26.0" }
```

- File `0600`, parent directory `0700`.
- The token is generated per app launch, compared in constant time, never
  logged, and never passed on a command line (so it does not appear in `ps`).
- The client's own config contains only the launcher path; it never sees the
  token. This is the reason the token file shape was chosen.

### Connection and consent

1. Host spawns `koushi-mcp`; it reads `endpoint.json`, connects, and sends
   `hello { protocol: 1, token, client: { name, version } }`.
2. Bad token → `unauthorized` and close. Good token → the app records a
   pending connection and replies `hello_ok { capabilities }`.
3. The first account-scoped call opens a consent surface in the Koushi window:
   which accounts, all rooms or an allowlist, read, write, attachment send,
   and max poll wait. Until granted, every account-scoped call returns
   `consent_required`.
4. Grants persist in `SettingsValues.mcp.connections`, keyed by a stable
   client identity (`client.name` plus a user-visible label). The token is
   never persisted; a new app launch re-authenticates the token and reuses the
   saved grant, but the user can revoke it at any time.

Revocation, account sign-out, and app shutdown all take effect immediately and
close in-flight subscriptions. Changing the visible UI account never changes a
connection's account binding.

### Tool surface

Every account-scoped tool takes `account_id` (the account tab id, surfaced by
`list_accounts`). There is no default account and no fallback: an id that is
missing, signed out, or locked fails with `account_unavailable`.

| Tool | Kind | Arguments |
| --- | --- | --- |
| `list_accounts` | read | — |
| `list_rooms` | read | `account_id`, `limit?`, `cursor?` |
| `search_messages` | read | `account_id`, `query`, `room_ids?`, `limit?` |
| `get_attachment` | read | `account_id`, `event_id`, `attachment_index?` |
| `send_message` | write | `account_id`, `room_id`, `body?`, `reply_to_event_id?`, `attachments?`, `client_request_id` |
| `wait_for_events` | read | `account_id`, `cursor?`, `timeout_s?` |

`list_rooms` and `search_messages` return only rooms the connection may see.
A search hit carries the room id, the room's display label, the event id, and
the core's snippet. `SearchResultItem` has no sender, timestamp, or account
provenance field, so those are a known gap for a later DTO change rather than
something this bridge can invent.

### Search mapping

`SearchCommand::Query { request_id, query, scope, room_filter }` already
exists; the bridge maps `scope` to `AllRooms` (or `CurrentRoom` for a single
room) and intersects `room_filter` with the grant. Two limits are recorded
rather than faked:

- The command takes no sender or time-range filter. v1 exposes room scope and
  page size only; sender/time filtering waits for Core support, and the plan
  does not post-filter pages while claiming exactness.
- `SearchScope::CurrentSpace` is not used by the bridge because it depends on
  the selected Space, which is UI state.

### Send mapping

The bridge sends through `TimelineCommand::SendText` on the account's
`CoreConnection`:

- `transaction_id` is derived from the client's `client_request_id` (UUID) and
  reused verbatim on retry, so the SDK send queue drops a duplicate rather
  than posting twice.
- The bridge does not use the composer lease/submission protocol. That lease
  is webview-owned (`renderer_generation`, `lease_id`,
  `acquire_terminal_composer_permit`) and exists to arbitrate the visible
  composer; an external caller has no draft to arbitrate.
- A plain reply maps to `TimelineCommand::SendReply { request_id, key,
  transaction_id, in_reply_to_event_id, document }`. Thread replies
  (`m.thread`) are out of v1 until the thread send path is confirmed; the
  composer's thread surface is a separate command family.
- Attachments use the existing staged-upload path: stage the local file, run
  media preparation, then send prepared uploads. One `send_message` call waits
  for preparation and settlement and returns either the event id or a typed
  failure. Limits: 50 MB per file, 10 files, 100 MB per call.

### Attachments

- Sending takes absolute local paths; Core reads the file. `koushi-mcp` never
  reads message content or attachments itself.
- Receiving writes through the existing `media_save` policy into the app cache
  under `mcp/attachments/<connection>/<event_id>/` and returns the path plus
  mime type and size. There is no caller-chosen destination, so the bridge
  cannot be used to write outside that tree.

### Events and polling

- `wait_for_events(account_id, cursor?, timeout_s?)` long-polls the account's
  event stream and returns as soon as an event matching the grant arrives, or
  with an empty result at the timeout.
- The cursor is opaque, account-bound, and monotonically advancing. Omitting
  it starts at "now". A cursor from another account or connection is rejected
  with `not_permitted`; it can never replay another account's history.
- v1 event kinds: `new_message` (room, event id, sender, timestamp, body
  excerpt) and `room_invite`. Edits, redactions, receipts, and typing are out.
- `timeout_s` defaults to the connection's `poll.max_wait_secs` (25 s) and is
  clamped to 50 s so the call stays under common client tool timeouts.
- Polling cadence is user-configured: `SettingsValues.mcp.connections[]`
  carries `poll { enabled, interval_secs, max_wait_secs }`, the app shows it on
  the connection row, and `get_connection_status` returns it. Documentation
  ships a ready-made instruction snippet telling the assistant to call
  `wait_for_events` while the user asked it to watch.

The honest limit, recorded here: MCP cannot wake a host that is not calling
the server. A message is seen immediately only while a `wait_for_events` call
is open; otherwise it is seen at the next call. Anything stronger needs a
host-side poller or scheduler and is out of scope.

### Failure model

Closed error codes: `koushi_unavailable`, `unauthorized`, `consent_required`,
`not_permitted`, `account_unavailable`, `room_not_found`, `event_not_found`,
`attachment_too_large`, `unsupported`, `internal`.

- `initialize` and `tools/list` answer even when Koushi is down, so the tools
  remain visible. `tools/call` attempts one connection (2 s) and then fails
  with `koushi_unavailable`.
- The next call re-reads `endpoint.json`, so an app restart with a new token
  needs no client restart.
- A disconnect while a call is open fails that call; there is no resend.

### Security

- No message body, query text, attachment bytes, token, or Matrix credential
  in logs, events, snapshots, or QA artifacts; diagnostics carry counts and
  error codes only.
- Socket permissions `0600`, endpoint file `0600`, parent directory `0700`;
  peer-uid check on the socket as defense in depth.
- Matrix tokens, SDK store keys, and search index keys never cross the socket,
  exactly as they never cross the Tauri command boundary.
- Consent is enforced on the server for every call; tool descriptions and
  assistant instructions are never the gate.
- Attachment reads go through Core; attachment writes go through
  `media_save` containment. Neither path accepts a caller-chosen directory.

## Testing

- Unit: JSON-RPC framing in `koushi-mcp` (one message per line, stdout clean,
  logs on stderr), IPC framing, token comparison, cursor monotonicity and
  account binding, `client_request_id` reuse.
- Integration (headless, two accounts, one encrypted room, one DM): search and
  context retrieval; send idempotency by repeating the same
  `client_request_id`; attachment send and receive round trip; wrong-account
  and unauthorized-room requests; revoke mid-session; app-down behavior;
  concurrent Claude Code and Codex connections.
- QA: one scenario registered in `koushi-qa` with synthetic accounts, no real
  homeserver, no private data.
- Manual: `tools/list` visible in Claude Code 2.1.x and Codex 0.147.x, quit
  Koushi mid-call, restart and reconnect, confirm no token appears in `ps` or
  logs.

## Phasing

1. Endpoint file, socket, `hello`/consent, `list_accounts`, `list_rooms`,
   search and read tools (read-only).
2. `send_message`: text, reply, idempotency, then staged attachments.
3. `wait_for_events`, polling settings, connection status tool.
4. Client validation on Claude Code and Codex, setup documentation with
   working configurations, removal of the `koushi_unavailable` rough edges.

## Open questions

- Sender and time-range filters need a `SearchCommand` extension; decide
  whether that lands here or in a follow-up.
- Whether `TimelineCommand::SendText`/`SendReply` dedupe by `transaction_id`
  across app restarts, or only within one runtime, for the retry window the
  MCP contract promises.
- Whether thread replies get their own tool argument or stay out of scope.
- Where the consent surface lives (first-run dialog vs. Settings → Connections)
  and whether the first call or the connection itself triggers it.
- Whether a persistent grant should also pin the launcher binary path, so a
  replaced binary does not inherit an old grant.
