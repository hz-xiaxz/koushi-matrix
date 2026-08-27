# Issue #552 Phase 2B1 — External-Link and Media Platform Ports

Status: design awaiting independent `reviewer-flash` verdict. No implementation is authorized before `Correct-to-merge`.

Base: `origin/main` `732459ad219a692054270d33ea9b105aff2c54ce` after merged Phase 2A PR #711.

## Objective

Remove direct Tauri imports from external-link and media URL/save domain surfaces without changing user behavior. One neutral existing-operation port family selects a browser or Tauri implementation; React continues to call small functions and owns no platform details.

This is Phase 2B structural adapter isolation. It does not migrate product semantics and does not close #552.

## Current boundary

- `domain/externalLinks.ts` owns pure HTTP(S) validation **and** imports `@tauri-apps/plugin-opener`; on opener failure it falls back to `window.open`.
- `domain/mediaUrl.ts` imports `convertFileSrc` and converts filesystem/file URLs while passing web/asset/thumbnail/data/blob URLs through.
- `backend/tauriTimelineTransport.ts` imports Tauri dialog/core APIs and owns filename sanitization plus the default-path/dialog/save flow.
- `appRuntime.ts` imports `tauriTimelineTransport::isTauriRuntime`; the timeline transport would create a cycle if it imported a selected media-save port that imported the predicate back from the timeline module.
- Browser preview behavior is implicit through caught Tauri failures/no-op save rather than an explicit adapter.

## Target boundary

1. Add `backend/runtimeEnvironment.ts` containing the existing exact predicate `"__TAURI_INTERNALS__" in window`. `appRuntime.ts`, `tauriTimelineTransport.ts`, `App.tsx`, and `app/useDesktopAttentionEffects.ts` import it directly; remove the predicate export from the timeline module and add no re-export shim. No predicate behavior changes.
2. Add one neutral `backend/linkMediaPort.ts` interface with only the existing operations:
   - `openHttpUrl(validHttpUrl)`;
   - `mediaSourceUrl(sourceUrl)`;
   - `saveMediaFile(sourceUrl, filename)`.
3. Add `backend/tauri/linkMediaPort.ts` implementing the current opener fallback, `convertFileSrc` conversion, safe-filename/default-path/dialog/save sequence with the exact existing Tauri command names and localized title.
4. Add `backend/browser/linkMediaPort.ts` implementing the current effective browser behavior: `window.open` for valid HTTP(S), source URL passthrough, and no-op save.
5. Add `backend/linkMediaRuntime.ts` as the sole per-call selector using `runtimeEnvironment::isTauriRuntime`. It exports the existing renderer-facing function names `openExternalHttpUrl`, `mediaSourceUrl`, and `saveReadyMediaFile`; invalid/non-HTTP links are rejected before either adapter.
6. Keep `domain/externalLinks.ts::toExternalHttpUrl` as a pure validator. Delete its platform opener and delete `domain/mediaUrl.ts`; update all callers to import renderer-facing operations from `backend/linkMediaRuntime`.
7. `tauriTimelineTransport` keeps all timeline transport semantics and imports only `saveReadyMediaFile`; remove its local save helper, Tauri dialog import, and predicate export. No timeline command/listener changes. Re-anchor the two `App.test.tsx` transport source slices on the module's `export {` boundary so removal of the old helper cannot silently widen them.
8. Extend ESLint so production `domain/**/*.ts` cannot import `@tauri-apps/*`, excluding `domain/**/*.test.ts` and the still-unmigrated `domain/desktopNotification.ts` until the notification family PR. Existing App/hook exceptions remain unchanged.

No generic platform framework, dependency, context/provider tree, re-export shim, IPC rename, DTO change, or unrelated notification/window work.

## Verify-first proof

Add `backend/linkMediaRuntime.test.ts` before production edits under jsdom. Mock `runtimeEnvironment`, browser port, and Tauri port; prove:

- a valid HTTP(S) link routes only to the selected adapter;
- invalid/file/javascript links route nowhere;
- media URL conversion and save route only to the selected adapter;
- both Tauri and browser branches are deterministic and no second selector exists.

