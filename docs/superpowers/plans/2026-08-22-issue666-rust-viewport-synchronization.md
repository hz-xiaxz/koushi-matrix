# Issue #666 Rust-owned viewport synchronization

## Problem and evidence

The live desktop shell can stop matching the visible macOS window after a
presentation/layout transition. Display density is the reliable reproducer, but
native resize, scale-factor, fullscreen, and panel/layout transitions must keep
the same root-bound invariant.

The current boundary cannot distinguish the leading layers:

- React changes density only through local state and local storage. It does not
  call a native resize API.
- `.desktop` uses `height: 100vh`, even though `html`, `body`, and `#root`
  already define an inherited `height: 100%` chain.
- Wry 0.55.1 gives the root WKWebView a flexible width/height autoresizing mask.
  Tauri Runtime Wry 2.11.4 then reports macOS `inner_size` from that same
  WKWebView frame, not independently from the NSWindow content view.
- Therefore a stale WKWebView frame can become a circular source of truth: the
  adapter reports the stale frame as the inner size and has no independent
  parent-bounds comparison that can detect or repair it.
- `titleBarStyle: Overlay` uses a full-size macOS content view, the path where
  native content and WKWebView frames must remain exact. Restart recreates the
  view hierarchy and restores the correct frame, matching the report.
- Existing diagnostics do not capture native parent bounds, WKWebView frame,
  JavaScript viewport dimensions, visual viewport dimensions, and root/body
  bounds in one correlated observation.

This is a missing ownership/synchronization contract, not evidence that density
CSS should resize the native window.

## Ownership contract

Rust owns live viewport synchronization. The authoritative native target is the
WKWebView parent NSView's current bounds. The WKWebView frame is an observation,
not an authority. React never caches an expected viewport, decides native
geometry, runs a retry/timer state machine, or mutates native size.

A new `viewport_sync` Tauri adapter contains all native/WebKit/macOS code and a
platform-neutral pure policy:

1. Hop to the macOS main thread with `run_on_main_thread`.
2. Inside one main-thread block, measure parent bounds and current webview frame
   in logical native points, run the policy, and apply any repair. Keeping the
   measure/decide/apply sequence in one block makes compare-then-set atomic with
   respect to AppKit layout and native resize processing.
3. Compare origin and size with one named sub-point tolerance.
4. Return `InSync` or `RepairToParentBounds`.
5. On macOS only, apply the parent bounds to the WKWebView frame when the policy
   says repair. Never call NSWindow/Tauri `set_size`, synthesize a DOM resize,
   or force layout.
6. Keep a Rust-owned monotonic observation generation for correlating native
   and DOM diagnostics. It is in-memory only and not product/persisted state.

Every NSView read and write, including the IPC-triggered path, is confined to
that single scheduled main-thread block. Unavoidable Tauri/Wry/objc2 calls stay
in the adapter. Policy types and decisions
are pure Rust and deterministic. Other platforms preserve their native webview
behavior and report `unsupported` rather than receiving a speculative repair.

## Triggers

Run the same idempotent synchronization boundary for:

- initial main-page load;
- native `Resized` and `ScaleFactorChanged` events (and fullscreen/maximize
  events represented by them);
- a one-shot frontend observation after a Display density render commit;
- a one-shot browser `resize` observation.

`Moved` remains window-persistence-only because it cannot change parent bounds.
Panel/layout transitions do not resize the native view; browser regression tests
must prove that opening/closing/resizing panels leaves root bounds unchanged.

The frontend call is observation-only. A small backend adapter captures finite
numbers from `window`, `document`, `visualViewport`, `body`, and `.desktop` and
submits a typed one-shot diagnostic payload. Rust performs any repair and logs
the correlated native/DOM result. No geometry is stored in React.

## Root layout contract

Use the existing inherited containing-block chain: `.desktop` has
`block-size: 100%` and `min-block-size: 0`, not `100vh`/`100dvh`. This removes a
second viewport-unit interpretation from the application root while preserving
viewport units for bounded overlays where they are semantically appropriate.

At every tested transition:

- root `top` and `left` are zero within subpixel tolerance;
- root width/height equal `documentElement.clientWidth/clientHeight`;
- root bottom/right equal the viewport edges;
- titlebar and right-panel close controls remain within the root viewport;
- no blank strip exists below the root.

## Diagnostics and privacy

Add one typed command under the diagnostics adapter. Its payload contains only:

- closed trigger and density tokens;
- finite, normalized dimensions;
- sign/magnitude or equality booleans for offsets;
- no room IDs, user IDs, text, URLs, selectors, screenshots, or raw errors.

