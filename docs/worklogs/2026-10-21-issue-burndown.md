# 2026-10-21 issue burndown

This branch addresses #1570, #1569, #1568, #1565, #1560, #1556,
#1551, #1550, #1543, #1534, and #1519. #1571 records the Phase A
Space-topic prerequisite separately, as required by #1565.

## Ownership and contracts

- Matrix topic values and event-specific permissions remain SDK/Core owned.
  Empty topic state normalizes to `None`; nonempty multiline text is preserved.
  The existing selected-settings observer reads current SDK state on its existing
  wakeup, with account-epoch and selected-target guards. No extra Matrix session.
- MCP Recent reads the existing Core Activity projection without opening it or
  starting its history resolver. Private replies, bounded immutable cursors,
  account/runtime/session/query/grant checks, unknown read state, and coverage
  limits are documented in overview/state-machine and assistant help.
- Navigation diagnostics have their own bounded ledger and loss counters.
  Exported targets are numeric aliases from a bounded private map; no identity
  hashes, names, messages, query text, paths, or credentials are exported.
  Operation spans and authoritative reducer publications are distinguished.
- Current-token access is a narrow documented exception to the secret boundary:
  explicit reveal/copy only, private zeroizing reply, authoritative SDK identity,
  Ready admission and final session revision validation, fixed failures, no
  AppState token/event/log/persistence, transient DOM disclosure. A process-unique
  nonsecret disclosure epoch invalidates UI scope across runtime replacement.
- Timestamp focus is an explicit exception to the default pointer-only tooltip
  policy. The existing document history owns editor undo/redo, including native
  Edit menu routing; IME composition remains protected.
- Local Synapse fixture generation/run uses the host POSIX UID/GID without
  nonroot UID/GID environment variables that trigger upstream chown/gosu.
  Existing fixture trees are neither repaired nor broadly chmod/chowned.

## Upstream references and divergences

- Element Web timestamp view-model/view and Element X iOS `Date.swift` /
  Android `DateFormatterTime.kt` informed locale-aware dates. Koushi also shows
  compact dates inline and the full date/time on keyboard focus.
- Element Web `SpaceSettingsGeneralTab.tsx` checks `m.room.topic` permission
  and calls `setRoomTopic`; `SpaceBasicSettings.tsx` uses multiline Description.
  Element X Android TopicViewer and iOS SpaceHeaderTopicSheetView display the
  shared topic. Koushi places its editor and feedback in one Space Info card.
- Element-style token reveal/copy uses the current authenticated SDK token;
  no serialized session, refresh token, recovery key, or device provisioning.

## RED and GREEN evidence

Targeted failures preceded implementation: document history shortcuts in the
browser; missing date/ID display; wrong Synapse fixture ownership; missing
Recent discovery and invalid-limit handling; missing direct Core APIs; private
token scope/admission; missing remote Space-topic updates and empty normalization;
and unknown-sender encrypted in-room verification requests before device keys.

Verified during integration (final gates are recorded in the PR):

- Native Linux bind ownership: 18 tests, none skipped, including editable/removable
  retained files after stop, forced termination, startup failure, and root UID.
- Core full library initially 1,727 passed; final isolated gates include the
  disclosure, direct-history, account-fallback and settings residency regressions.
- Frontend full Vitest including Space UI: 181 files / 2,249 tests passed,
  with typecheck, lint and production build also passing. The final expanded
  Chromium/WebKit DOM suite passed all 645 tests.
- Final isolated affected Rust library suites: 2,516 passed / 13 ignored, with
  all-targets Clippy warnings denied. QA binary tests: 125 passed, and its
  separate all-targets Clippy gate passed. The state/search/protocol WASM
  target check also passed.
- Independent crypto review: final verification suite including QR, 117 passed;
  SDK E2EE 79 passed; genuine late-UTD recovery delivers exactly one typed request.
