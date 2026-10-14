# #1427 - Device-local notification delivery (mail and WeChat relay)

Status: implementation plan. Canon: `docs/architecture/state-machine.md`,
"Account Notification Settings" and "Local Notification Delivery";
`docs/architecture/overview.md`, "Desktop Attention Surfaces" and "Secrets and
Private Data"; `docs/policies/engineering-rules.md`, rule 9 and the secrets
rules; `REPOSITORY_RULES.md` (canon-first, RED-then-GREEN). Related: #981 and
#1025 (closed; the server-owned email surface this plan does not replace).

## Problem

The only email-notification path is the server-owned Matrix `kind: email`
pusher (`crates/koushi-sdk/src/account_notifications.rs`,
`crates/koushi-core/src/account/account_notifications.rs`). It needs a
homeserver that supports email 3PIDs and pushers, and matrix.org does not, so
the feature is unusable where it is wanted most. Delivery is a server-side
digest the user cannot inspect, and the settings surface can only report that
the server does not support it (#1025).

A desktop user who wants to be told when a DM, a mention, or one of their own
keywords arrives while the app runs has no option today. For a user whose
messaging life runs through WeChat, mail alone is also the wrong shape: the
notice has to be able to arrive where the messages do, which is why the channel
is defined over delivery sinks rather than over one mail provider.

## Product decisions (maintainer, 2026-10-14)

- **The app sends the mail.** The server-owned pusher surface stays exactly as
  the server state it reflects; it is not presented as the primary email path.
- **The provider is an app-wide setting.** Resend over its HTTPS API is first;
  a generic SMTP provider follows behind one sender boundary so Brevo, Mailjet,
  Amazon SES, or a own mailbox can be used without touching the pipeline.
- **Delivery is a sink list, not a mail feature.** Mail and a WeChat relay sit
  behind one boundary, share the candidate, the interval, the suppression and the
  prefix policy, and are enabled independently, because the practical Chinese
  relays (a WeCom group-robot webhook, or a third-party push service) are not mail
  and must not need a second pipeline.
- **The message must agree with the unread state.** The trigger is the persistent
  attention state the badge and the unread marks show, not the transient toast: a
  room is mailed only after its qualifying unread state has lasted the configured
  dwell, and the body is recomputed at send time, so reading, muting, or focusing the
  room before the dwell expires cancels the message.
- **The API key is user-supplied.** No bundled Koushi sender, no shared
  credentials, no server-side relay operated by this project.
- **The destination address is app-level** and independent of the Matrix
  account, because the address may not exist as a 3PID on the homeserver.
- **Triggers are DM, mention, and user keywords.** DM and mention reuse the
  existing attention authority (`RoomAttentionKind`); keywords are Matrix
  `kind: content` push rules so another Element-compatible client reads the
  same rules.
- **An optional plaintext message prefix** may be included, default OFF,
  enabled only through an explicit consent that is re-asked when the provider
  or the destination address changes.

## Constraints found in the current tree

- **Two settings scopes already exist.** `SettingsValues.notifications`
  (`crates/koushi-state/src/state/settings.rs:844`) is the device-local,
  app-persisted policy; `AppSettingsValues` / `AppNotificationSettings`
  (`:158`/`:177`) and `AccountSettingsValues` /
  `AccountNotificationSettings` (`:191`/`:202`) are the scoped projections, and
  `SettingsPatchScope {Account, App}` (`:1077`) routes a patch. Persistence is
  `settings/settings.json` at `SETTINGS_SCHEMA_VERSION = 3`
  (`crates/koushi-core/src/settings.rs:17`), with account-owned preferences in
  an encrypted per-account file (`crates/koushi-core/src/store/account_settings.rs`).
- **There is no generic named-secret API.** `save_raw` / `load_raw` /
  `delete_raw` are private (`crates/koushi-key/src/lib.rs:1138`/`:1144`/`:1150`);
  every public `CredentialStoreBackend` method is session-keyed, and
  non-session names are fixed constants (`:34-54`). Only the debug/test file
  backend exposes `save_named` / `load_named` / `delete_named`
  (`crates/koushi-store/src/credential_backend.rs:1010`/`:1016`/`:1026`). An
  email API key needs new typed public surface, not a generic escape hatch.
- **The banner payload carries counts, never content.** `NativeNotificationPayload`
  is `#[serde(skip)]` and redacted
  (`crates/koushi-state/src/state/native_attention.rs:47`/`:113`); the body
  builder is `native_notification_count_body` (`:420`). `message_previews`
  (`settings.rs:855`) is stored, mirrored into
  `NativeAttentionProjectionInput.message_previews` (`:188`) and fed from live
  settings (`crates/koushi-state/src/reducer/native_attention.rs:104`), but it is
  read by no projection code, while `docs/architecture/overview.md:1450-1464`
  describes preview text when it is ON. The prefix option therefore needs its
  own source and must not be built on that unused input; the drift is recorded
  as its own defect.
- **Dispatch is a candidate-scoped transient effect.** The desktop layer
  dispatches when `attention.notification` changes
  (`apps/desktop/src-tauri/src/lib.rs:406-411`), gated by
  `notifications.desktop_notifications`
  (`apps/desktop/src-tauri/src/commands/native_attention/notification.rs:148`);
  suppression and dedupe already live in
  `native_attention_suppression_reason` (`state/native_attention.rs:454`) and
  the dispatch reducer (`reducer/native_attention.rs:10`). Canon forbids adapter
  code from inventing dedupe or suppression, so the email leg is core-owned and
  the adapter stays transport-only.
- **No shared outbound HTTP client.** `reqwest` is built per call
  (`crates/koushi-sdk/src/auth.rs:229`), with rustls and no proxy or custom-CA
  handling anywhere; the SDK session client is injected at
  `crates/koushi-sdk/src/sliding_sync_discovery.rs:95`. The email sender must
  not become the first place that invents TLS or proxy policy.
- **Product text needs both locales.** The catalog is
  `apps/desktop/src/i18n/messages.ts` (`MessageId` union `:3`, `ja` `:3686`) and
  `apps/desktop/src/i18n/messages.test.ts` enforces equal ids and a complete
  Japanese catalog. Existing notification ids are around `:885-943`.
- **A new QA lane must be registered in three places**: the core registry
  (`crates/koushi-qa/src/bin/headless_core_qa/registry.rs`, tokens and
  `from_env_value` / `should_run_stage`), the token contract
  (`scripts/lib/qa-token-contract.mjs:27`) and the lane membership lists
  (`scripts/lib/headless-qa-lanes.mjs`, checked by
  `scripts/check-headless-qa-lanes.mjs`), plus a row in
  `docs/agents/qa-lanes.md`.

## Design

### Settings (app scope)

`AppNotificationSettings` (`settings.rs:177`) gains a `delivery` block, mirrored
in the flat `NotificationSettings` (`:844`) patch surface and the app branch of
`SettingsPatchScope` (`:1077`):

- a sink list, each entry with `kind` (`Resend`, `WeComWebhook`, `Smtp`),
  `enabled` (default false) and its own non-secret fields: mail addresses, the
  webhook URL, or the SMTP host, port, TLS mode and username;
- channel-wide `dwell_secs` (default 300; how long a qualifying unread state must
  last), `min_interval_secs` (default 900; the gap between two messages),
  `only_when_unfocused`, `prefix_chars` (0 = no prefix) and `prefix_consent`;
- per sink a `configured` flag for its secret, never the secret itself.

One owner per fact: the interval, the destinations, and the consent live here;
the trigger kinds stay projections of the server rules (`RoomAttentionKind`), and
the prefix length is bounded by a constant.

### Secret

New typed surface, not a generic one: a fixed key name per sink next to the
existing constants (`crates/koushi-key/src/lib.rs:34-54`), typed
`save`/`load`/`delete` wrappers on `CredentialStore` (`:962`/`:1021`/`:1026`),
and matching passthroughs on `CredentialStoreBackend` beside
`save_matrix_session` (`crates/koushi-store/src/credential_backend.rs:113`). The
value goes to the OS keychain in production (adapter only in
`apps/desktop/src-tauri/src/keyring_backend.rs`) and the encrypted file store
under QA, so `assert_file_credential_store_active` keeps holding. It is
read when the sink is configured or the delivery actor starts, never per
delivery, and every type that carries it uses a zeroizing wrapper with a
redacting `Debug`.

### Relay boundary and sinks

One `NotificationSink` boundary owns "turn one notification into one delivered
message": a sink-agnostic request (title, body, destination, idempotency key),
typed failure (`InvalidSecret`, `UnverifiedSender`, `Suspended`, `RateLimited`,
`Transport`, `Rejected`), and no policy. Three sinks prove it:

- `Resend` - `POST https://api.resend.com/emails` with the key in the
  `Authorization` header.
- `WeComWebhook` - **deferred, not built in this change.** The group-robot webhook
  with a text body, the webhook key being the secret, is the cheapest way for a
  desktop app to reach WeChat; a WeCom self-built app or a third-party relay
  (ServerChan, PushPlus, WxPusher) is the same sink shape with different fields. The
  boundary is shaped for it, but nothing offers it until someone needs it, and the
  canon records it as not implemented rather than as a sink that exists.
- `Smtp` - on a later phase, and the reason the boundary is worth having: Amazon
  SES, Brevo, Mailjet and Elastic Email outside China, and Aliyun DirectMail or
  Tencent Cloud SES inside it, all expose SMTP, so one sink covers the whole cheap
  end of the market.

Each sink is exercised against a local stub that asserts the request shape and the
error mapping (401, 422, 429, 5xx, timeout). Because the tree has no shared client,
a sink takes the client it is given and adds no TLS, proxy, or CA policy of its
own; proxy support belongs to shared outbound-client work, not here.

### Dwell, then digest

Delivery state is fed by the same per-room attention projection that drives the
badge, not by the toast candidate:

- a room enters the deliverable set on the transition into a qualifying enabled
  kind (`Dm`, `Mention` - which includes a keyword hit - and optionally `Message`),
  and the timer is anchored there, so a room that keeps receiving messages still
  comes due, while a room that goes read, is muted, is low priority, or is focused
  (under the focus option) before the dwell expires is cancelled and re-armed on the
  next transition;
- on expiry the core recomputes the digest from the current projection, so the body
  matches the badge at that moment: the rooms that came due, each with its safe
  display label, kind and counts, bounded to a few rooms plus an "and N more" tail,
  and no room, event, sender, or transaction identifiers;
- `min_interval_secs` coalesces mail after a send, and a retry of a transport or
  rate-limit failure honors the same interval;
- the dwell runs on the core executor's clock (`crate::executor::Instant`, the
  pattern the composer debounce already uses), so unit tests advance a paused tokio
  clock instead of sleeping and the headless lane configures a one-second dwell and
  waits in real time;
