# Issue #551 runtime residual composition-root audit

Status: audit review pending. This document decides whether the split-later `runtime.rs` ownership candidate is complete after PRs #617–#624.

## Measured result

- Audit base: `8a303ac548c044a2e147f9ed50380ef988cd834a`.
- `runtime.rs`: 6,474 newline-delimited lines / 278,570 bytes / SHA-256 `db5be13aafa038b2cd896a4e5117df74932cc67bd3a452a74d98a6a8fdcb1c21`.
- Original approved roadmap baseline: 10,909 newline-delimited lines / 441,487 bytes.
- Residual reduction: 4,435 lines (40.7%).
- Private runtime leaves: Activity1,303; connection665; profile/display737; composer641; navigation593; scheduled-send137; reducer/deferred529. Runtime tree total: 11,079 lines.
- Residual inventory: 25 production top-level identities plus two cfg-only test types, 54 impl/trait methods, 53 inline tests (one qa-bin-gated).
- Runtime tree inventory: 92 production identities, 113 methods and 82 all-feature / 81 default unit tests.
- Production definitions end before the inline test composition at approximately line 4,162; the parent AppActor implementation remains the dominant production composition root.

## Delivered ownership seams

| PR | Owner | Result |
| --- | --- | --- |
| #617 | Activity projection | mutable Activity cache remains one AppActor-owned projection; pure resolution/cache logic moved |
| #618 | connection transport/admission | public transport façade, lag/snapshot/submit behavior and attach moved with paths preserved |
| #619 | profile/display diagnostics | pure privacy-safe label/receipt/native-attention projections moved |
| #620 | composer lifecycle | fail-closed permit, reconciliation, encrypted persistence and debounce helpers moved atomically |
| #621 | navigation support | persistence, focused acknowledgement and replacement cleanup calculations moved |
| #622 | scheduled sends | encrypted persistence, deadline and one-item local dispatch moved atomically |
| #623 | reducer/deferred support | one instrumented reducer gateway and ordered cross-domain persistence moved |
| #624 | AccountActor routing | sole encrypted user-content admission classifier left runtime for its secure-backup route owner |

Every PR received design review, full-diff review, deterministic exactness checks, full local gates, CI7/7 and merge evidence. Public CoreRuntime/CoreConnection paths, CoreCommand/CoreEvent/AppState/wire shapes and resource owners remained compatible.

## Residual resource owner

`AppActor` has 28 source fields / 27 production fields:

- bounded command and action receivers;
- cfg-only composer mutation receiver;
- broadcast event sender and snapshot watch sender;
- authoritative `AppState`, settings/store ports and AccountActor handle;
- composer lease registry, lease-change watch, rejection sender/receiver, load status, pending persistence and pending acceptance map;
- navigation/scheduled-send/room-preference load markers and navigation persistence status;
- Activity projection;
- state, Activity-resolution, internal-request and navigation-projection generations;
- `pending_select: HashMap<String, VecDeque<RequestId>>` plus focused/date pending slots.

`AppActor::run` owns exactly six select arms:

1. composer persistence timer;
2. scheduled-send timer;
3. composer lease changes;
4. rejected composer permits;
5. command inbox with command batching;
6. action inbox with reducer/effect/persistence/publication ordering.

There are exactly two timers, one authoritative state owner, one shutdown owner and one publication point per processed batch. Splitting `run` would require nearly every field, duplicate cleanup or a callback/registry façade.

## Central exhaustive registries

`handle_command` remains exhaustive over six `CoreCommand` families:

- App53 variants;
- Account54 production / 56 with QA;
- Sync3 production / 4 test;
- Room41;
- Timeline33;
- Search4.

`account_command_projected_action` centrally catalogs 43 production variants that create speculative actions and 11 that intentionally do not; both QA-only variants intentionally do not. `is_verification_gate_command` is a closed 13-command provisional-session allowlist.

`AppEffect` has 43 variants:

- `handle_app_effects` actively dispatches25 and explicitly leaves18 to other owners;
- `handle_post_projection_effects` actively dispatches12 and suppresses31 to prevent duplicate Matrix/store/sync side effects;
- UI/search-crawler effects remain a separate actor-projection path.

These are static/exhaustive registries, not feature owners. Splitting them would require partial matches, delegation wrappers or duplicated origin firewalls, violating the roadmap.

## Source-contract coupling

Nineteen inline residual tests inspect `runtime.rs` source/order. Two external tests also inspect it (`runtime_intent_lifecycle.rs`, `search.rs`), while one connection-local test characterizes the runtime tree. The persistence source test reads each owner file independently; no source concatenation exists.

This coupling is intentional for actor-loop/effect/order contracts. Moving residual methods solely to shorten the parent would multiply owner-file plumbing without establishing ownership.

## Rejected further seams

- **Actor loop:** owns all fields, six select arms, timer polling, batching, permit cleanup, publication and shutdown.
- **Command subdomains:** no pre-existing movable identities remain below `handle_command`; extraction needs wrappers and distributes exhaustive admission.
- **Effect dispatchers:** two complete 43-variant origin-specific firewalls must remain visible and distinct.
- **Room preferences:** the small load/save/session trio is bidirectionally coupled to the parent UI/effect registries; a file adds reverse calls for roughly 50 lines.
- **Activity resolver:** mutates authoritative state/generation, invokes reducer/UI effects and routes AccountActor work; it does not belong in the pure Activity leaf.
- **Admission diagnostics/catalogs:** verification allowlist, account speculative projection, privacy suppression and space-member rollback belong beside exhaustive command ownership.
- **Space-member rollback mapper:** deliberately shared by AppActor and AccountActor; relocation only moves a shared catalog or adds a façade.
- **Test hooks:** span CoreRuntime startup, channels, stores, AppActor fields and AccountActor handles; extraction widens private fields/constructors.
- **Public façade/startup:** `CoreRuntime::start_inner` constructs all channels, actors, stores, watches and two abort-on-drop task handles; it is the composition root.
- **Small utilities:** search scope/data-directory helpers are runtime-boundary setup; separate files would be line-count-only fragmentation.

The final two-identity encrypted-content classifier was the only remaining clean cross-owner edge and was delivered in #624.

## Cohesion decision

The residual is one cohesive runtime composition root:

- public runtime startup/attach/shutdown façade;
- one AppActor resource graph and select loop;
- one exhaustive command/admission registry;
- two intentionally distinct exhaustive effect firewalls;
- shared request/generation/correlation fences;
- one state publication boundary;
- runtime-bound setup and source contracts.

No further move-only seam avoids wrapper APIs, reverse dependencies, field widening, callback registries, duplicated ownership or effect/order changes. Static registries remain centralized by design. The runtime split-later candidate should be marked complete after an unconditional formal reviewer verdict and merge of this evidence.

## Verification evidence

All eight delivered PRs passed their focused checks and full repository matrix. The final audit branch is documentation-only; run agents-doc lint, rustfmt/diff checks and the repository fast/full gates required before merge. Latest `origin/main` and PR base must match.

## Review gate

- Read-only post-#623 audit found one final cross-owner edge; #624 delivered it with exactness2/2 and CI7/7.
- Formal `reviewer-flash` residual verdict pending.
