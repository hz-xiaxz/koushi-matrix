# Issue #648 Nested Markdown Bullet Lists

## Scope

Preserve authored unordered-list nesting in the Rust-owned composer `formatted_body`. Structural HTML equivalence is required; Element's cosmetic newlines are not. Do not replace the existing Markdown subset parser, add a dependency, broaden syntax support, or address the separately reported fenced-code/escape/ordered-list gaps.

## Contract

- `- ` and `* ` remain the only recognized list markers.
- Leading ASCII spaces determine nesting within one contiguous unordered-list block.
- A greater indentation than the current item opens one nested `<ul>` inside that item's still-open `<li>`.
- Equal indentation creates a sibling item.
- Lesser indentation closes nested lists/items until the nearest previously opened indentation level; an unmatched outdent clamps to that nearest enclosing level instead of inventing an orphan list.
- The first list item's indentation defines the block root, preserving existing recognition of an indented first item.
- Inline Markdown and HTML escaping continue through `push_inline_markdown_subset` for every item body.
- Plain body, mention semantics, block-boundary `<br>` behavior, and flat-list output remain unchanged.

Expected structural output:

```text
- A
  - B
```

```html
<ul><li>A<ul><li>B</li></ul></li></ul>
```

Two-space and four-space indentation are structurally equivalent for this two-level case. Byte-exact Element whitespace is intentionally not copied.

## Verify first

Add focused Rust integration cases before production changes:

1. RED for two-space nesting.
2. RED for four-space nesting.
3. Nested siblings followed by a root sibling close tags in the correct order.
4. Three-level nesting and outdent to a previously opened level converge correctly.
5. Unmatched between-level outdent clamps to the nearest opened outer level: `- A\n    - B\n  - C` makes C a root sibling, not a newly invented nested level.
6. An indented first item seeds the block root: equal-indented following items are siblings, while greater indentation nests relative to that root.
7. Inline formatting/escaping inside nested items remains safe.
8. Existing flat-list and block-boundary cases remain byte-identical.

Run `cargo test -p koushi-state --test composer_semantics_state`, then the relevant crate tests and formatting check.

## Implementation

Replace `unordered_list_item_body` with the smallest parser returning `(leading_space_count, body)`. Render the contiguous list with an indentation stack while keeping each current `<li>` open until its nested list or sibling boundary is known. Reuse the existing inline renderer and block-boundary code. No AST, recursive document model, generalized Markdown parser, or new public API.

## Gates

- `reviewer-flash-opencode-go` design verdict: v1 `Not correct-to-merge` for missing unmatched-outdent and indented-root RED cases; v2 `Correct-to-merge` after both were added, with no remaining blocking findings.
- `luna-implementer` at max thinking for verify-first implementation.
- RED: 22 passed / 7 failed before production changes. GREEN: focused `composer_semantics_state` 29 passed, `koushi-state --lib` 39 passed, and `cargo fmt --check` exited 0 under parent rerun.
- `reviewer-flash-opencode-go` reviewed the exact 162-line implementation patch and returned `Correct-to-merge` with no blocking findings.
- Integrated full local matrix, CI, merge, issue evidence, and build-artifact cleanup in the shared PR.
