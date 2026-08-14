# Formatted-body newline preservation (#522)

Status: design approved by reviewer-gpt; implementation may proceed.

## Problem

`ComposerDocument::formatted_body_with_options` emits Matrix HTML when a semantic mention is present. `format_markdown_subset_html` currently joins ordinary source lines with raw `\n`, which HTML layout collapses. The plain `body` remains correct, but Koushi and other Matrix clients render the formatted body as one line.

## Ownership and constraints

- Rust owns composer serialization, semantic mentions, and Matrix `formatted_body`.
- React remains a presentation-only renderer of sanitized formatted DTOs.
- Preserve the plain body byte-for-byte, mention anchors, `m.mentions`, lists, spoilers, code, and math blocks.
- Do not add CSS `white-space` repair or frontend Matrix semantics.

## Minimal design

Change only the shared Rust HTML serializer. Treat each ordinary non-empty source line and each existing list/math block as an output chunk with its source line range.

When joining chunks:

- ordinary line to ordinary line: emit one `<br>` for each authored newline;
- any boundary involving an existing block: retain one raw separator because the block boundary supplies one visible line break, and emit `<br>` only for additional authored blank lines;
- leading and trailing authored newlines around emitted content: emit matching `<br>` elements;
- contiguous list items and lines inside fenced display math remain owned by their existing block serializer and gain no synthetic internal breaks.

Line-break conversion alone does not cause plain multiline text to gain a `formatted_body`; it only makes HTML explicit when mentions or existing markdown/math formatting already require formatted output.

## Verify-first sequence

1. Add Rust regression assertions before changing production code:
   - mention + one newline;
   - mention + blank line;
   - leading/trailing/multiple newlines;
   - inline Markdown + newline;
   - edit-shaped replacement document with mention + blank line;
   - plain multiline text still produces no `formatted_body`;
   - exact ordinary↔list and ordinary↔math output in both directions, with and without an intervening blank line;
   - contiguous list items and internal display-math lines contain no synthetic `<br>`.
2. Run `cargo test -p koushi-state --test composer_document` and `cargo test -p koushi-state --test composer_semantics_state`; record the new assertions failing on raw newline output.
3. Add a focused `TimelineView.test.tsx` case proving the resulting formatted DTO renders explicit visible `<br>` nodes.
4. Implement the serializer change and rerun the same checks green.
5. Run desktop typecheck, lint, focused frontend tests, full crate/frontend/browser gates, then reviewer-gpt full-diff review before PR.

## Acceptance

- Plain `body` retains authored newline characters.
- Formatted output never relies on raw HTML whitespace for ordinary line breaks.
- One newline produces one visible line transition; two produce one visible blank line.
- Leading, trailing, and repeated newlines have deterministic tests.
- Plain multiline text without mentions or formatting still omits `formatted_body`.
- Mention identity/anchors and exact ordinary/list/math boundaries do not regress; list/math internals gain no `<br>`.
- The edit path inherits the same shared serializer behavior.

## Review record

- Design review round 1: Findings — add the plain-multiline `None` invariant and exact bidirectional list/math boundary coverage.
- Design review round 2: reviewer-gpt `Correct-to-merge`.
- Final diff review: required before PR creation; record the reviewer-gpt verdict in the PR body.