- the queue is in-memory and bounded and is dropped on exit, so a durable outbox
  remains out of scope for v1 and is stated as a limit, not implied;
- failures surface as a typed status per sink on the settings surface: an invalid
  key, a suspended account, or a rejected webhook must never read as ON, and a retry
  with bounded backoff is the only automatic reaction.

### Keywords

Keywords are written as Matrix `kind: content` push rules on explicit user
action, following the semantics already recorded in
`docs/qa/integration-edge-cases.md:198`: whole word, case-insensitive,
`override > content > room > sender > underride`, first match wins. Because the
SDK evaluates those rules into the highlight counter, a keyword hit ranks as a
`Mention`-kind candidate and therefore also raises the badge, exactly as it does
in Element. Opening the settings screen writes nothing; a rule that already
matches is not rewritten; a server that cannot write rules leaves the editor
read-only rather than pretending to save.

### Message prefix and consent

- The prefix is taken from the decrypted triggering message, trimmed to
  `prefix_chars` at a grapheme boundary, and added only when the option is on
  **and** a stored consent matches the current sink and destination.
- Consent is its own state, never inferred from the toggle. Changing a sink, its
  endpoint, or a destination clears it and the option turns itself off until
  re-consented.
- It is independent of the OS previews switch: enabling local previews never
  enables the prefix, because the toast stays on the device and the delivery
  leaves it.
