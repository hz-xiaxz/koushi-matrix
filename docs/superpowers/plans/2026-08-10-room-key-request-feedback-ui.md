# Room-Key Request Progress and Refusal Feedback UI — Implementation Plan (issue #460)

> **For agentic workers:** Implement verify-first: add the failing headless check
> before the fix and let the same check turn green.

**Goal:** GUI phase over the Rust-owned requester lifecycle (#459): clicking
"Request keys and retry" on an undecryptable message produces immediate visible
acknowledgment (toast + pending state), a waiting state with non-terminal
offline guidance, localized refusal copy per typed withheld code, and success
copy on key receipt/decryption recovery. React renders Rust-owned state only.

**Architecture:** Rust (timeline actor) owns the requester lifecycle
(#459/#466 decrypt-retry flow) and **publishes a dedicated typed event** on
every request-state transition. React keeps a per-event presentation map and
renders it in the message row + toast; it dispatches the existing typed command
and never infers Matrix semantics.

## Rust changes

1. **koushi-sdk**: expose `room_key_withheld_stream(session)` — a doc-hidden
   wrapper for the SDK crypto store's `room_keys_withheld_received_stream`,
   mapped immediately to app-owned closed tokens `(room_id, session_id, code)`
   with a redacted type (no `Debug` of raw SDK info; never exported as-is).
   The SDK stores standard `m.room_key.withheld` events for
   `blacklisted | unverified | unauthorised | unavailable` only
   (`machine/mod.rs add_withheld_info`); `no_olm` and `history_not_shared`
   are not correlatable from this stream and are documented as such (they fall
   back to the generic copy). The timeline UTD cause adds
   `unverified` (`WithheldForUnverifiedOrInsecureDevice`) and `custom`
   (`WithheldBySender`).
2. **Timeline actor**:
   - `key_request_states: BTreeMap<event_id, KeyRequestUiState>` with
     `{ stage: &'static str, withheld_code: Option<&'static str> }`, keyed
     internally by event id (never exported raw).
   - A `withheld_sessions: BTreeMap<(room, session), closed code>` map fed by
     the withheld observer task (correlated by room + session).
   - Publish `RoomKeyRequestStateChanged { event_id, stage, withheld_code }`
     as an `AppAction` on every transition: `sent`/`awaiting` (after the device
     request is queued; duplicate clicks are coalesced by the existing
     `DecryptRetryController::admit`), `still_waiting` (presentation deadline),
     `withheld` (code from the observer), `decryption_recovered` (decrypted
     settlement **or** a later decrypted diff for a session that was still
     waiting), `send_failed`. Background/automatic thread requests do not
     publish a `sent` toast (only the user-triggered path does).
   - Presentation state survives the operational timeout: the controller's
     single-pending slot is operational (kept as-is); a late decrypted diff
     still transitions the presentation state to `decryption_recovered`.
3. **DTO/event wire**: `RoomKeyRequestStateChanged` carries only
   `{ event_id, stage, withheld_code }` (closed tokens). No identifiers, key
   material, or raw reasons cross the WebView.

## Frontend

- A per-event request-state map in React, updated by the Rust event:
  - `sent`/`awaiting` → pending marker + waiting copy; the first `sent` for a
    user-triggered request shows the toast "Decryption key requested".
  - `still_waiting` → "まだ応答がありません。別の端末がオフラインの場合、後で復号されることがあります。" (non-terminal; request stays pending).
  - `withheld` → localized copy per the issue's table for the closed codes we
    can correlate: `unavailable`, `unauthorised`, `unverified`, `blacklisted`;
    `no_olm`/`history_not_shared`/`custom` use the generic
    "復号鍵を取得できませんでした。" (documented: no correlatable source).
    The historical-specific unauthorised copy is dropped (no typed historical
    fact for an unauthorised withheld event in the SDK — documented).
  - `decryption_recovered` → "復号鍵を受信しました" and clears the pending marker.
- A minimal toast component (existing UI primitives, ARIA live region).
- Raw remote `reason` strings are never rendered; copy comes from i18n keys
  keyed by the closed tokens only.
- Toast suppression on initial hydration and on background/automatic requests.

## Tests (mapped 1:1 to the issue's 11)

1. Click → one typed command + toast + pending state.
2. Repeated clicks while pending → no duplicate commands.
3. `still_waiting` → non-terminal offline guidance, request stays pending.
4. Each correlatable withheld outcome renders the correct localized copy.
5. (historical unauthorised copy is dropped by design — documented) generic
   unauthorised copy is asserted.
6. Raw remote reason strings are never rendered.
7. `decryption_recovered` clears pending and updates the message.
8. Late key after `still_waiting` still settles `decryption_recovered`.
9. Room/timeline/account switching does not apply stale outcomes (event_id
   keyed; per-timeline actor state).
10. Browser-headless test: keyboard activation + ARIA live region/toast.
11. Privacy: DTO/tests contain no IDs/key material/raw errors.

## Gates

Full local matrix (cargo test koushi-core/sdk/desktop/state, frontend vitest,
typecheck, lint, fmt, diff check) + gpt-review design before implementation and
diff after implementation + CI green + non-squash merge + issue closed.
