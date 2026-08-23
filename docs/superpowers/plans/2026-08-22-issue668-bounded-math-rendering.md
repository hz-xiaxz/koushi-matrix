# Issue #668 Bounded Math Rendering

## Scope and decision

The production path is the Rust-owned `TimelineItem.formatted.html` snapshot,
which is tokenized and rendered by
`apps/desktop/src/components/timeline/TimelineMessageBody.tsx`. A sanitized
`<span data-mx-maths="...">` or `<div data-mx-maths="...">` reaches the shared
`renderMathFormula` helper, which synchronously calls KaTeX.

Admit at most **1024 JavaScript UTF-16 code units per expression**, after the
existing `trim()` and with the boundary inclusive. An expression with
`source.length > 1024` is not sent to KaTeX; it uses the existing plain-child
fallback in the same inline (`span`) or display (`div`) container. This is an
admission limit, not a message-body truncation limit. The guard lives in the
shared helper so the inline and display call sites cannot diverge.

Accepted expressions call KaTeX 0.18.1 with explicit finite limits and the
existing safety/compatibility options:

```ts
{
  displayMode,
  strict: false,
  throwOnError: false,
  trust: false,
  maxExpand: 1000,
  maxSize: 20
}
```

The explicit `maxExpand` avoids relying on a package default; `maxSize` closes
the current `Infinity` default. An empty source and a KaTeX exception retain
the current text fallback behavior. `data-mx-maths` and
`dangerouslySetInnerHTML` are used only on the accepted, successfully rendered
branch.

## Root cause and evidence

- `apps/desktop/src/components/timeline/TimelineMessageBody.tsx` currently
  trims the attribute and calls `katex.renderToString` for every non-empty math
  node without a source-length check or explicit `maxExpand`/`maxSize`.
- Both inline `span` and display `div` renderers route through that helper, so a
  visible attacker-controlled/decrypted message can synchronously consume the
  WebView main thread and create a large HTML string during a render.
- `apps/desktop/src/App.test.tsx` proves the existing ordinary
  `E=mc^2`-shaped path emits KaTeX markup; it is the compatibility regression
  guard and should remain green.
- The installed dependency is KaTeX `^0.18.1`. The parent benchmark used the
  installed 0.18.1 implementation with `maxExpand=1000` and `maxSize=20`:

  | flat ASCII source | median render time | generated HTML |
  | ---: | ---: | ---: |
  | 257 chars | 3.8 ms | 41 KB |
  | 513 chars | 4.0 ms | 82 KB |
  | 1025 chars | 5.8 ms | 164 KB |
  | 4097 chars | about 44 ms after warm-up variance | 656 KB |

The 1024 boundary keeps ordinary short formulas and the measured 1025-character
case out of the accepted workload, while avoiding the much more expensive
multi-thousand-character region. It is deliberately more permissive than a
small UI token limit: normal equations, fractions, sums, and short aligned
expressions remain eligible without changing their output.

## Resource bound and sufficiency decision

For approximately 20 visible attacker messages, the bound is 20 independent
admission decisions. Twenty over-limit expressions make **zero** KaTeX calls;
twenty accepted expressions cannot exceed the per-expression source, expansion,
and size limits. Using the nearest measured 1025-character row as a conservative
proxy, the accepted-work envelope is about 20 x 5.8 ms = 116 ms median
synchronous work and 20 x 164 KB = 3.28 MB of generated HTML on the benchmark
machine. These are measured planning estimates, not a portable wall-clock or
memory SLA; the hard contract is the finite input/expansion/size admission.

**Finite KaTeX options plus the 1024-unit admission cap are sufficient for this
issue. Do not add caching, `React.memo`, or a worker.** The cap prevents the
unbounded parser/input path, `maxExpand` bounds macro expansion, and `maxSize`
bounds KaTeX's size checks. `React.memo` cannot protect an initial render or a
changed message, a cache would retain plaintext-derived HTML and still pay the
first-render cost, and a worker would turn this synchronous React-node path into
an asynchronous ownership/stale-result problem. Add scheduling, caching, or a
worker only after a separate benchmark shows that this finite accepted workload
still causes a reproducible regression and a privacy/ownership design exists.

This does not claim to bound an unlimited number of visible rows, arbitrary
plain-text message size, or one event that splits its legal payload across many
separately admitted formulas. Timeline virtualization, a per-message aggregate
math budget, and any general message-size policy are separate concerns; this
change bounds each expensive KaTeX expression and fully blocks the reported
approximately 20-row attack shape because each reported formula is over the
per-expression cap. Add an aggregate budget only if a separate deterministic
reproduction proves that many individually admitted formulas remain a product
regression.

## Verify first: deterministic RED

