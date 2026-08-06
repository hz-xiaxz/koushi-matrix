# Historical Records

Investigation records and superseded contracts, kept because they explain why
current rules exist and because old plans, issues, and transcripts still refer to
them.

**Nothing in this file is runnable.** Commands and flags quoted here are
preserved as artifacts. For anything you intend to execute, use
[qa-lanes.md](qa-lanes.md).

## Retired QA vocabulary

Issue #412 settled the runtime on one Element X-compatible Simplified Sliding
Sync engine, and #417 removed the Legacy Sync wire states. Anything below is
rejected by the current runners. If you find one in `docs/superpowers/plans/`,
`docs/qa/`, an issue, or an older transcript, it is an artifact — do not run it.

| Retired | Status | Use instead |
| --- | --- | --- |
| `conduit` | No lane supports it; `selectedServers()` throws and the Linux GUI image no longer installs it | `--server=tuwunel`, or `--server=synapse` for headless lanes |
| `--core-backend=legacy\|probed\|both` | The runner exits with "`--core-backend` is obsolete: the production runtime has one Sliding Sync engine" | omit the flag |
| `KOUSHI_QA_FORCE_SYNC_BACKEND` | Backend forcing removed | nothing; there is one engine |
| `--scenario=timeline_legacy_fallback` | Scenario retired; the runner rejects `timeline_legacy_*` | `--scenario=timeline_reconnect` for live catch-up |
| `legacy_fallback_*` tokens | Emitted by the retired scenario | `live_catchup_checkpoint=ok`, `live_catchup_gap_repaired=ok` |
| `Core homeserver QA (conduit)` CI job | Replaced | `Core invitations (tuwunel)` / `Core invitations (synapse)`, plus `Core QA binary tests` |

## Sync backend selection and the sliding-sync probe (superseded)

This whole mechanism is gone. It is recorded because it is the clearest example
in this repository of a test that returned the right answer for the wrong reason,
and because the measurement is the reason "advertised support" is never trusted
here.

Koushi used to pick between a `SyncService` (sliding sync) owner and a legacy
`/sync` owner. Selection did not trust the advertised MSC4186 version: before
either continuous owner started, a bounded authenticated zero-timeline
invite-list contract preflight ran, and only the presence of the requested list
selected `SyncService`. Omission, error, malformed response, or a two-second
end-to-end deadline selected `LegacySync`. The disposable authenticated probe
received no refresh token, retries were disabled, and cursor/room payload was
discarded.

Measured 2026-07-26, no local server ever selected `SyncService`:

| server | advertises | Koushi selects |
| --- | --- | --- |
| conduit 0.10.12 | yes | LegacySync |
| tuwunel 1.7.1 | yes | LegacySync |
| Synapse v1.151.0 (local lane) | **no** | LegacySync |

All of them advertised `org.matrix.simplified_msc3575` in
`/_matrix/client/versions`. The direct cause was a field-name drift, not a server
gap: MSC4186 renamed the invited-rooms list filter `is_invite` to `is_invited`,
and ruma 0.24 still modeled it as `is_invite` with no serde rename, so the probe
sent a field the current MSC does not define. Both Conduit and Tuwunel answered
HTTP 200 but omitted the list, and the probe read that omission as
`KnownIncomplete` and fell back:

| request | conduit | tuwunel |
| --- | --- | --- |
| `is_invite` (what Koushi sent) | `lists` absent | `lists` absent |
| `is_invited` (MSC4186) | `lists` present | `lists` present |
| no filters | `lists` present | `lists` present |

Response latency was ~5ms in all cases, so the deadline was never involved.

The trap: fixing the field name alone would have been wrong. Removing the filter
flipped conduit to `SyncService`, and the `invites_dm` stage then failed with
`have 0 invites`, `observer_diag_base_invite_update_seen=false`, and
`observer_diag_invite_projection=0`. Conduit's sliding sync did not deliver
invites at all, so the fallback's *outcome* was correct even though its
*mechanism* was accidental. An echo-based probe could not have detected this,
because conduit would echo the list and still lose invites.

Two further notes from that investigation:

- The local Synapse lane could not evaluate sliding sync at all: its config set
  only `experimental_features: msc3266_enabled`, so MSC4186 was off and stage one
  failed regardless of the filter name. A lane asserting
  `sync_backend_a=SyncService` was proposed and never built.
- The probe's own decision was invisible in QA output: `trace_sync!` recorded
  `probe_done` with the `reason` token at `DiagnosticLevel::Debug`, which never
  reached stderr, so diagnosing the fallback required replicating the request
  outside the app. Raising that to a stderr-visible level would have turned a day
  of investigation into one log line. That is why current diagnostics put the
  deciding token where a single run can show it.

Behavioral coverage at the time proved success, omission, malformed/error, and
timeout, plus that `M_UNKNOWN_TOKEN` caused zero refresh calls, no authoritative
session-change/token mutation, and fail-closed selection.

