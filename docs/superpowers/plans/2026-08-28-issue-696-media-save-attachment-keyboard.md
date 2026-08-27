# Issue #696 Media Save And Attachment Keyboard Plan

Date: 2026-08-28
Status: Implemented

## Goal

Make a timeline attachment's first Download click open the save flow after Rust finishes downloading it. Make the upload-staging caption use the same Rust-resolved send shortcut as the main/thread composer, and make forward Tab from the caption focus Send.

## Design

1. Add a React regression that clicks an initially idle file attachment once, observes `downloadMedia`, updates the Rust-owned download DTO to `ready`, and expects `saveMediaFile` without a second click.
2. In `TimelineMediaAttachment`, retain only the pending UI save intent for that mounted media row. Consume it only when that row's Rust-owned `downloadState` becomes `ready`, then call `saveMediaSource` with the same arguments as the ready-state button. A `ready` state without a click never saves. Clear intent on `failed`; a retry click re-arms it, so retry-to-ready saves once. Unmounting before ready may discard the presentation intent; the ready row still exposes its normal save button. Do not duplicate download state, bytes, Matrix semantics, or native save logic.
3. Pass the existing `ResolveComposerKeyAction` and a narrowed `main | thread` surface into `UploadStagingDialog`. On caption Enter, send the same key facts to that resolver with `autocomplete_open: false` and `send_enabled` equal to the dialog's sendability. Send only when it returns `send`; other actions remain unsent. `ImeTextField` keeps composing Enter from reaching this handler.
4. On unmodified forward Tab from the last (or only) caption, focus the enabled Send button. When Send is disabled, and for earlier captions, Shift+Tab, or other focus movement, keep native tab order.

## Scope

- Reuse `saveMediaSource`, `composerKeyEventFromDom`, and the existing Rust shortcut resolver.
- No Rust state changes, new commands, new abstractions, new dependencies, or unrelated media/composer changes.
- Cover main and thread prop wiring with focused headless tests.
- Media RED cases: first click followed by ready saves once; ready without a click does not save; click followed by failed, retry, and ready saves exactly once.
- Dialog RED cases: resolver `send` submits; a non-send action does not; composing Enter remains unsent; Tab from the last sendable caption focuses Send; non-sendable and earlier-caption Tab retain native order.

## Review Gate

- Round 1 — `reviewer-flash`: Findings-required. Clarified last-caption/enabled-Send Tab scope and explicit save-intent failure/no-intent behavior plus RED cases.
- Round 2 — `reviewer-flash`: Correct-to-merge.
- Post-implementation diff review — `reviewer-flash`: Correct-to-merge.

## Verification

```bash
npm --prefix apps/desktop test -- src/components/TimelineView.media.test.tsx src/components/dialogs.test.tsx src/components/rightPanel.test.tsx src/components/TimelinePane.renderIsolation.test.tsx
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
npm --prefix apps/desktop run test:ui-headless
node scripts/check-sdk-submodule.mjs
cargo fmt --all -- --check
git diff --check
```
