# Issue #551 App Tauri timeline transport extraction

Status: implemented; full-diff review pending. Scope is the first behavior-preserving `App.tsx` ownership seam.

## Baseline

- Base: `ecde951d2ccf9e6046192139465be3e33e97e25b` after runtime audit PR #625.
- `App.tsx`: 7,245 newline-delimited lines / 258,131 bytes / SHA-256 `99bd020ccaceb8b1b132b8f51b79a86228c24a0a3f7071ccf0d5d26b13ab4983`.
- Focused baseline: `App.test.tsx` 80/80; desktop typecheck green.

## Ownership decision and immutable order

Create private direct module `apps/desktop/src/backend/tauriTimelineTransport.ts`. Move exactly these six declarations in original relative order:

1. `CORE_EVENT_NAME`
2. `tauriCoreEventListenerReady`
3. `tauriTimelineTransport`
4. `safeDownloadFilename`
5. `saveReadyMediaFile`
6. `isTauriRuntime`

`isTauriRuntime` moves because transport construction and media save call it while residual App listeners/QA/desktop flows also consume it; duplicating App's copy or importing App would violate the no-new-duplicate/no-cycle contract. The pre-existing private copy in `backend/client.ts` remains out of scope, so this move creates no third implementation.

Export exactly `CORE_EVENT_NAME`, `tauriTimelineTransport`, and `isTauriRuntime`. Keep listener readiness and media filename/save helpers private. Import the three exports directly in `App.tsx`; no barrel or App re-export.

The leaf owns module-load Tauri timeline transport construction, CoreEvent listener readiness, all timeline command adapters and ready-media filesystem save. It owns no React state, product state, snapshot mutation or Matrix semantics.

## Exact behavior contract

Preserve event name `koushi-desktop://event` and every command adapter exactly:

- timeline subscribe, room/thread pagination and room repair;
- reaction/redaction, retry/cancel send;
- read receipt, fully-read and typing;
- edit/redact, pin/unpin;
- media/avatar download and ready-file save;
- message source, room key and forwarding;
- link-preview load/hide;
- viewport observation, navigation scroll anchor and timestamp open.

Preserve the listener lifecycle byte-exact: `disposed` and `unlisten` are set synchronously, late `listen()` resolution disposes immediately after an early unsubscribe, and `ensureSubscribed` awaits the listener-ready promise before invoking `ensure_timeline_subscribed`.

Preserve module-load `isTauriRuntime()` detection; do not replace the transport with a hook, factory, context, lazy initializer or callback registry.

## Imports and App residual

Destination imports are explicit:

- `invoke` from `@tauri-apps/api/core`;
- `listen` from `@tauri-apps/api/event`;
- `save as saveDialog` from `@tauri-apps/plugin-dialog`;
- `TimelineTransport` from `../components/timeline/TimelineTransport`;
- `CoreEventPayload`, `TimelineGapId`, `TimelineKey` from `../domain/coreEvents`;
- `ComposerDocument`, `TimelineScrollAnchor` from `../domain/types`;
- `t` from `../i18n/messages`.

App adds one direct import for the three exports. Remove only `TimelineGapId` and `TimelineScrollAnchor` from App type imports; retain `invoke`, `listen`, `saveDialog`, `CoreEventPayload`, `TimelineKey`, `ComposerDocument`, `TimelineTransport` and `t` because residual App code uses them.

Leave in App:

- QA-send, menu, StateDelta and state-refresh listeners/timer;
- `appTimelineTransport` memo that augments the leaf with snapshot/navigation actions;
- avatar effects/callbacks that call the leaf;
- desktop-attention transports/effects;
- secure-backup chooser/file paths;
- composer draft lifecycle;
- global QA error and pointer-resize listeners;
- all render branches/public compatibility re-exports.

React continues to own DOM/Tauri listener cleanup. Rust remains the product-state/command/event owner; no DTO, command name, Tauri registration, QA token or i18n catalog changes.

## Tests and source contracts

Move no tests. Update only two `App.test.tsx` source contracts:

- `Tauri timeline transport routes thread pagination by TimelineKey`;
- `Tauri timeline ensure waits for the webview CoreEvent listener registration`.

Both read `./backend/tauriTimelineTransport.ts`, bound `const tauriTimelineTransport` through `function safeDownloadFilename`, and preserve all existing pagination/root-event/listener-order assertions. No assertion is removed or weakened.

Linux/macOS QA source tests continue reading `App.tsx`; imported calls `isTauriRuntime()` and `CORE_EVENT_NAME` remain at the same residual listener sites.

## Deterministic exactness

A temporary TypeScript AST verifier compares immutable base with parent + leaf:

- declarations6/6 in relative order, parent0;
- initializer/function bodies, type annotations and comments exact modulo approved export modifiers;
- exports3/private3, destination import paths7, App direct import3;
- App orphan type imports2 and no other import deletion;
- command string/method inventory and lifecycle statements exact;
- source tests2 retain assertion sets with only approved owner path/boundary edits;
- public App exports and App hooks/listeners/timers/render tree unchanged; App edge changes are the planned direct import and two orphan removals, while new leaf type/i18n edges are acyclic and no reverse edge is introduced;
- duplicate/missing/excess declarations0.

## Verification

Run App tests80, typecheck, lint, full Vitest and Playwright with polling, build, boundary/security/source checks and diff/format checks. After full-diff approval, integrate latest `origin/main` if required, run the complete repository matrix and PR CI7/7.

The App split-later checkbox remains open for diagnostics, verification/destructive UI, composer/attention re-evaluation and final residual audit.

## Review gate

- Read-only App reconnaissance measured hooks/resources/render branches and selected the transport as the cleanest existing ownership seam.
- `reviewer-flash` independently traced declaration/import closure, all transport methods/commands, listener/media lifecycle, source assertions, QA regexes and edge direction and recorded `Correct-to-implement`.
- Implementation integrated by `luna-implementer` and parent-audited.
- Exactness: declarations6/6, parent0, exports3/private3, destination imports7, command strings27, App orphan imports2, retained top-level declarations exact, source assertions2 unchanged; hook/listener/timer/render/public-export deltas0.
- `App.tsx` 7,245 → 7,065 newline-delimited lines; `backend/tauriTimelineTransport.ts` 194.
- Focused App80, typecheck/lint, full Vitest1,370, Playwright248, build/boundary/security and diff checks green.
- Full diff and delivery pending.
