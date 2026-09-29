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

Koushi once chose between a `SyncService` (sliding sync) owner and a legacy
`/sync` owner through a bounded zero-timeline invite-list preflight that
distrusted the advertised MSC4186 version. Measured 2026-07-26, no local server
(conduit 0.10.12, tuwunel 1.7.1, the local Synapse lane) ever selected
`SyncService`, although conduit and tuwunel advertised
`org.matrix.simplified_msc3575`. The cause was field-name drift: MSC4186 renamed
the list filter `is_invite` to `is_invited`, ruma 0.24 still sent `is_invite`,
and both servers answered HTTP 200 without the list. Fixing the name alone would
have been wrong: conduit then selected `SyncService` and lost invites entirely
(`invites_dm` saw 0 invites), so the fallback's outcome had been right by
accident. The local Synapse lane could not evaluate sliding sync at all (MSC4186
was off in its config), and the probe's `reason` token was logged at Debug level
and never reached stderr.

Lessons: advertised support is not proof of behavior, and the deciding
diagnostic token must be visible in a single run (see
[verification.md](verification.md#minimize-human-round-trips)).

## Login timeout investigation (#334, #375)

`login A: timed out waiting for LoggedIn event` was long recorded as a Conduit
baseline; Tuwunel failed identically. The current symptom guide is in
[troubleshooting.md](troubleshooting.md#local-homeserver-core-qa).

- **#334 (2026-07-26).** A freshly registered primary A parks in the
  verification gate and `LoggedIn` stays held until promotion. Gate completion
  was a scenario allowlist, so every unlisted scenario could only time out. The
  shared login route now completes the gate unconditionally; scenarios that must
  not bootstrap own their login and return from `run_async` before that route.
- The failure recurred intermittently in 2026-07-30 CI. Naming the session phase
  in the timeout (`phase=…`) identified `phase=rechecking_trust` in one run.
- **#375** routed `AppEffect::CheckCurrentDeviceTrust` through both production
  effect lanes with an authoritative own-user `/keys/query`. Re-emitting the SDK
  subscriber's current value made one lane green but broke initial promotion;
  the shipped recheck settles on a real trust value, respects
  `trust_generation`, and releases the SDK Olm read guard before network I/O.
- Two later orderings (coalescing a recheck without retaining demand, and a
  mismatching ack preceding the recheck emission) are covered by the focused
  gates in troubleshooting.

## Browser-headless flake history

These were fixed by 2026-07-25 (a full 208-test serialized run passed). A
recurrence is a regression to investigate, not a known failure.

- The tier became a CI gate on 2026-07-25 after a stale IPC-argument assertion
  from #319 sat red on `main` until #323.
- Several `basic-operations` specs (reply-mode submit, pin/unpin) were flaky
  only in parallel runs: the harness `get_snapshot` returned a static snapshot
  that could reset composer or pin state mid-test, amplified by worker
  contention (see `playwright.config.ts` for the `workers: 1` rationale). A
  durable fix would make the harness `get_snapshot` response follow the reply
  lifecycle.
- A shell-landmark a11y spec was once listed as pre-existing failure and later
  passed unnoticed; do not re-add a known-failures entry without a fresh failing
  run.
- Three `timeline-scrollback` full-file flakes (fixed 2026-06-30) came from
  headless Chromium's unreliable native scroll/rAF delivery; the durable rules
  are in [troubleshooting.md](troubleshooting.md#browser-headless-harness).

## Rationale moved out of the rule books

- **Silent `SelectRoom` drop.** Dropping `SelectRoom` under a saturated
  `ACTION_QUEUE_CAPACITY` inbox caused the large-account "room selection did not
  complete" / blank-timeline / unloaded-members regression while every
  small-account headless lane passed (engineering rules "Async and Runtime" 6).
- **#116.** The blocker was three stacked silent no-ops (`handle_select_room`
  `Vec::new()`, an empty `build_state_delta`, then neither `StateDelta` nor
  `StateChanged`) behind one opaque 10 s timeout, invisible because every lane
  used small accounts (engineering rules "Async and Runtime" 7 and 10).
- **Wave 2 (#38, #39).** Parallel Phase A work collided on shared surfaces used
  as free-form append targets, which produced the hot-file and parallel
  protocol rules.
- **Local DMG signing (2026-09-15).** `build:dmg` reported ad-hoc signing but
  passed no identity to Tauri, so the app failed `codesign --verify --deep
  --strict`; the headless reproduction is `dmgSigning.test.ts`.
- **Harness seed re-emit (until 2026-07-30).** The boot loop's only exit was
  seed-row visibility, so a spec that replaced the timeline early had its rows
  overwritten by a late seed re-emit.
- **`complete_new_identity_gate_for_qa`** once returned without observing its
  confirmation outcome, so a failed confirmation looked like a stall after
  `gate_new_identity_bootstrap=ok`.
