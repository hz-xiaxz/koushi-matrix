# #1460 - Notification routes: OS, this device, homeserver

Status: implementation plan. Canon: `docs/architecture/state-machine.md`,
"Local Notification Delivery", "Native Attention" and "Account Notification
Settings"; `docs/architecture/overview.md`, "Desktop Attention Surfaces";
`docs/policies/engineering-rules.md` rule 9 and the secrets rules. Supersedes
#1454, #1455, #1456, #1457, #1458. Related only: #1264, #1343, #1357, #1358.
Background: #1427 (device delivery), #994 (OS preview intent), #981/#1025.

## Problem

1. Device delivery settings are app-wide: one `enabled`, one trigger set, one
   timing, one prefix consent for one sink. The settings screen therefore
   cannot truthfully present per-destination targets, content, or timing.
2. A held room keeps the snapshot taken when it came due, and a retry resends
   the stored body, so an edit or redaction can leak obsolete text and counts
   go stale (#1454).
3. `next_wake_at` covers only the dwell and retries, so a batch held behind the
   minimum interval waits for an unrelated event (#1455).
4. The OS `message_previews` preference has no control and no effect: the
   banner body is counts-only even when it is true (#994 remainder).
5. No WeChat sink exists although the sink boundary was shaped for one.
6. The digest has no parent space name and the text source is capped at 240
   characters, so "entire message" cannot be offered truthfully.

## Element reference

- Element Web: Settings → Notifications has "Show message in desktop
  notification" (`notificationBodyEnabled`, device-level) separate from the
  account push rules and from the email pusher toggle ("Enable email
  notifications for <address>"), which is a `kind: email` pusher. Element
  never claims the server sends mail; it only registers the pusher.
- Element X: notification settings are account push rules plus an OS-level
  switch; there is no device-originated mail. Nothing to mirror for device
  delivery, which stays Koushi's own feature.

Koushi keeps Element's split (device preview vs. server rules) and adds a
third, explicitly named route: "Sent from this device".

## Design

### Settings schema (koushi-state, `SettingsValues.delivery`)

```text
NotificationDeliverySettings { sinks: Vec<NotificationSinkSettings> }
NotificationSinkSettings {
  id, destination, from_address, sender_name, config, secret_binding,
  targets: { direct_messages, mentions, keywords, watched: Vec<Target> },
  content: NamesOnly | Prefix { graphemes } | Full,
  content_consent: Option<{ destination, endpoint, recipient_binding, accepted_at_ms }>,
  timing: { dwell_secs, min_interval_secs, only_when_unfocused },
}
NotificationSinkConfig = Resend { endpoint } | Smtp { .. } | ServerChan { endpoint }
```

- No global or per-destination enable switch. A destination is *eligible*
  when its setup is complete (canonical fields present, credential present
  for the current binding) and it has at least one target. "Configured" in
  the UI is that setup state, never a delivery claim.
- `content` other than `NamesOnly` is honored only while `content_consent`
  matches the sink's current destination, endpoint and recipient binding.
  `canonicalize` downgrades `content` to `NamesOnly` and drops the consent the
  moment any of the three changes, on load and on every patch. For
  ServerChan the recipient is whoever owns the SendKey, so saving a new
  SendKey (or clearing it) clears that sink's consent and content.
- Defaults for a new destination: DMs and mentions on, keywords on, no
  watched targets, `NamesOnly`, dwell 300 s, minimum interval 900 s, only when
  inactive on.

### Migration (load-time, conservative)

A sink without a `targets` field is a pre-#1460 sink. It is rebuilt from the
legacy top-level fields:

- If the legacy channel `enabled` was false or the sink's own `enabled` was
  false, all its targets are OFF (it stays configured, sends nothing).
- Otherwise it copies the legacy triggers, watch list, dwell, interval and
  focus rule.
- Content is `Prefix { prefix_chars }` with a migrated consent only for the
  one sink the legacy consent named, still matching destination and endpoint;
  every other sink is `NamesOnly`. Nothing migrates to `Full`.
- The legacy top-level fields are read once and never written again.

### Content and provider limits

- The candidate carries the latest event's text up to
  `MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS = 4000` characters plus a
  `truncated_at_source` flag, and the room's parent space labels (first two).
- Per provider message-text budget, per room: Resend and SMTP 4000 grapheme
  clusters, ServerChan 1000. `Full` renders up to the budget; if the source
  was capped or the budget cut it, the room block is suffixed with
  `[truncated]` and the subject never says "full".
- Non-text, undecrypted, redacted or unavailable text falls back to names only
  for that room; the delivery still happens.
- Email: subject names the first room ("Mention in Room · Space") and
  "and N more rooms"; body has one block per room. ServerChan: `title` is the
  same single-line, names-only summary (the WeChat card shows only the title
  on the service-account channel), `desp` carries the blocks with Markdown
  punctuation escaped so message text cannot inject links or images.

### Channel (koushi-core)

The channel becomes one lane per sink: dwell tracker, held room ids, last
attempt, window sequence, attempts, in-flight and retry. Each observation
computes each lane's qualifying set from that sink's targets and focus rule.

- **#1454**: held entries store room ids; the payload is built from the
  current observation at send time. A retry rebuilds its body from the
  current observation of the rooms it covered: rooms that no longer qualify
  are dropped; if none remain the retry is cancelled; if the rebuilt body is
  byte-identical the idempotency key is reused, otherwise a new window key is
  minted (a possible duplicate notification is preferred to resending
  obsolete plaintext).
- **#1455**: `next_wake_at(now)` includes each lane's coalescing deadline
  while that lane holds rooms and has a credential, so the batch is released
  at the deadline and re-checked against the current projection.
- **Read elsewhere**: a receipt from another device makes the room's unread
  zero in the room list, the candidate disappears, and the lane drops the
  dwell, the held id and any retry coverage. Limits: a send already handed
  to the provider cannot be recalled; while this device is offline or has
  not synced the receipt it cannot know.
- Suppression unchanged: own messages, muted/low-priority/read rooms,
  ignored-user DMs, focus, and the dwell anchored on a live transition.

### ServerChan Turbo sink (koushi-notify)

`POST {endpoint}/{SendKey}.send`, `application/x-www-form-urlencoded`,
`title` single line, `desp` Markdown, default endpoint
`https://sctapi.ftqq.com`. The SendKey is validated (`SCT` prefix,
`[A-Za-z0-9]`, bounded; `sctp` refused) before storage. Classification:
HTTP 2xx with JSON `code == 0` → Accepted; 401/403 or a code naming the key
→ InvalidSecret; 429 or a quota/limit code → RateLimited; other non-zero
code → Rejected; 5xx/transport/timeout/unparseable → Transport. The URL,
the response text and the key never leave the sink. `Delivered` is renamed
in the UI as "Accepted by the service" for every sink: no sink proves the
user received the message.

### OS preview (#994)

`message_previews` gets a working switch under "OS notifications", OFF by
default. When ON, the banner body is the triggering event's text only when
the projection can verify the event: the live room whose unread count rose
in this observation, a latest event that is not redacted, not an edit or
reaction relation, and carries decrypted plain text. The text is flattened
to one line and bounded at 120 characters. Otherwise the count body is used.
Thread replies are allowed (they are the latest event); an edit falls back.
The payload stays `serde(skip)` and redacted in `Debug`.

### Homeserver section

`AccountNotificationsSnapshot.server_email_delivery: Unknown | KnownUnavailable`
is derived in `koushi-sdk` from the account's server name; `matrix.org` is the
only known-unavailable entry. The UI always explains that a server email
pusher cannot make mail arrive unless the homeserver sends mail, and shows
the matrix.org warning only for `KnownUnavailable`.

### UI

Account Settings → Notifications: three sections, "OS notifications",
"Sent from this device" (list → detail with back; add → same detail),
"Homeserver notifications". Header keeps the account switcher without
connection, unread and invitation indicators (#1458). Timing inputs show
"seconds" with the minutes equivalent and include the unit in the accessible
name (#1456). Secret inputs are write-only; React never keeps a saved
secret in state after submit (#1457).

## Tests

- RED first: held-content leak on edit/redaction (#1454), interval wake
  (#1455), read-elsewhere cancel, OS preview verified/unverified cases.
- Migration fixtures for every legacy shape; consent invalidation on
  destination, endpoint and SendKey change.
- ServerChan sink against a local `wiremock` stub; `sctp` refusal; secret
  never in URL logs or `Debug`.
- Headless QA tokens for per-destination delivery, the ServerChan stub and
  the interval wake.

## Out of scope

WeCom group robot, WxPusher, localization of wire text, durable outbox.