Rust records source `desktop.viewport_sync` with observation generation,
trigger, native support, repair decision, parent/webview size, JS viewport and
root/body sizes, visual viewport presence/size/offset class, and root alignment
booleans. Invalid/non-finite payloads fail closed without recording or repair.
The existing bounded diagnostics buffer owns retention.

## Verify first

Before production changes, add checks that fail on the baseline:

1. Pure Rust policy RED: mismatched parent/webview origin or size requires
   `RepairToParentBounds`; matching/sub-tolerance values are `InSync`.
2. Pure Rust trigger RED: page load, native resize/scale-factor, density commit,
   and browser resize are admitted; moved/panel-only observations do not invent
   native window resizing.
3. Frontend reporter RED: Default → Compact → Default and Comfortable changes
   emit one finite one-shot measurement after the committed DOM state, with no
   expected-size cache, timer, forced reflow, or direct native mutation.
4. CSS contract RED: the application root uses the inherited 100% block size,
   not viewport units.
5. Browser-headless GREEN non-regression guard: after density transitions,
   viewport resize, right-panel open/close, and panel-width drag, `.desktop`,
   body, and viewport bounds remain equal and header/close controls stay
   reachable. This is not a baseline reproducer for the WKWebView defect.
6. Rust diagnostic tests prove private-data-free fields, invalid-number
   rejection, correlated generation, and repair/in-sync outcomes.
7. Source/adapter contract proves all macOS WKWebView frame access is isolated in
   `viewport_sync.rs` and no same-size window resize/synthetic DOM event exists.
8. Add a private-data-free macOS root-layout evidence token and assertion to
   `scripts/desktop-mac-gui-smoke.mjs`. The smoke must drive density transitions
   and native resize, then require a correlated Rust result that parent bounds,
   WKWebView frame, JavaScript viewport, and `.desktop` root are aligned. Update
   `docs/agents/qa-lanes.md` and the smoke's enforcing tests in the same change.

Chromium cannot reproduce a WKWebView frame bug. No unattended headless check
fails on the baseline for the native defect: item 4 provides the baseline CSS
RED, while item 8 is the only candidate native baseline reproducer on an
available macOS session. Pure policy/adapter tests are deterministic
cross-platform gates, and item 5 is a necessary GREEN layout guard. A macOS
compile alone proves only platform API compatibility. Manual visual inspection
is confirmation only.

## Implementation shape

Expected files:

- new `apps/desktop/src-tauri/src/viewport_sync.rs`;
- narrow wiring in `src-tauri/src/lib.rs` and
  `src-tauri/src/commands/diagnostics.rs`;
- existing macOS `objc2-app-kit` dependency gains only the `NSView` feature;
- a typed `DesktopApi`/`TauriDesktopApi` command method, browser-fake no-op,
  `tauriIpcMock` response, isolated frontend reporter, and narrow `App.tsx`
  wiring;
- `styles.css`, focused Vitest/contract tests, one browser-headless spec, and
  the macOS GUI smoke assertion and its tests;
- architecture/agent ownership and QA-lane documentation for the durable
  boundary.

Do not move density into the Matrix/product reducer merely to trigger geometry:
it is a presentation preference and this bug concerns native viewport authority.
This leaves the existing localStorage-backed density preference in tension with
the broader Rust-owned settings canon; resolving that ownership and its migration
is a separate settings task, not a reason to add a second source of truth here.
The new Rust state owns synchronization generation/decisions, while React keeps
only its existing presentation selection. Do not add a second persisted file,
DTO field, frontend expected-size state, debounce, retry, compatibility shim,
or dependency.

## Gates

- `reviewer-flash-opencode-go` design verdict `Correct-to-merge` before tests or
  implementation.
- `luna-implementer` at max thinking implements verify-first RED → GREEN.
- Parent reruns focused Rust/Tauri/frontend/browser gates and the complete
  applicable repository matrix.
- `reviewer-flash-opencode-go` reviews the exact finished diff and returns
  `Correct-to-merge`; all findings are fixed and re-reviewed.
- One independently reviewable commit series, PR closes #666, required CI is
  green, merge is confirmed on `main`, build artifacts are removed, and the
  worktree is clean.

## Review record

- Pre-implementation design review Round 1:
  `reviewer-flash-opencode-go`, `Not correct-to-merge`; required macOS QA,
  main-thread atomicity, and RED/GREEN classification corrections.
- Pre-implementation design review Round 2:
  `reviewer-flash-opencode-go`, `Correct-to-merge`; all Round 1 findings fixed.
