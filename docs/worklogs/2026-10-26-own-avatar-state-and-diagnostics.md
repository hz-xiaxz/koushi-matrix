# Own avatar state preservation and diagnostics

Consulted REPOSITORY_RULES.md, AGENTS.md, architecture overview, Profiles And
Avatars state-machine contract, and verification guidance. The branch also
contains the separately reviewed event-retention diagnostics and idle receiver
fix documented by their worklogs. The temporary graphics/cache experiment is
excluded; this PR introduces no cache purge, Inspector, or new retry policy.

## Reproduction and scope

A synthetic reducer test demonstrates that same-MXC own-profile metadata
refresh replaces Loading/Ready/Failed with NotRequested. Unlike other profile
merges, the own profile did not preserve thumbnail state. This can discard
known state independently of whether the remote profile image exists. The
reported historical disappearance was not captured with these new diagnostics;
this defect must not be presented as its conclusively established cause.

Both OwnProfileUpdated and correlated ProfileUpdateSucceeded now preserve only
the previous own avatar's exact same-MXC state when incoming state is
NotRequested. Changed/removed sources and explicit incoming states still win;
stale request completions remain inert. Other profile copies cannot override
this merge. The state-machine canon was amended before implementation; the
independent design review approved the narrow merge and observation-only scope.

## Diagnostics and production paths

- CoreConnection borrows the published own profile at diagnostic export time
  without cloning AppState or creating an event subscriber. The desktop adapter
  samples every account via non-event connections. Closed state/failure tokens
  and non-touching retained-byte presence probes contain no MXCs, handles,
  identities, image data, or raw errors. Published state and cache presence are
  explicitly non-atomic observations.
- Renderer reports include its current own-avatar DTO state separately, plus
  saturating page-lifetime load/error event totals from EntityAvatar's existing
  recoverable-image hook. Failed-image fallback and unmount do not erase these
  totals. They are events across all hook users, not unique images or proof of
  an own-avatar failure. No per-source maps or event/DOM references are retained.
- These changes preserve existing download, timeout, eviction, and retry owners.
  There are no new controls or edited property workflows. Help documentation
  explains capturing the report before restart; no settings locations moved.

## Verification record

RED: the same-source preservation test failed with NotRequested in place of
Loading before the reducer fix. GREEN covers Loading/Ready/Failed, both reducer
paths, conflicting other-profile copies, source change/removal, explicit
incoming-state precedence, and stale request IDs. Core diagnostic tests cover
all coarse states/failure kinds, privacy, missing/present bytes, no event
subscriptions, and no LRU promotion. Renderer tests verify event totals survive
fallback/unmount and formatted reports omit synthetic private values.

Final gates passed: Core library 1769 (11 existing ignored), desktop library
280 (one ignored), SDK library 297 (one ignored), state library 103, and the
profile_state integration target 33. The final avatar diagnostic tests passed
2/2, including the LRU eviction assertion. Frontend passed 2293 tests across
186 files, typecheck, ESLint/semantic/IME guards, and production build. Core,
state, and desktop Clippy passed all targets with warnings denied. SDK, test
structure, Tauri adapter, command snapshot, retained-reachability, agent-docs,
user-help, tracked-file secret scan, Rustfmt, and diff checks passed.
Independent integrated review approved the completed changes with no blockers;
its only follow-up was to record the already-passing integration-test target.
Formatting in the temporary worktree explicitly used default Rustfmt settings,
because an unrelated ancestor-directory Rustfmt config otherwise applied.