Add the focused test before the production guard. The test is deterministic and
must not use timing thresholds.

**New test file:**
`apps/desktop/src/components/timeline/TimelineMessageBody.test.tsx`

Use `renderFormattedBody` with synthetic sanitized HTML and
`renderToStaticMarkup`. Spy on the imported KaTeX object with
`vi.spyOn(katex, "renderToString")`; restore the spy after each test. No real
account text, identifiers, or raw diagnostics are needed.

The initial RED should assert all of the following:

1. An inline `<span data-mx-maths="${"x".repeat(1025)}">...</span>` produces
   visible fallback text, has no KaTeX markup, and
   `renderToString` was **not called**. The baseline fails because it calls
   KaTeX.
2. A display `<div data-mx-maths="...">` uses the same no-call behavior for an
   oversized source; this proves the shared guard covers both call sites.
3. An exact-boundary inline source of `"x".repeat(1024)` is admitted: the spy is
   called exactly once, KaTeX markup is present, and `data-mx-maths` is retained.
   A variant padded with outer whitespace proves the limit applies after the
   existing `trim()`. This distinguishes the required `> 1024` guard from an
   incorrect `>= 1024` guard.
4. Small ordinary `E=mc^2` expressions are accepted through both inline `span`
   and display `div` paths. Each emits `message-math` plus KaTeX markup, retains
   `data-mx-maths`, and calls KaTeX with `displayMode: false` or `true`
   respectively.
5. The accepted calls' options contain `maxExpand: 1000`, `maxSize: 20`,
   `strict: false`, `throwOnError: false`, and `trust: false`. The baseline
   fails because the two finite options are absent.
6. A short source such as `\\rule{1000em}{1000em}` runs through the real
   production/KaTeX path with spy call-through. Assert deterministically that
   the generated markup does not contain KaTeX's observed unclamped giant
   dimension literal and is capped at the installed 0.18.1 representation for
   20em. Capture the baseline markup first so the RED assertion is specific and
   non-vacuous; if KaTeX falls back instead of clamping, stop and revise the
   design rather than accepting an absence-only assertion.
7. A deterministic 20-expression fixture uses 20 copies of an approximately
   2950-unit synthetic flat source matching the reported attack shape, plus a
   separately rendered small admitted control. The attack fixture must produce
   visible fallback text and exactly zero KaTeX calls. Use a sentinel spy
   implementation for the RED count fixture so the pre-fix test cannot spend
   time rendering all oversized inputs; no render-time benchmark assertion is
   added.

The RED command is:

```bash
npm --prefix apps/desktop test -- src/components/timeline/TimelineMessageBody.test.tsx
```

Record the test's own non-zero exit status before editing the production helper.
The GREEN run uses the same command and the same assertions. The existing
`apps/desktop/src/App.test.tsx` formatted-body test remains an ordinary-formula
compatibility check rather than being weakened or replaced.

## Minimal implementation

After the RED:

1. Add one local numeric source-length constant and one local finite-options
   constant (or equivalent literal) in
   `apps/desktop/src/components/timeline/TimelineMessageBody.tsx`.
2. In `renderMathFormula`, trim once, preserve the empty fallback, then reject
   `source.length > 1024` before entering the KaTeX `try` block. Return the
   existing child fallback with the existing inline/display class.
3. Pass `maxExpand: 1000` and `maxSize: 20` alongside the existing options.
4. Leave the Rust sanitizer, formatted-body tokenizer, KaTeX CSS, output shape
   for accepted formulas, and existing catch fallback unchanged.

No other production file, dependency, generated contract, Rust DTO, or QA
scenario is needed for this renderer-local fix. The only future implementation
files are:

- `apps/desktop/src/components/timeline/TimelineMessageBody.tsx`
- `apps/desktop/src/components/timeline/TimelineMessageBody.test.tsx`

This design document is the only file changed in the current design-only turn.
`apps/desktop/src/App.test.tsx` is an existing regression guard and is not
modified by the implementation plan.

## Security and privacy

- Math attributes originate in decrypted Matrix content. The length check is a
  fail-closed parser admission boundary, before any KaTeX work or generated
  HTML allocation.
- Keep `trust: false`; do not permit macros/commands to escape KaTeX's trusted
  subset. Keep the Rust-owned sanitized formatted-body boundary authoritative;
  this helper is not a sanitizer.
- Do not log, count, hash, diagnose, cache, persist, or telemetry-report the
  source, rendered HTML, room/event identifiers, or message body. A KaTeX
  exception remains an identifier-free silent text fallback.
- The no-cache decision avoids creating a second first-party store for
  plaintext-derived HTML. The formula exists only as current visible UI state,
  consistent with the repository privacy rules.
