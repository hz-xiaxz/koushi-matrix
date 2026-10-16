# #1468 - Notification settings: tabs, destination list, atomic save

Status: implementation plan. Canon: `docs/architecture/state-machine.md`,
"Local Notification Delivery" (the "Saving a destination" bullet). Follows
#1460.

## Problem

#1460 shipped the three routes on one stacked page, with these defects:

- the kind picker had stray, unstyled buttons;
- every field of a new destination expanded inline;
- a new destination was saved in two steps: the draft first, then the secret;
- an existing one was committed field by field on blur, so a half-configured
  destination could be saved;
- a ServerChan text mode needed a third step, because its consent names the
  SendKey revision, which exists only after the secret is stored.

## Design

- **UI**
  - **Tabs.** The Notifications page has three tabs: OS notifications, Sent
    from this device, and Homeserver notifications. The selected tab is
    ephemeral presentation state.
  - **List.** Sent from this device opens on the destination list. Each row
    shows the kind, the destination, a setup chip, and a summary. The add
    button opens a kind picker.
  - **Detail screen.** Choosing a kind, or a row, opens a configuration screen
    with Connection, Targets, Content, and Timing groups. Its sticky footer has
    Save and Discard. Test and Remove appear for a saved destination.
  - **Draft.** Edits are a React draft until Save. The secret stays in the
    DOM-owned input and is read only when the command is built.
- **Core**
  - `AppCommand::SaveNotificationSink { request_id, sink, secret, accept_content_consent }`.
  - The order is: validate, upsert the non-secret sink, then store the secret
    if one is given. If the secret store fails, an added sink is rolled back.
    Last, the core mints the consent if it was accepted.
  - A new failure kind, `SinkLimitReached`.
  - The old secret commands stay for removal and headless QA.

## Verification

- Core runtime tests in `crates/koushi-core/src/runtime/notification_delivery_tests.rs`.
  Each fails before the command exists.
- Redaction of the command's `Debug` in `crates/koushi-core-testkit/tests/command_redaction.rs`.
- Component tests for the tabs, list, picker, and detail screen.
