# Account-owned notification settings

User decision: all notification policy and external destinations belong to one account.
Canon: overview Desktop Attention Surfaces; state-machine Local Notification Delivery;
REPOSITORY_RULES Review And Audit; state-ownership snapshot/wire contracts.

Design reviewed independently before implementation: approved with migration, persistence,
credential identity, and account-local attention-clear constraints (review in this chat).

Implementation: move notification policy and delivery from app projection to encrypted
account projection; key credentials by complete SessionKeyId and sink ID; require loaded
account settings and authenticated session for delivery commands; remove app editor.
Keep legacy global document inert in schema4. Upgrade encrypted account file v1->v2
atomically, copying only shared sound/badge values once and no destinations/credentials.
Preserve the existing Matrix-ID menu edits. Verify failing-then-passing ownership tests,
credential isolation, migration/retry and save/readback tests, UI placement, typecheck and
focused runtime tests. UI/transport JSON remains the same projected SettingsValues shape.

Upstream inspection: Element X Android NotificationSettingsPresenter binds user push
preferences to matrixClient.sessionId and reads rules from NotificationSettingsService.
Element X iOS NotificationSettingsScreenViewModel combines an AppSettings notification
switch and a session NotificationSettingsProxy. We intentionally make every Koushi
notification preference account-owned, per the user's explicit multi-account decision.
A matching upstream user-notification-settings component was not found in the available
Element Web checkout.
No upstream code was copied and standard server push-rule APIs remain unchanged.

Integrated independent review: initial blocking concurrency/migration/cleanup findings
were fixed; final verdict APPROVED, no remaining P1/P2 blockers. Invalidation epochs and
watch signals cannot downgrade immediate privacy clearing to routine grace. Rust watches
every account, including background tabs and removal. External credential cleanup precedes
removal of persisted identity or the encrypted sink-ID index.

Verification completed:
- Ownership regressions failed before the implementation and pass afterward.
- State settings/notification suites; Core notification (113) and settings (16);
  account ownership integration (5) and runtime settings (20); native attention (14).
- Frontend full suite: 173 files, 2,129 tests; final settings panel rerun: 38 tests.
- Chromium account settings/help/multi-account scenarios: 19 passed.
- Rust Clippy for Core/Desktop all targets with warnings denied; cargo fmt --check;
  TypeScript typecheck; ESLint, IME, semantic-owner, Tauri contract, domain/leaf boundary,
  Rust test-structure, AGENTS/help documentation checks; git diff --check.
- npm lockfile audit: zero vulnerabilities.

The legacy shared destinations are archived and inactive; users must register them
for the intended account again. This change has not been released or installed.

Merge preparation: integrated current main, preserving its account-tab and MCP
assistant settings additions; independent follow-up review approved. Updated the
existing browser and website screenshot routes to use account notifications.
The feature-unified CI-profile workspace suite, workspace/QA/release Clippy,
frontend suite (2,162), and both-server local QA pass. An initial workspace run
hit an existing MCP registration deadline; the unchanged exact test and complete
workspace rerun pass. Final required CI is checked on the PR before merge.

The website lane exposed missing delivery runtime-state forwarding in the account
panel. A production-wiring browser test reproduced `needsCredential` for an
explicitly configured Rust-shaped fixture, then passed after forwarding the same
account snapshot's notification_delivery field. Exact screenshot status assertions
replace an English partial-text match. Eight focused browser cases, the English
screenshot, full UI suite, typecheck, and lint pass; independent review approved.
