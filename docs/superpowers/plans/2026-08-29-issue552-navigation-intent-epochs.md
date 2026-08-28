# Issue #552 navigation intent epochs

Status: implemented, locally verified and approved by exact-final-diff review; pending PR/CI.

## Scope

Phase 4.4 reconciles `roomNavigationRequestRef` and `spaceNavigationRequestRef`. This is a naming/documentation and evidence leaf: no Rust, SDK, Tauri, IPC/DTO or navigation behavior changes.

## Ownership decision

Retain and rename them to `roomNavigationIntentEpochRef` and `spaceNavigationIntentEpochRef`.

They are renderer-specific intent owners, not duplicate Matrix/product state:

- user intent exists before `drainActiveComposerScopesForNavigation` completes;
- an older click may finish its async renderer drain after a newer click;
- Rust cannot reject that old intent because its command has not yet been submitted;
- the epochs prevent stale pre-submit continuations, returned promises and dialog/profile follow-ups from applying;
- Rust owns every submitted SelectRoom/SelectSpace request, navigation state and projected terminal.

The Space epoch is the broader Space/Home view-intent lifetime and is also read by Space settings, invite cancellation and role-update view fences. The room epoch owns room selection and room settings/profile follow-ups. Cross-navigation invalidation advances both when the active context changes. Same-panel close/reopen does not.

## Deterministic evidence

Use deferred promises and no sleeps:

1. rapid room A then B, resolve B then A: B remains selected and A is not applied/logged as committed;
2. rapid Space A then B while renderer drains yield: only B reaches `selectSpace`, then B remains selected;
3. preserve existing late room result after newer Home/Space navigation and late DM settings/profile tests;
4. source contract requires the `IntentEpoch` names, documents pre-submit drain ownership, and rejects the old `RequestRef` names;
5. focused tests confirm settings/cancel/role consumers read the renamed Space/room intent epoch without semantic changes.

## Implementation

1. Rename both refs and all consumers exactly.
2. Add the renderer-intent ownership comment at declaration.
3. Add focused A/B tests and source contract.
4. Update ownership canon, inventory, umbrella plan and index.

## Rejected alternatives

- Delete the refs: stale pre-submit drain continuation can submit old intent after the latest click; Rust has no evidence to distinguish it.
- Move renderer composer-drain intent into Rust: crosses the renderer/Rust ownership boundary and is unnecessary.
- Merge both into one generic epoch: room and Space/Home lifetimes have distinct consumers and invalidation rules.
- Add a generic request manager: no benefit over two counters.

## Local verification evidence

- focused App/navigation: 139/139;
- full Vitest: 1513/1513;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs and production build: passed;
- secret scan, Tauri adapter boundary, SDK submodule sync, diagnostic isolation and domain-crate platform guards: passed.

No Rust/Tauri/SDK source or contract changes; the exact PR head runs the complete Rust/Tauri/QA/dependency CI matrix.

## Acceptance

- Every retained navigation epoch has a single documented renderer-intent purpose.
- Rapid A/B and cross-navigation stale results are deterministic and latest-intent safe.
- Rust remains sole owner after command submission.
- No behavior, accessibility, privacy, IPC/DTO or browser/Tauri parity change.
