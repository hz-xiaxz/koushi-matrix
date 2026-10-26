# Memory retention diagnostics

## Scope and canon

Add observational diagnostics for event-broadcast retention and renderer-owned
resources. Consulted REPOSITORY_RULES.md, AGENTS.md, architecture overview,
engineering rules and agent verification/state-ownership guidance. Diagnostic
sampling must not consume events, attach receivers, change cache lifetimes,
force GC or export message text, identifiers or media URLs.

## Changes and review

- Core runtime/account manager expose broadcast capacity, retained count,
  receiver count and inline slot lower bound. The desktop adapter samples
  already-cached command receivers without waiting for their connection locks.
  Raw unread positions, capacity-clamped pending events and overwritten positions
  are distinct. Sampling is non-atomic; nested payload heap remains unavailable.
- Renderer diagnostics sample DOM/text counts, natural-size image RGBA8 reference
  sizes (including a separate per-source estimate), canvas reference sizes,
  live object-URL Blob sizes and timeline-store counts/text lengths.
  Object-URL instrumentation replaces existing creation/revocation calls without
  changing ownership or cleanup timing. The registry retains URL strings and
  numeric sizes only, never Blob objects.
- Existing Open diagnostics action captures the renderer store and native
  summaries; existing report formatting exports these measurements. No new IPC
  command, product-state transition or wire event is introduced. Architecture
  and troubleshooting documentation describe the interpretation and limits.
- Self-review traced the production action through snapshot collection and
  report output, audited all production object-URL call sites, and checked that
  only primitive numeric samples escape. This diagnoses growth; it does not fix
  suspected idle-receiver retention or claim exact WebKit/GPU memory usage.

## Verification

New renderer and core checks failed before the implementation and pass after it.
Core checks use a real broadcast with an idle receiver, a draining receiver and
overflow; repeated observations neither consume events nor add subscribers.
Export tests distinguish overwritten positions from retained payloads and
unavailable measurements from zero. Renderer tests check duplicate sources,
Blob revocation accounting, missing versus empty stores and content exclusion.

Frontend suite: 185 files / 2291 tests passed; the final report-scope refinement
also passed its 11 report tests. TypeScript, frontend lint and production build
passed. Rust library suites passed: core 1764, desktop 279, SDK 297 and state 103
(existing ignored tests remain ignored). Core/desktop Clippy including test
targets passed with warnings denied. Rustfmt, SDK submodule, Rust test structure,
Tauri adapter, command snapshot, user-help, secret-scan and diff guards passed.

The running application was not rebuilt, restarted or driven by automation.
