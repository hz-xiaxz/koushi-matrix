# Issue #552 Phase 2B4 — Event subscription platform port

Status: implemented and fully verified on the approved branch; pending final exact-diff re-review and merge.

## Scope and invariant

Phase 2B4 isolates the remaining Tauri event-listener operations behind one neutral port. It completes Phase 2 structural platform isolation without changing event names, payloads, listener timing, late-registration disposal, batching, timers, menu routing, QA settlement, timeline readiness, Rust state, IPC, or DTOs.

After this phase:

- `App.tsx` imports no Tauri package;
- `backend/tauriTimelineTransport.ts` retains command/invoke adapter ownership but receives Core events through the neutral event port rather than importing Tauri events;
- `backend/tauri/desktopEventPort.ts` alone owns Tauri `listen` and the three closed event channel names;
- every remaining production `@tauri-apps/*` import is in a statically enumerated backend adapter;
- React remains the owner of renderer listener cleanup, batching/debounce timers, QA presentation, and menu-to-shortcut dispatch; no product state or async backend resource moves to TypeScript.

Out of scope: Tauri command `invoke` adapters, event emitters in the Tauri harness, Rust event production, listener semantic migration, acknowledgement reliability (Phase 3), and later #552 owner phases.

## Current behavior to preserve exactly

1. All listeners are installed only on existing Tauri-guarded paths; no browser listener/fallback is added.
2. Core channel remains `koushi-desktop://event`; menu remains `koushi-desktop://menu`; state wake remains `koushi-desktop://state`.
3. The adapter unwraps only Tauri's `{ payload }` envelope. `CoreEventPayload` and menu string values reach existing consumers unchanged; state wake payload remains ignored.
4. Each App effect retains `disposed`, late `listen(...).then(dispose => ...)` settlement, synchronous cleanup, and empty dependency array.
5. QA send completion keeps its dedicated Core listener and existing `qaSendPending` gate/status projection.
6. Menu payload parsing and `handleShortcutAction` remain in App.
7. StateDelta batching remains ordered and flushes through `applyAppStoreDeltas`; forward gaps still call `refresh`.
8. State wake debounce remains the renderer-owned 250 ms timer with identical unmount cancellation.
9. `tauriTimelineTransport.listenCoreEvents` retains its own disposed/late-unlisten handshake. `ensureSubscribed` still awaits the exact latest listener-registration promise before invoking `ensure_timeline_subscribed`.
10. App's separate timeline-store listener through `TimelineTransport` is unchanged.

## Design

### Neutral contract and composition

Add contract-only `backend/desktopEventPort.ts`:

- `listenCoreEvents(listener: (payload: CoreEventPayload) => void): Promise<() => void>`;
- `listenMenuActions(listener: (payload: string) => void): Promise<() => void>`;
- `listenStateChanges(listener: () => void): Promise<() => void>`.

Add `backend/desktopEventRuntime.ts` exporting one factory-created `desktopEventPort`. As in Phase 2B3, compose the Tauri adapter unconditionally to preserve existing module-import behavior; all actual calls remain behind current Tauri/timeline guards. Factory construction must perform no subscription.

### Tauri adapter

Add `backend/tauri/desktopEventPort.ts` with `createTauriDesktopEventPort()`:

- private closed constants for the exact three event names;
- each method calls Tauri `listen` once with its exact payload type and passes only `event.payload` (or no state payload) to the neutral listener;
- return Tauri's unlisten promise/result unchanged;
- no batching, retry, disposed flag, timer, runtime check, state projection, or event-name export.

### Caller migration

- Remove App's event import/disable and the local menu/state event constants. Import the concrete object from `backend/desktopEventRuntime.ts` and replace the four raw listeners with the matching methods. Keep all four effects' guards, local lifecycle variables, `.then` late-disposal logic, cleanup, batching/debounce, and dependencies exactly in App.
- Remove `CORE_EVENT_NAME` from App's timeline-transport import.
- In `tauriTimelineTransport`, remove only the Tauri event import and private/exported Core event constant; import the concrete `{ desktopEventPort }` from `./desktopEventRuntime` (never the Tauri implementation directly) and call `desktopEventPort.listenCoreEvents`. Keep its Tauri core `invoke` import, null browser selection, listener-ready promise, command methods, and export of `tauriTimelineTransport` unchanged.
- Update `App.test.tsx`: App's Tauri import statement set becomes empty; the timeline transport source contract anchors on `tauriCoreEventListenerReady = desktopEventPort.listenCoreEvents`; add non-vacuous source assertions for all four App subscription methods and their late-disposal/cleanup tokens.
- Update `scripts/linuxGuiQa.test.ts` source regex from raw `listen<CoreEventPayload>(CORE_EVENT_NAME` to the exact sequence `/if \(!isTauriRuntime\(\)\)[\s\S]*desktopEventPort\.listenCoreEvents[\s\S]*qaSendPending\.current[\s\S]*qaSendCompletionStatusFromCoreEvent[\s\S]*setQaSendStatus\(eventStatus\)/`, scoped to the existing empty-dependency effect.
- Existing specialized App tests and the Playwright harness may retain official Tauri event mocks/emitters; the new adapter consumes those mocks transitively and the harness remains an allowed test boundary.