The test is RED on the base because `linkMediaRuntime`/ports do not exist. Keep pure URL-normalization assertions in `domain/externalLinks.test.ts`; move Tauri opener/media conversion/save assertions to adapter/runtime tests. Update `TimelineView.interactions.test.tsx` and `TimelineView.media.test.tsx` mocks to target `backend/linkMediaRuntime`, and update `backend/appRuntime.test.ts` to mock `runtimeEnvironment` instead of the timeline module. No sleeps or log assertions.

## Behavior invariants

- HTTP(S) normalization and rejection are unchanged.
- Tauri opener failure still calls `window.open(url, "_blank", "noopener,noreferrer")`.
- Web/asset/custom-thumbnail/data/blob URLs pass through unchanged.
- `file://` decoding and local path `convertFileSrc` behavior are unchanged in Tauri.
- Browser preview returns source URLs unchanged and media save remains no-op.
- Filename sanitization, `default_media_save_path`, dialog cancellation, and `save_downloaded_media` arguments remain exact.
- `tauriTimelineTransport`, App/component method signatures, and Core/Tauri wire contracts remain unchanged.

## Implementation sequence

1. Add and run the RED runtime-routing test; record exact missing-module failure.
2. Add neutral/environment ports and both concrete implementations.
3. Move wrappers/callers; delete the domain platform import/file and timeline-local save helper.
4. Extend the domain import guard and update tests without changing unrelated behavior.
5. Update inventory, Phase 2 plan/index and this implementation record.
6. Run focused tests, all Tauri-importing domain tests, typecheck, lint, full Vitest, build, Playwright, secret/boundary/docs/SDK guards, a direct-import inventory assertion, and `git diff --check`.
7. Obtain `reviewer-flash` exact-diff approval before PR/merge.

## Expected files

- `apps/desktop/src/backend/{runtimeEnvironment,linkMediaPort,linkMediaRuntime}.ts`
- `apps/desktop/src/backend/{browser,tauri}/linkMediaPort.ts`
- `apps/desktop/src/backend/linkMediaRuntime.test.ts` and focused Tauri implementation tests
- `apps/desktop/src/backend/{appRuntime,tauriTimelineTransport}.ts`
- `apps/desktop/src/domain/externalLinks.ts` + test; delete `domain/mediaUrl.ts` + move its test
- `apps/desktop/src/App.tsx` (only `openExternalHttpUrl` and `isTauriRuntime` import sources change; its three existing direct Tauri imports remain)
- `apps/desktop/src/app/useDesktopAttentionEffects.ts` (predicate import source only)
- `apps/desktop/src/components/{Shell,mediaLists,UserSettingsPanel}.tsx`
- `apps/desktop/src/components/timeline/{ReceiptReaders,TimelineMedia,TimelineMessageBody,TimelineItemRow}.tsx`
- `apps/desktop/src/components/{TimelineView.interactions,TimelineView.media}.test.tsx`
- `apps/desktop/src/App.test.tsx`
- `apps/desktop/src/backend/appRuntime.test.ts`
- `apps/desktop/eslint.config.js`
- `docs/architecture/frontend-ownership-inventory.md`
- `docs/superpowers/plans/2026-08-27-issue552-remaining-ownership-phases.md`
- `docs/agents/plans.md`

## Design review record

- Round 1, `reviewer-flash`: **Not correct-to-merge**. Important findings required a direct `runtimeEnvironment` import decision with no re-export shim, explicit mock updates for AppRuntime and TimelineView tests, and exclusion of domain test files from the new lint rule. Minor findings required stable source-test anchors, the exact App hot-file boundary, and enumerated callers. All are incorporated above.

## Acceptance

- No direct Tauri import remains in `domain/externalLinks.ts` or a media URL domain module; domain Tauri imports are statically denied except the explicitly deferred notification file.
- One neutral three-operation family; one browser and one Tauri implementation; one runtime selector.
- Existing external-link, media rendering and save behavior remains byte/argument equivalent.
- No second product owner: these are platform operations only.
- Deterministic RED/GREEN routing, adapter behavior and full frontend gates pass.
- Inventory records Phase 2B1 as structural only and leaves #552 open.