- Synthetic `example.invalid`-style data is sufficient for tests; no real
  account or homeserver QA data may enter fixtures, snapshots, logs, or
  screenshots.

## Accessibility and compatibility

- Under the cap, ordinary formulas retain the current inline/display tags,
  KaTeX CSS, source marker, and rendered semantics. `strict: false` and
  `throwOnError: false` preserve the current malformed-formula behavior.
- Over-limit formulas remain visible as their sanitized child text in the same
  `span`/`div` shape rather than disappearing or becoming an empty visual
  placeholder. Text remains selectable and available to assistive technology;
  no focusable control, tooltip, or new untranslated warning is introduced.
- Do not truncate the fallback text in this change: truncation would be a
  separate message-body/accessibility policy. The cap controls KaTeX admission,
  not authored content retention.
- No direction, line-breaking, CSS, localization catalog, or keyboard behavior
  changes are required.

## Non-goals

- No general Matrix message/body byte limit, truncation, server validation, or
  change to Rust formatted-body sanitization.
- No TeX grammar rewrite, macro allowlist, KaTeX fork, dependency upgrade, or
  replacement renderer.
- No cache, memoization, worker, debounce, retry loop, or new renderer state.
- No timeline virtualization change or promise of a bound for an unlimited
  number of visible rows.
- No new telemetry, diagnostics, user-facing copy, or accessibility widget.
- No changes to the static `apps/desktop-shell` demo, which does not use this
  KaTeX timeline path.

## Gates and acceptance mapping

| Acceptance requirement | Evidence and exact gate |
| --- | --- |
| Oversized source is never handed to KaTeX | Focused spy test in `TimelineMessageBody.test.tsx`; RED then GREEN with `npm --prefix apps/desktop test -- src/components/timeline/TimelineMessageBody.test.tsx`. |
| Both inline and display math are bounded | The same focused test exercises `span` and `div` over-limit nodes and asserts zero calls plus visible fallback text. |
| Strict cap is 1024 UTF-16 units and boundary is intentional | Focused exact-1024 admission test and source review of the shared helper; no duplicated per-tag guards. |
| KaTeX limits are finite and stable | Spy options assertion requires `maxExpand: 1000`, `maxSize: 20`, `trust: false`, `throwOnError: false`, `strict: false`, and the correct display mode; a real call-through giant-rule test proves the installed KaTeX clamps the rendered dimension. |
| Ordinary formulas keep working | Existing `apps/desktop/src/App.test.tsx` KaTeX markup assertion plus focused accepted `E=mc^2` inline/display assertions; run `npm --prefix apps/desktop test -- src/App.test.tsx`. |
| Approximately 20 visible attacker expressions cannot invoke unbounded KaTeX work | Deterministic 20-node fixture matching the reported approximately 2950-unit-per-expression shape asserts visible fallback and exactly zero KaTeX calls; the measured benchmark envelope is rationale, not a flaky latency gate. |
| No privacy or dependency regression | No source/body diagnostics or cache; synthetic fixtures only; run `npm --prefix apps/desktop run qa:secret-scan` and inspect the finished diff. |
| Frontend remains valid | Run, reading each command's own exit status: `npm --prefix apps/desktop run typecheck`, `npm --prefix apps/desktop run lint`, `npm --prefix apps/desktop run build`, `npm --prefix apps/desktop test`, and `npm --prefix apps/desktop run test:ui-headless`. |
| Repository integration remains green | Run the applicable local headless lane `npm --prefix apps/desktop run qa:headless-local -- --server=both` after the focused/frontend gates; no Rust or Matrix contract files are changed by this plan. Also run `git diff --check`. |

Implementation must not start until this plan's design review is accepted. The
implementation review must inspect the exact two-file allow-list, the RED/GREEN
evidence, the cap/options assertions, and the privacy/accessibility contract;
no timing-only or visual-only result substitutes for the focused test.

## Review record

- Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`. Required exact
  1024-unit admission evidence, real giant-rule clamp behavior, accepted display
  mode coverage, an exact 20-node attack fixture, and the many-admitted-node
  residual decision. The design was amended to cover all five findings.
- Round 2, `reviewer-flash-opencode-go`: `Correct-to-merge`. No blocking
  findings remained; implementation may begin under the exact RED/GREEN and
  two-production-file allow-list above.

## Implementation evidence

- RED: the final six-test focused file against the pre-fix helper exited 1 with
  five failures and one preservation pass; over-limit inputs reached KaTeX and
  finite option/clamp assertions failed.
- GREEN: the unchanged focused command passed 6/6. Combined focused App/math
  tests passed 84/84, full Vitest passed 1,453 tests across 87 files, and
  typecheck, lint, build, secret scan, and `git diff --check` passed.