## Login timeout investigation (#334, #375)

`login A: timed out waiting for LoggedIn event` was recorded for a long time as a
Conduit baseline. It never was one — Tuwunel failed identically. The actionable
残り is in [troubleshooting.md](troubleshooting.md#local-homeserver-core-qa).

- **#334 (2026-07-26).** Primary A is always a freshly registered user, so it
  parks in the verification gate, and `AccountEvent::LoggedIn` is held in the
  actor's pending-ready events until the trust transition promotes the session.
  Gate completion was an allowlist of scenarios, so every scenario missing from it
  — `media`, `login_sync`, `timeline`, `reply`, and most of the focused tier —
  could only ever time out at `login A`. The shared login route now completes the
  gate unconditionally; scenarios that must not bootstrap own their login and
  return from `run_async` before that route.
- **#334 did not end the failure mode.** The same timeout recurred on `main` in
  2026-07-30 CI: three red runs on one branch, one on another, while a third
  branch passed the identical job — intermittent, and green locally on the same
  commits.
- The fix that found it was a diagnostic, not a guess: naming the session phase
  in the timeout message (`phase=…`) identified `phase=rechecking_trust` in ONE
  run. Before that, the message was identical for every hypothesis and diagnosing
  it took a full CI round trip.
- #375 then routed `AppEffect::CheckCurrentDeviceTrust` through both production
  effect lanes and performed an authoritative own-user `/keys/query`. The first
  candidate was not a safe one-line fix: re-emitting the SDK subscriber's current
  value made one lane green 3/3 but broke initial promotion. The shipped recheck
  settles on a real trust value, respects `trust_generation`, and releases the
  vendored SDK Olm read guard before network I/O.
- Two further orderings were found after that: coalescing an explicit reducer
  recheck without retaining demand, and reducing a Ready projection while the
  reducer was still gated so a mismatching ack preceded the recheck emission.
  Both are now covered by the focused gates listed in troubleshooting.

## Browser-headless flake history

All of the entries below passed in a full 208-test serialized run on 2026-07-25.
They are explanations of fixed failure modes, not currently-expected failures — if
one goes red again it is a regression to investigate, not a known issue to
tolerate.

- The browser-headless tier became a CI gate on 2026-07-25. Before that it ran
  only when someone remembered, and a stale IPC-argument assertion introduced by
  #319 sat red on `main` until #323 found it by hand.
- `e2e/basic-operations.spec.ts:81` ("submitting the composer in reply mode
  invokes send_reply, not send_text") was flaky in the full run but passed
  reliably in isolation. Root cause was a test-layer timing race, not a product
  bug: the App's snapshot refresh (`get_snapshot`) returns the harness's static
  Plain `readySnapshot`, which can land after the reply-target click and
  momentarily reset the composer mode to Plain so the submit dispatches
  `send_text`. It reproduced on a clean checkout and was amplified by
  parallel-file worker contention. A durable fix should make the harness
  `get_snapshot` response consistent with the reply lifecycle. The
  `reply send does not repair product state by cancelling reply mode` regression
  added in that remediation passes deterministically in isolation.
- `e2e/desktop-shell-a11y.spec.ts:18` ("the three-pane shell exposes landmarks
  and reachable keyboard focus stops") was recorded as a PRE-EXISTING failure: the
  `complementary` landmark named "Context panel" was not found at the default `/`
  harness state, and it failed identically on a clean pre-#77-83 checkout. It
  passes as of 2026-07-25, so the gap was fixed without the note being updated —
  exactly the drift the CI gate now prevents. Do not re-add it to a
  known-failures list without a fresh failing run.
- `e2e/basic-operations.spec.ts:959` ("Explore searches public rooms and joins
  only after Rust snapshot updates") failed once during #328 verification: the
  `Join this room?` preview dialog was never found. The same commit passed a full
  serialized re-run (210/210), passed in isolation, and passed with the only
  changed frontend file reverted. One occurrence is a re-check, not a known
  failure.
- `e2e/basic-operations.spec.ts:2811` ("pin and unpin actions render the Tauri
  snapshot response without a manual state event") was flaky in the full parallel
  run and deterministic in isolation — same shared-harness snapshot-timing class
  as the reply spec.
- `e2e/timeline-scrollback.spec.ts` full-file runs had three flaky failures
  ("scrollback prepend keeps the anchor...", "active scroll inside mounted
  overscan...", "timeline navigation renders Rust-owned unread controls...") that
  were NOT a product bug and NOT the #158 Task 7 media work: they reproduced
  identically at the Task 7 base commit, and that diff only touched
  `TimelineMediaAttachment`. Root cause was test fidelity around headless
  Chromium's unreliable native scroll/rAF delivery. Fixed 2026-06-30; the durable
  rules it produced are in
  [troubleshooting.md](troubleshooting.md#browser-headless-harness).
