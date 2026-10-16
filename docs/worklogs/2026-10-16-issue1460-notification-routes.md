# Issue #1460: notification settings by route and destination

## What shipped

Account Settings → Notifications is split by who delivers: OS notifications on
this device (desktop toggle plus an opt-in, bounded message preview), destinations
this device sends to itself (Resend, SMTP, ServerChan Turbo for WeChat; a list and a
per-destination detail with credentials, targets, content, timing and outcome), and
the homeserver's own email pusher, labeled as server-dependent. The ServerChan
detail links to the SendKey page and refuses ServerChan³ keys by kind.

## Review findings and how they were resolved

An adversarial review of the first PR revision raised six points; all were fixed
before merge.

- **Renderer patch could restore a replaced binding (high).** A `SettingsPatch`
  from React carried `secret_binding`, so a stale form could write back the old
  revision - and with it the ServerChan consent for the previous SendKey.
  `NotificationDeliverySettings::apply_patch` now keeps the core's binding unless
  the patch is marked `delivery_bindings_from_core`, a `#[serde(skip)]` field only
  the core sets. Covered by
  `a_renderer_patch_cannot_restore_a_replaced_sendkey_binding_or_its_consent`.
- **Secret stored before the binding was persisted (high).** The save now unbinds
  on disk first, stores the value, then binds; each write is checked against what
  the settings store actually published, and a failure returns `StoreUnavailable`
  with the sink unconfigured. A failed command now republishes the snapshot before
  reporting, since the state can move before the failing step. Covered by
  `a_new_secret_is_not_stored_while_the_old_binding_cannot_be_retired`, which makes
  the settings directory read-only.
- **Removal did not await the clear (medium).** The section awaits the clear and
  keeps the destination when it fails; the clear also runs for a sink id the
  settings no longer hold.
- **Secret carried as `String` (medium).** The command carries `AuthSecret`, which
  zeroizes on drop.
- **Body source cut mid-grapheme (medium).** The 4000-char bound cuts on a grapheme
  boundary (combining marks and ZWJ sequences covered).
- **Preview one grapheme over the bound (low).** The ellipsis counts toward the
  120-grapheme bound.