### Static boundary

- Remove the last App ESLint grandfathering comment/disable. Update ESLint comments to state zero direct App/component/app-hook/appRuntime Tauri imports.
- Keep `no-restricted-imports` enforcement on App and all prior neutral layers.
- In `App.test.tsx`, assert that App has zero Tauri import statements and enumerate the exact remaining production import modules: `backend/client.ts`, `backend/tauriTimelineTransport.ts`, and `backend/tauri/{desktopAttentionPort,desktopEventPort,linkMediaPort,windowDialogPort}.ts`. Add an ESLint backend guard that rejects `@tauri-apps/**` everywhere under `src/backend/**` except tests, `backend/tauri/**`, `client.ts`, and `tauriTimelineTransport.ts`; this makes the six-module allowlist static rather than documentary.

## Verify-first tests

Before production edits, add jsdom `backend/desktopEventRuntime.test.ts` with `vi.resetModules()` and a factory spy. It proves one adapter is composed and no subscription occurs eagerly. This is RED because the runtime module does not exist. Before App/transport production edits, also apply and run the RED source contracts for App's empty Tauri import set and four port methods, the timeline readiness assignment `tauriCoreEventListenerReady = desktopEventPort.listenCoreEvents`, the six-module adapter allowlist, and the exact Linux-GUI QA regex above.

Add jsdom `backend/tauri/desktopEventPort.test.ts` with mocked Tauri `listen` and prove:

- exact channel name and generic payload envelope unwrapping for Core and menu;
- state listener ignores payload and wakes once;
- each method returns the exact disposer produced by Tauri;
- factory construction performs no listen.

Focused App/timeline/source tests must prove each listener still owns late-registration disposal, cleanup, batching/timer teardown, and the timeline readiness await ordering.

## Expected files

- `apps/desktop/src/backend/desktopEventPort.ts` (new)
- `apps/desktop/src/backend/desktopEventRuntime.ts` (new)
- `apps/desktop/src/backend/desktopEventRuntime.test.ts` (new)
- `apps/desktop/src/backend/tauri/desktopEventPort.ts` (new)
- `apps/desktop/src/backend/tauri/desktopEventPort.test.ts` (new)
- `apps/desktop/src/backend/tauriTimelineTransport.ts`
- `apps/desktop/src/App.tsx`
- `apps/desktop/src/App.test.tsx`
- `apps/desktop/src/scripts/linuxGuiQa.test.ts`
- `apps/desktop/eslint.config.js`
- ownership inventory and Phase 2 plan/index docs

No appRuntime, domain, component, Rust, IPC, DTO, generated artifact, CSS, dependency, BrowserFakeApi, or harness change is expected.

## Verification matrix

- focused runtime/Tauri adapter/App/timeline/Linux-GUI source tests;
- full Vitest and Playwright;
- typecheck, lint, build, IME/docs checks;
- Tauri/domain boundary guards, SDK submodule, secret scan, and `git diff --check`;
- exact-final-diff `reviewer-flash` verdict and current-head CI before merge.

## Design review record

- Round 1, `reviewer-flash`: `Correct-to-merge` with four Minor precision findings. The concrete runtime import target, App.test/ESLint six-module allowlist gate, RED ordering for source contracts, and exact QA regex are incorporated above; a focused Round 2 confirms them before implementation.

## Implementation evidence

- RED: runtime composition failed because `desktopEventRuntime` did not exist; App/timeline/Linux-GUI source contracts failed on all four old raw-listener/import paths before production edits.
- Focused runtime/Tauri adapter/App/timeline/Linux-GUI tests: 4 files / 125 tests passed.
- Full Vitest: 98 files / 1480 tests passed.
- Playwright: 263 tests passed.
- Typecheck, lint/IME/docs, production build, Tauri/domain guards, SDK-submodule check, secret scan, and `git diff --check` passed.

## Acceptance

- App and all neutral/domain/app-hook layers import no Tauri package;
- Tauri event imports exist only in `backend/tauri/desktopEventPort.ts` and allowed tests;
- all event names, payload delivery, setup/teardown, batching/debounce, and readiness ordering are unchanged;
- Phase 2B exits with every direct production Tauri import adapter-owned and statically enumerated;
- full gates pass and #552 remains open for semantic ownership phases.
