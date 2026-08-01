# Search scope, reply quote rendering, and session popover fixes (#403, #401, #400)

Date: 2026-08-02

## Problem restated

The post-#402 desktop surface has three independent regressions:

- The top-bar search scope column is only `88px` wide, so the `Room/DM`
  label (and its Japanese equivalent) is clipped or squeezed.
- Reply quotes expose only a collapsed plain-text preview even when the
  referenced Matrix event has Rust-sanitized formatted content. Lists, line
  breaks, links, and code blocks therefore lose their structure.
- The session-status popover references undefined `--surface-raised` and
  `--shadow-lg` tokens, so its background and shadow declarations are invalid
  and timeline content can show through it.

## Design

### #403 — search scope selector

Keep the existing three-value search semantics and default unchanged. Widen
the titlebar grid's selector track and give `.scope-select` a shared logical
minimum width derived from the longest supported English/Japanese label. Keep
the selector hidden at the existing mobile breakpoint. Add a browser-headless
layout assertion that checks both localized labels, the selector's rendered
width, and titlebar bounds without relying on screenshots.

### #400 — session status popover

Define the missing raised-surface and elevation tokens in the theme token set,
including dark-theme values, and keep the popover mapped to those tokens. Add
headless coverage that opens the existing Rust-shaped status popover in light
and dark themes and asserts an opaque computed background plus a non-empty
shadow, while retaining the current focus and IPC behavior tests.

### #401 — formatted reply quote DTO and renderer

Extend the Rust-owned `ReplyQuote` DTO with an optional, sanitized formatted
body containing `html`, `plain_text`, and `code_blocks`. The state crate owns a
quote-specific serializable mirror so it does not depend on `koushi-core`;
`koushi-core` converts the already-sanitized `TimelineFormattedBody` from the
embedded event projection into that mirror. Legacy plain previews remain the
fallback for missing/old/unsupported data, and redacted or unavailable
references keep their current state text.

The React timeline maps the quote formatted DTO into the existing
`renderFormattedBody` path, with no link-preview/media loading path. This
reuses the same allowlisted renderer for links, Markdown structure, spoilers,
and code blocks. Add a bounded quote style so long formatted content cannot
expand the timeline without limit. Update the checked-in TypeScript/core-event
contract fixtures and Rust serialization assertions with a non-empty formatted
quote example.

## Implementation tasks

1. Add the #403 browser-headless regression to
   `apps/desktop/e2e/top-bar-alignment.spec.ts`, run it red, then update
   `apps/desktop/src/styles.css` and rerun the focused Playwright spec.
2. Add the #400 light/dark computed-style regression to
   `apps/desktop/e2e/session-status.spec.ts`, run it red, then add the theme
   tokens and verify the focused session-status and shell tests.
3. Add a failing Rust projection/serialization test for formatted reply quotes
   in `crates/koushi-core/src/timeline.rs` and `crates/koushi-core/src/event.rs`.
   Add a failing React timeline test and browser-headless fixture assertion for
   list/code/link structure, then implement the state DTO, core conversion,
   TypeScript mirrors, renderer reuse, and quote bounds.
4. Regenerate or update the checked-in wire contract artifact using the
   repository's contract test, and update all existing ReplyQuote fixtures.
5. Run focused Rust, Vitest, Playwright, typecheck, lint, SDK-submodule, and
   IME gates as applicable. Read the complete diff including this plan and
   untracked files before committing.

## Verification commands

```bash
node scripts/check-sdk-submodule.mjs
cargo test -p koushi-core --lib timeline -- reply_quote
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml core_event_wire_format_matches_checked_in_contract_artifact
npm --prefix apps/desktop run test -- src/components/Shell.test.tsx src/components/TimelineView.test.tsx
npm --prefix apps/desktop exec -- playwright test e2e/top-bar-alignment.spec.ts e2e/session-status.spec.ts e2e/basic-operations.spec.ts -g "scope|popover|reply quote" --workers=1
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
```

## Scope boundaries

- Search scope values, default behavior, and all-room search performance are
  unchanged.
- Reply quote formatting does not add event navigation, media previews, or a
  second sanitizer.
- Session status command/state behavior is unchanged; only theme tokens and
  the presentation regression are addressed.
