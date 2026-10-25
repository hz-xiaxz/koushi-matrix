# Remove idle event receivers

## Scope and design

Consulted REPOSITORY_RULES.md, AGENTS.md, architecture overview, engineering
Async and Runtime rules, and verification guidance. A command/snapshot-only
connection must not pin historical broadcast payloads. The previous connection
constructor subscribed unconditionally; both the desktop command cache and
account snapshot watchers retained receivers they never read.

The architecture contract was amended before implementation and independently
reviewed read-only: explicit non-subscribing connections were approved, with
normal `attach()` remaining eager. No lazy subscription or drain task is added.
CoreConnection holds an optional receiver; an absent receiver uses the existing
closed-stream receive behavior. Commands, request IDs, watch snapshots, leases
and native-artifact correlation retain their existing connection ownership.

## Production paths and review

- CoreRuntime and AccountRuntimeManager expose explicit attachments without
  events. Desktop cached command connections and account snapshot watchers use
  these; account-tab snapshot enumeration also avoids pointless subscriptions.
- Selected event forwarding, request-outcome/OIDC waiters and native-attention
  sound delivery retain eager subscriptions before command submission.
- Diagnostics report `cached_command_receiver_status=not_subscribed` for the
  command cache, distinctly from an unavailable/busy sample.
- No Matrix state machine, SDK fork, event capacity, timeline eviction policy,
  frontend UI or user workflow changes. Existing help instructions still apply.

## Verification

RED: a test using the actual SelectedCoreConnection cache observed one receiver
where zero was required, before the implementation. Core tests cover eager
receive before the first read, release after real consumers drain while command
and snapshot connections remain alive, commands/watch convergence with zero
event receivers, and fail-fast closed outcome waiting without lazy subscription.
Desktop tests cover cache and watcher subscription absence, request IDs, and
diagnostic distinction between absent subscriptions and unavailable samples.

Independent integrated audit approved the complete diff with no blocking
findings. Combined Rust library suites passed: Core 1767, desktop 280, SDK 297,
state 103; existing ignored tests remain ignored. Core/desktop Clippy including
tests passed with warnings denied. Rustfmt, SDK submodule, Rust test structure,
adapter boundary, command snapshot, agent-docs, user-help and diff guards passed.
Local packaging and installation are recorded in the final change report.

The fix removes this particular retention path; it does not claim all memory
growth is resolved or trim unrelated caches.
