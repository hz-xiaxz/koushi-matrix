# Room Re-entry Generation Boundary Implementation Plan

Date: 2026-07-31

Issues: #286, #287

## 1. Browser RED and observability

- Extend the TimelineView browser harness with test-only session-anchor,
  room-switch/remount, and diagnostic-log controls.
- Add a Playwright case that batches a live window with a large historical
  prepend during re-entry and proves the seeded stale anchor currently wins.
- Add assertions for one private-data-free restore-decision diagnostic.

Gate:

```bash
npx playwright test e2e/timeline-scrollback.spec.ts -g "stale session anchor" --workers=1
```

## 2. GUI fix

- When user input is pending, reconcile actual bottom state before discarding a
  scroll event as a programmatic echo.
- Add one-shot restore-decision diagnostics and age bucketing.
- Keep genuine free-scroll anchor restore and prepend compensation unchanged.

Gate:

```bash
npx playwright test e2e/timeline-scrollback.spec.ts --workers=1
npm run typecheck
```

Run from `apps/desktop`.

## 3. Core RED and relay fix

- Add a focused unit test that feeds the same superseded actor projection
  through several relay batches and asserts it is never attached, while a
  current-generation projection remains attached.
- Pass the owning actor generation into every `run_diff_relay` call.
- Filter projection tags before `relay_received=queued` and actor delivery.

Gate:

```bash
cargo test -p koushi-core --lib stale_prior_actor_gap_projection
cargo test -p koushi-core --lib timeline
```

## 4. Integrated verification and PR

- Run SDK guard, formatting, focused gates, desktop tests/typecheck/lint, exact
  CI workspace tests, secret scan, and diff checks.
- Read `git diff origin/main...HEAD` plus untracked status.
- Publish one non-draft PR closing #286 and #287; merge with a merge commit
  after CI is green.

