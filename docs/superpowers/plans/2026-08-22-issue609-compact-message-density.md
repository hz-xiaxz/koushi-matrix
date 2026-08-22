# Issue #609 Compact Message Density

## Scope

Improve the existing Compact display mode only. Consecutive messages from the same sender visually reuse the first row's avatar/name and use tighter vertical spacing. Do not add a fourth density mode, font-size slider, persisted setting, Rust DTO/state change, message aggregation, or timeline-order mutation.

## Ownership and presentation contract

The timeline remains a Rust-ordered sequence of independent message DTOs. React derives one presentation-only adjacency fact from the already projected `visibleRows`; it never rewrites, merges, or drops messages.

A row is a same-sender continuation only when:

- the current and immediately preceding full `visibleRows` entries are renderable `event` or `threadRoot` rows;
- both `item.sender` values are non-empty and identical;
- no separator is placed between them: an unread marker on the current row or a read marker after the preceding row breaks the run.

Date dividers, timeline gaps, pending/failed thread-root placeholders, different senders, and marker boundaries break a run. The comparison uses the full row list and `visibleIndex`, not merely the virtual window, so virtualization boundaries do not invent a new first row.

The row receives a semantic continuation class/attribute regardless of active density. CSS under `.desktop[data-density="compact"]` alone:

- suppresses the continuation avatar visually while retaining its alignment column;
- visually hides the repeated sender label without deleting the accessible DOM text;
- tightens Compact row block padding, with continuation rows tighter than run starts;
- preserves timestamps, edited/send-state labels, actions, receipts, media, thread summaries, message IDs, and all event semantics.

Default and Comfortable output/layout remain unchanged.

## Verify first

1. RED component test: two adjacent same-sender event rows mark only the second as continuation.
2. Different senders do not group.
3. Date-divider/gap/thread-placeholder and both marker directions break grouping: unread above the current row and read below the preceding row.
4. A virtual window beginning mid-run still marks its first rendered row from the preceding full row.
5. Existing row count, event IDs, actions, and visible message bodies remain unchanged.
6. CSS contract pins Compact-only avatar/sender suppression and tighter padding; Default/Comfortable selectors do not consume the continuation class.
7. Browser-headless rendering confirms Compact hides duplicate visible avatar/name while Default continues to show both.

## Implementation

Use the existing `visibleRows`, `visibleIndex`, row kinds, and marker IDs in `TimelineView`. Pass one boolean to `TimelineItemRow`, add one class on the existing `<article>`, and add the smallest Compact-only CSS rules. Reuse the repository's existing visually-hidden pattern if present; otherwise use the standard clipping properties inline in the selector. No helper module, context, state, memo, observer, or dependency.

## Gates

- `reviewer-flash-opencode-go` design verdict: `Correct-to-merge`; no blocking findings. The review verified full-list virtualization adjacency, both marker directions, accessible sender retention, Compact-only CSS, and Rust ownership.
- `luna-implementer` at max thinking for verify-first implementation.
- RED: rendering tests failed on missing continuation behavior and style contract failed on missing Compact selectors. GREEN: rendering 24/24, styles 30/30, typecheck, lint, and focused Playwright 1/1 all exited 0; Playwright used polling because the host inotify watch limit was exhausted.
- `reviewer-flash-opencode-go` reviewed the exact 436-line full patch and returned `Correct-to-merge` with no blocking findings.
- Integrated full local matrix, CI, merge, issue evidence, and build-artifact cleanup in the shared PR.