- Consent copy is provider-agnostic policy, not a vendor datasheet: it states
  that the text leaves the device unencrypted, that whether and how long the
  configured service stores it depends on that service's own terms, and that the
  user must check those terms before enabling the option. The app asserts no
  provider-specific retention as fact, and links provider documents instead of
  paraphrasing them.

### Settings surface

Settings -> Notifications gains a "Notify me from this device" section: the sink
list, each with its own fields (from and to addresses for mail, the webhook URL
for the WeChat relay), a write-only secret field that only ever reports configured
or not, a test send, the interval, the prefix option behind the consent dialog,
the last outcome per sink, and a watch list of rooms and spaces picked from the room
list. A watched room is delivered even when its category is off, and a watched space
covers every joined room that is a child of it, resolved when the candidate set is
built so a room added later is covered. The list stores ids and the room list supplies
the label, so no room name is copied into settings. The server-owned group moves
behind a second tab in the same category, so which party sends a notification is
chosen by switching tabs rather than by reading a note about who owns what: the tab
labels name the sender, each tab carries one line saying what it controls, and a note
saying "everything below is sent by your homeserver" is not needed. The device tab is
selected first, and where there is no account the server tab is absent rather than
empty. The keyword list is edited in the device tab next to the switch that delivers
for it, and it is the account's own `content` rules read back from the homeserver
rather than a list copied into local settings, so a rule another client wrote is shown
and a failed write is reported instead of appearing saved; the copy says the keyword
itself is stored on the homeserver, which is what lets the app and the other devices
highlight it too, while the switch decides what this device mails about. React renders
the DTO and
dispatches typed commands; every string is a catalog entry in English and Japanese.

