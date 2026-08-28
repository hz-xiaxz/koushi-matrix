# Issue #552 Phase 4.2 — Diagnostics dialog request-fence decision

Status: implemented, locally verified and exact-final-diff approved. `reviewer-flash` design Round 4 returned `Correct-to-merge` before implementation after verifying the overlap-test mechanics, `openDiagnostics`-only source scope and deliberate page/account lifetime. Exact-diff Round 1 passed all seven focus areas with no blocking findings, and verdict-only Round 2 recorded `Correct-to-merge`. Focused diagnostics GREEN is 16/16.

## Decision

**Keep** App's diagnostics request epoch as a renderer-specific dialog-open intent owner. Rename `diagnosticSnapshotRequestGenerationRef` to `diagnosticsOpenIntentEpochRef` and document its lifetime; do not move it to Rust/Tauri, replace it with appStore generation admission, or introduce a generic request manager.

## Traced boundary

```text
Open diagnostics click
  -> App increments dialog-open intent epoch
  -> DesktopApi.getDiagnosticSnapshot()
  -> Tauri get_diagnostic_snapshot (no arguments, no RequestId)
  -> await media-preparation stats + read sliding-sync/global diagnostics
  -> privacy-safe FrontendDiagnosticLogSnapshot (no state_generation)
  -> App admits success/failure only for latest open intent
  -> renderer-local runtimeDiagnosticSnapshot / diagnostics.fetch fixed token
  -> DiagnosticDialog opens
```

### Rust/Tauri authority already present

- Tauri owns assembly of privacy-safe diagnostics, including media/thumbnail summaries and Rust sliding-sync/global diagnostic records.
- `koushi_diagnostics` owns the bounded runtime diagnostic buffer and dropped-entry count.
- The command accepts no account, dialog, generation or request argument and returns a diagnostic DTO, not `DesktopSnapshot`.
- BrowserFake mirrors the DTO only; `client.test.ts` proves the argument-free IPC command.

### Why the renderer epoch is not duplicate semantics

1. Diagnostic snapshots are outside `AppState`, `StateDelta`, Core command terminals and `appStore`; there is no Rust `RequestId`, demand generation or `state_generation` that can order overlapping promises.
2. Only React knows which click should open the one diagnostics dialog. Tauri can produce a valid snapshot for each invocation but cannot know that a later click superseded an earlier open intent.
3. A newest failure intentionally retains the last successful runtime snapshot, appends only the fixed private-data-free `diagnostics.fetch kind=unavailable` renderer record, and still opens the dialog. An older late success or failure must not replace/annotate that latest result.
4. Closing an already-open dialog is presentation state. It does not cancel Rust work; a stale overlapping request remains fenced by the later open epoch and cannot reopen the dialog. The epoch is intentionally page-lifetime and is not reset on account replacement: the snapshot contains only privacy-safe global/runtime diagnostics, and `DiagnosticDialog` composes it with the current AppState rather than prior account state.
5. Serializing or disabling clicks would change observable behavior without creating a stronger owner. Moving dialog intent into Rust would invert the ownership boundary.

## Existing evidence

`App.diagnostics.test.tsx` already proves:

- every open fetches a fresh diagnostic snapshot;
- a failure preserves the prior successful snapshot, emits only a fixed renderer diagnostic and exposes no raw error/private value;
- overlapping opens admit newest failure and ignore an older late success;
- diagnostic DTOs and exported reports remain private-data-free.

## Scope

In this PR:

- rename the ref to `diagnosticsOpenIntentEpochRef` and add an ownership comment;
- extend the exact existing test `only the newest overlapping snapshot success can survive a stale failure`: resolve the second/newest success, close its dialog, reject the first/stale request, assert the dialog remains closed, then perform a third successful open and assert no stale `diagnostics.fetch kind=unavailable` record was appended;
- add a source ownership contract scoped specifically to `openDiagnostics` (not the separate stateless `copyDiagnostics` path), proving that dialog-open completion remains outside `setSnapshot`/appStore and uses one local epoch on both success and failure;
- classify this family as renderer-specific in the ownership inventory/canon;
- update Phase 4 status and plan index.

No Rust/Tauri command, API/DTO, BrowserFake behavior, dialog accessibility, diagnostics content, retry/backoff, dependency or IPC change.

## Verify-first policy

There is no production behavior fix, so no fabricated RED is required. Existing and new adversarial tests are unchanged GREEN proof against deleting the epoch. The mechanical rename/source contract must be complete before merge.

Run focused diagnostics tests, full Vitest/Playwright, typecheck/lint/build, privacy/boundary/docs gates, applicable Rust/Tauri/wasm/dependency gates, exact-final-diff review and current-head CI.

### Local verification evidence

- focused diagnostics: 16/16;
- full Vitest: 1494/1494;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs, build, secret scan, Tauri adapter and domain dependency guards: passed;
- SDK submodule sync, diagnostic isolation, rustfmt, workspace tests (2535 passed/12 ignored), Tauri tests (175 passed/1 ignored), wasm check, QA binary tests (135 passed), cargo-deny and cargo-machete: passed.

No real-homeserver lane is added for this proof-only renderer intent rename/test change: Rust/Tauri diagnostic assembly, IPC, DTOs and runtime resources are unchanged, and deterministic App/BrowserFake tests exercise the exact completion boundary.

## Acceptance

- every diagnostics dialog-open result/failure settles only the latest renderer open intent;
- stale failure cannot annotate or reopen a later successful dialog after the user closes it;
- the page-lifetime epoch deliberately survives account replacement because the Rust DTO is global/runtime and private-data-free;
- no raw error or private diagnostic value enters renderer logs;
- docs distinguish Rust diagnostic-content authority from renderer dialog-intent authority;
- no generic manager, Rust panel state, retry loop or IPC change is introduced;
- later Phase 4 request families remain separately gated.