- Timestamp focused tests cover DST, midnight, locale and keyboard disclosure;
  editor keyboard browser tests passed Chromium and WebKit.
- Vite audit found an existing High advisory; updated to 8.1.6 and audit reports
  zero vulnerabilities before the frontend build.

The historical #1534 failure is not claimed to have the same proven cause as
our synthetic unknown-device regression. Local user-verification checks and the
new deterministic race/scope/cancellation tests validate the repaired paths.
No real account or external homeserver is used for reproductions.

## Reviews and boundaries

Independent design reviews approved token access, MCP Recent/navigation, and
Space-topic actor ownership before integration. Crypto integrated review found
and resolved initial lookup/replay races, task cancellation, room-scoped flow
checks, and capacity handling. Token integration review found and resolved the
settings-scope closure gap and per-runtime epoch collision.

Packaged macOS/Windows/Linux native-menu checks remain manual platform checks;
browser keyboard tests and Rust menu contract tests are automated. A clipboard
copy intentionally survives closing the disclosure, as documented in help.

Additional pre-UI verification used a dedicated Cargo target directory after
stale dependency/binary mismatches appeared while a parallel checkout build was
active. Final Rust results use `/tmp/koushi-issue-burndown-target` with the compiler
wrapper disabled for source coherence, without altering user configuration.

The settings-demand regression also proves that backend generation 42 is
forwarded even when the local room-list projection counter is 1. The counters
serve different owners. Real settings demand shares existing residency and
never resets a retained room subscription; ten focused residency tests passed.
The Topic write path subscribes to the existing SDK update stream before the
HTTP request and waits for the exact acknowledged event to become canonical.
Eleven focused tests cover acknowledgement-only failure, wrong-room/event
isolation, repeated set/clear with immediate reopening, remote supersession,
early arrival, cancellation, bounded proofs, lag and canonical batch ordering.
Timeline state alone never confirms a write. The structural gate follows this
delegation from the public update method to the SDK write.

Strict disposable Tuwunel runs on both 1.7.1 and 1.9.1 exposed a second problem:
the server accepted a Topic write but omitted its requested canonical state from
an incremental sync response that contained the event in its timeline. Local SDK
state therefore remained stale for writers and viewers. Private wire checks
confirmed the requested selectors and the authoritative server event ID; no
message content or identities are recorded here. Upgrading alone did not fix it.

The SDK repair runs inside its existing uncancellable sync-response owner, while
the position guard is held and before the global store lock. Only a requested,
missing empty-key Topic or PowerLevels event signaled by that response's timeline
qualifies. The same authenticated client fetches a full current state event;
validated results enter ordinary required-state processing. Provided canonical
state is never replaced and timeline contents are never promoted. Each response
allows at most eight requests, concurrency two and a two-second total budget,
with explicit room subscriptions prioritized. Failures preserve ordinary sync
processing; they never invent write confirmation. No extra session, sync loop,
cache or Core network owner is introduced.

Nine focused SDK repair regressions passed, including topic-only permission
changes, invalid full events, failure/timeout preservation, unique prioritized
admission, nonjoined-room exclusion, no authentication refresh, and a real sync
caller cancellation that proves position ordering and a free global store lock.
The metadata ingress and final Topic proof stream received independent review.

The final disposable Synapse SDK and Core room-management run passed, including
both Space-topic contracts. Subsequent version-specific Tuwunel results are
recorded in the PR validation alongside this worklog.

The Space QA scenario keeps immediate writes, SDK metadata observations and
reopening checks. Its authoritative server assertion runs once at the very end,
so an extra GET between writes cannot mask the synchronization race.

Final independent reviews approved crypto source/integration, token UI and
backend fences, MCP delivery/cursor/filter isolation, settings residency, and all
account-selection diagnostic paths. The account observer stores only private
metadata; its regression consumes the exact emitted numeric record through a
test-only sink so unrelated runtime resets cannot erase test evidence.