## Phases

1. Canon and record: this plan, the canon sections above, and the issue.
2. Secret surface with redaction tests, including "no secret reaches a settings
   snapshot or a diagnostics export".
3. Relay boundary plus the Resend sink and the stub-server contract tests. The
   boundary and the sinks are a new leaf crate, `crates/koushi-notify`, because
   `koushi-sdk` owns Matrix-facing primitives by the crate-ownership rule and
   `koushi-store` owns credential and encrypted-file mechanics, so a sender that is
   neither Matrix nor platform gets its own home; `koushi-core` depends on it and
   performs the send, which keeps a sink secret inside the core rather than
   serializing it into a DTO, an event, or a log.
4. The SMTP sink, because it is the no-hassle path: an existing mailbox with an
   app password needs no signup, no card and no domain, and a QQ mailbox can push
   its new-mail reminder into WeChat, which covers the WeChat case without a
   webhook. The WeCom-webhook sink joins here when a mailbox relay is not wanted.
5. Settings model, protocol patch, DTO, React section, catalog entries.
6. Dispatch integration, coalescing, and the headless lane.
7. The keyword editor, which is what makes the keyword trigger reachable at all:
   nothing in the app could write a `kind: content` push rule, so the trigger named
   a list the user had no way to fill. It rides the account notification surface,
   which already fetches the ruleset for the account's own push settings.
8. User guide, troubleshooting, lane row, and closing the issue once only manual
   checks remain.

### Keyword editor surface

The editor is a list of the account's `content` rules, in the server's own rule
order, each row a pattern with a remove control, plus one text field and an add
control. It rides the existing account notification surface rather than the delivery
projection, because the rules are server state the account screen already loads:
`load_account_notifications` already calls `fetch_push_ruleset`, so the list costs one
more projection field rather than a new fetch, and the write path reuses that
surface's operation state (`pending`, confirmed, failed) instead of inventing a
second one.

Frozen shapes, to be implemented as stated so the DTO mirror and the React side can be
written against them:

- `AccountNotificationsSnapshot` gains `keywords: Vec<NotificationKeywordRuleState>`
  ordered as the server orders the rules, each `{ rule_id, pattern }` - no action, no
  index, nothing else, because the rule's action is an implementation detail and the
  index is the order itself.
- `AppCommand::App(AddNotificationKeyword { request_id, pattern })` and
  `AppCommand::App(RemoveNotificationKeyword { request_id, rule_id })`, both carrying a
  request id so an admission settles the caller exactly as the other commands do.
- `normalize_notification_keyword` owns validation before anything leaves the client:
  trim, reject empty, reject control characters, reject over
  `MAX_NOTIFICATION_KEYWORD_CHARS` (64), and reject a pattern that matches an existing
  rule case-insensitively, because a duplicate would write a second rule that can never
  fire. The homeserver stays the authority for the write itself.
- `MAX_NOTIFICATION_KEYWORDS` (64) bounds the list: an add beyond it is refused with the
  same typed failure rather than silently trimming the newest rule.
- A remove names a rule id that exists in the loaded list; an unknown id is refused.

Adding and removing are the only writes, they happen only on explicit user action, and
opening the screen writes nothing. The copy sits with the controls: the keyword is
stored on the homeserver, which is why the app and the other devices highlight it too,
while the switch beside it decides what this device mails about.

### Recommended default setup

The plan optimizes for a user who does not want a subscription, a domain, or a
verification flow, so the documented default is a self-addressed notice: a mailbox
the user already owns as the SMTP sender and the destination (or Resend's
`onboarding@resend.dev` sender to the account's own address on the free plan). That
mode needs no DNS records, no production-access request, and no billing, and it is
the configuration the user guide leads with; a verified domain stays an optional
upgrade for deliverability, not a prerequisite.

Within that mode the vendors to document are, in order of setup cost and vendor
stability: an existing Apple iCloud or Google mailbox over SMTP with an app
password (no signup, no card, no domain), then a company-backed API account that
authenticates a single sender address rather than a domain - Twilio SendGrid,
Mailjet (Sinch), or Brevo - with Resend kept as the first implementation. A new API
provider is one more sink, not a new pipeline.

The initial senders are therefore iCloud Mail, Gmail, Resend, and QQ Mail: the first
three outside China, and QQ Mail as the China route, because its own new-mail reminder
can surface the message in WeChat and the mail sink then covers both destinations.
Each of the four is a preset over the same two implementations - a REST sink and an
SMTP sink - so a preset fills host, port, TLS mode and the secret field name and adds
the one setup step it needs (an app password, an app-specific password, an API key, or
QQ Mail's authorization code plus its WeChat reminder). The WeCom group-robot webhook
stays the next sink behind the same boundary, for a user who wants a direct relay
rather than a mailbox reminder.

## Verification

- RED first: a headless scenario that fails before the feature exists - a DM, a
  mention and a keyword hit each queue exactly one delivery once the configured
  dwell has elapsed, while a room read (or focused) inside the dwell, a muted room,
  a channel with the prefix off, and an own message queue none, and a second
  delivery inside the minimum interval is held. A watched room with its category
  off queues one, and a room that joins a watched space after the list was written
  queues one.
- Dwell: unit tests on a paused tokio clock prove the timer anchors on the
  transition into the qualifying state, survives further messages in the same room,
  and re-arms after it is cancelled.
- Secret hygiene: no sink secret is present in settings snapshots, diagnostics
  exports, logs or QA artifacts; the QA run stays on the file credential store and
  never prompts for the keychain.
- Contract: each stub asserts the request shape (authorization header or webhook
  key, from, to, subject, body) and the error mapping (401, 422, 429, 5xx,
  timeout).
- Consent: the prefix is dropped when the consent is missing or was granted for
  another sink, endpoint, or destination.
- Docs: `node scripts/check-agents-docs.mjs`, the lane checker, the catalog
  tests, `cargo fmt --check`, `cargo clippy -D warnings`, and the frontend
  typecheck and tests.
- Manual only: one real Resend message to the maintainer's own inbox, and one real
  WeChat message through a WeCom group robot.

## Out of scope

- A bundled or shared Koushi sender.
- A durable outbox that survives a restart.
- A WeChat official-account template-message sink, which needs a certified
  account, and anything that would make the app hold a WeChat or mail account
  password: a relay takes a webhook URL or a send key, never an account credential.
- Rewriting or mirroring the server push-rule set, or changing server state when
  a settings screen opens.
- Proxy, custom-CA, or pinned-TLS support for the new sender; that belongs to
  shared outbound-client work.
