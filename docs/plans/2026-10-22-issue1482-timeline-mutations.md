# #1482 — Timeline mutation liveness

Status: scoped mutation/publication implementation independently approved;
#1482 remains open for the outstanding acceptance below.
Canon consulted: REPOSITORY_RULES.md (ownership, state-machine discipline, review),
overview Async Design Rules, state-machine Live Signals, engineering rules
Async and Runtime, verification.md. Base: 58e805f0.

## Design

The session TimelineManagerActor owns a MutationLane supervisor with one retained
execution-owner task. That owner directly polls bounded SDK operation futures,
independently of manager or actor mailboxes. It sends no completion messages to
either mailbox; results use the existing event broadcast. Manager dispatch admits
EditText, Redact, SendReaction and RedactReaction directly. A global 64-operation
semaphore bounds both ingress and all retained work; each target accepts at most
eight queued operations plus one active. Admission never awaits channel capacity.

All four mutations share FIFO ordering within (TimelineKey, actor generation,
target event), preserving the existing per-view command ordering. This change
does not add a cross-view freshness guarantee: distinct SDK timeline controllers
still observe each other's mutations through the SDK queue/cache/sync. Revalidate
target content and reaction state on the worker's originating SDK controller
before execution, and report invalid state instead of blindly toggling. FIFO
between different views was not provided by the original independent actors and
is not claimed here. Own pending-reaction refusal is tested; explicit cross-view
refusal evidence remains outstanding.

The actor handle reuses its existing Matrix enqueue context to retain the SDK
Timeline, immutable originating actor generation and shared event-to-SDK-transaction
map populated by the actor send monitor. Manager routing builds MutationContext
with those values, the generation gate and AccountWorkScheduler; own user identity
comes from the originating SDK room. Sharing that map preserves edit/redact fallback before
remote echo; its synchronous lock is never held across an await. Worker execution
uses existing public SDK Timeline methods and existing SDK local echoes.

Each accepted operation has a drop-backed correlated settlement guard. Success
retains the existing diff-only result contract. Failures emit OperationFailed.
Capacity rejection is QueueOverflow; queued requests for invalid generations fail
NotSubscribed before starting. Active SDK futures continue to completion across
unsubscribe/replacement: dropping toggle_reaction_local would skip its rollback.
No gate watch cancels active execution. Retained SDK handles only affect their
original controllers, and existing actor generation fences exclude stale output.

A 30-second admission-relative client settlement deadline includes queued work.
Expired queued requests fail and release permits without SDK execution. Active
expiry emits one Sdk failure but does not drop
the SDK future: the target remains occupied until natural SDK completion, allowing
rollback and preventing an overtaking successor. No claim is made that the SDK's
unbounded retry policy has a finite cleanup time. A blocked target consumes one
of the 64 permits; capacity fails explicitly instead of accumulating work.

Shutdown first closes admission and fails queued requests; active work is still
polled by the independent owner while presentation actors are retired. Only after
all presentation actors are shut down does manager cancel and await the execution
owner. SDK futures then drop with discarded controllers, so no live presentation
can reuse an optimistically modified abandoned controller. Unexpected manager drop
aborts the retained owner as a fail-safe; all guard-owned requests settle on drop.
No detached tasks, shared semantic mutation cache or competing retry policy.

The actor select keeps cleanup/control priority but fairly selects ordinary
mailbox and relay data, preventing sustained diff traffic from starving pending
publication. Existing read/send workers remain intact. The independent execution
owner removes the worker-lock/full-mailbox cycle without wrapping every manager
await or changing manager lifecycle semantics.

PendingSendProjection carries an optional clone of its opaque SendLifecycleTrace.
The trace records coordinator insertion separately from actor publication, only
after a generation-fenced ItemsUpdated or InitialItems actually includes that
pending row. No renderer acknowledgement protocol is introduced: receipt remains
unobserved and is explicitly documented as such, rather than claimed from an
actor broadcast. Trace payloads contain only opaque correlation and safe tokens.

## Verification

Before implementation, add a real SDK/TimelineActor mock-server regression for
held remote reaction deletion and remote edit event fetch. Hold the HTTP response
behind a deterministic barrier, submit the mutation through production manager
routing, then assert pending ItemsUpdated and incoming diff publication before
release. Verify send-terminal progress too. Tests must be RED on the base.

Exercise FIFO and fresh target validation, independent targets, failure, capacity,
timeout, foreign/stale generation, duplicate completion, unsubscribe/replacement,
owner drop/shutdown. Cover active worker polling during manager inline waits,
full mailboxes and sustained relay traffic. Diagnostic tests verify distinct
insertion/publication with the same opaque trace and no premature/stale/duplicate
publication. Renderer tests assert Rust-shaped pending/diff handoff visibility.
Run focused checks then workspace/structural checks and disposable Tuwunel/Synapse
live_signals and send_queue_fast QA. Record commands, actual results and any
unmet acceptance. Independent design and final diff review required.

## Inventory

Existing #1481 inventory is the starting point. Complete the actor-handler
HTTP/store/lock inventory during implementation, including retry/cancel, source
loads, preview/media workers, gap/pagination and recovery handlers; record owners,
ordering and remaining limitations. This change does not attribute the original
October 16 delayed-row trace to one proven HTTP operation.

## Design review disposition

Initial independent review rejected manager-direct polling, eager cancellation,
and the proposed global cross-view freshness guarantee. Revision 2 uses an
independent retained execution owner, retains active SDK futures after client
timeout, retires presentation controllers before abort-and-await shutdown, and
preserves the existing per-view ordering scope. Coordinator insertion tracing
moves after actual insertion. Review revision 2 before implementation.

Independent revision-2 review: approved with admission-relative queued deadlines;
verify own local-pending reactions as well as remote reaction ids. Initial
rejected design and all dispositions remain above.

## Current actor wait inventory (2026-10-22)

| Handler family | Dependency / owner | Ordering and disposition |
| --- | --- | --- |
| EditText, Redact, SendReaction, RedactReaction | SDK controller/store locks; edit cache miss and remote reaction removal can issue HTTP | Move together to per-view/event FIFO, preserve SDK local echo and explicit failure |
| SetTyping | Existing session TypingLane | Room FIFO; unchanged |
| SendText/Reply/media | Existing manager enqueue supervisor + SDK send queue | Existing send terminal owner; unchanged |
| SendReadReceipt/FullyRead | Manager read worker supervisor | Existing read-key ordering/settlement; unchanged |
| RetrySend/CancelSend, queue reconciliation | SDK SendHandle/store locks and room queue subscribe | Local queue mutation; actor still awaits SDK/store; no direct outbound HTTP |
| DownloadMedia/LoadLinkPreviews | Actor-retained network tasks, typed completion | Existing per-event replacement/cancel; unchanged |
| Paginate / gap inspection / live tail | Retained pagination/gap/relay workers | Generation/correlation guarded; dispatch bookkeeping may await manager capacity |
| LoadMessageSource / history diagnostics | SDK controller read, key rotation/store diagnostics | Local/store awaits; no source event HTTP fetch in project_message_source_for_event |
| RequestRoomKey / recovery ticks | download_room_key_from_backup and request_room_key_for_event are awaited inside actor under retry deadline | Additional network-capable recovery path discovered; requires separate crypto-state-machine design, not certified responsive by this mutation slice |
| ForwardMessage | destination_room.send(...).await in handle_forward_message | Additional direct outbound send path discovered; distinct MessageForwarded result contract, not migrated or claimed covered by this mutation slice |
| Visibility/policy, canonical/read/thread publication, reply quote resolution | SDK/cache reads, reducer/manager channel reservation; reply quote fetch has retained owner | Local/store/backpressure waits remain; no global lock-wait liveness guarantee |

The recovery and forward findings extend the old #1481 inventory. Keep #1482
open until its full inventory and required automatable evidence are discharged;
the planned mutation correction must not be represented as removal of every
remaining HTTP-capable actor path. No crypto state-machine behavior changes are
authorized by a concurrency observation alone.

Additional reviewed correction: per-row `apply_link_previews_to_item` called
`latest_encryption_state` and could fetch state during an optimistic reaction
diff, blocking the actor even after mutation separation. Independent review
approved cached `encryption_state` projection, with Unknown/missing policy:
explicit room override or both globals true. Known state keeps existing policy.
Suppression updates on subsequent diff/replay/policy, not a new observer.

## Integrated review and evidence

Independent integrated review initially found two blockers: routing tagged an old
controller with the current gate generation, and publication diagnostics confused
generation acceptance with successful broadcast send. Both were fixed and covered
by regressions. Re-review approved the scoped diff with no blocking findings.

The no-receiver regression also reproduced replay dropping manager-owned pending
inputs. Replay now clones the existing display projection and rematerializes its
canonical window, preserving pending rows and suppressed late SDK echoes. Emission
outcomes keep generation acceptance/batch advancement separate from actual
broadcast success, so an absent receiver does not consume publication attribution.

Verify-first: on the base, held remote reaction removal and held edit preparation
both reached the real HTTP barrier and then failed pending-publication assertions.
Moving mutations exposed a second actor wait in per-row encryption projection;
the reviewed cached-state correction made both regressions pass. Current focused
suite: 13 passed, including held HTTP pending/diff/send-terminal progress, actual
SDK reaction rollback after client expiry, per-target FIFO, independent targets,
capacity, queued expiry, stale generations, teardown, formatting propagation,
own pending-reaction refusal, and opaque insertion/publication attribution.

Frontend full suite: 182 files / 2,252 tests passed. Focused renderer coverage uses
Rust-shaped DTOs to verify pending-row visibility, unrelated diff progress and
convergence to a single sent row. Typecheck, lint, structural checker and its 20
script tests, documentation checks, SDK gitlink validation, formatting and diff
whitespace checks passed.

Library suites: Core 1,752 passed / 11 ignored; SDK 296 passed / 1 ignored;
State 102 passed; desktop 278 passed / 1 ignored. The production runtime
`send_queue_fast` test target passed all 7 tests.

Local QA: Tuwunel and Synapse live_signals passed in both SDK and Core. Tuwunel and Synapse send_queue
reached send_fail/resend/fifo/cancel_send, then failed session restoration in
rechecking_trust with a network-class failure. This lane is not reported green;
it does not establish restart/persisted-send acceptance.

Outstanding #1482 acceptance: deterministic full-mailbox/SDK-lock progress and
sustained-relay fairness regressions; explicit cross-view refusal coverage;
send_queue restart/restore QA resolution; full additional HTTP inventory disposition
(recovery and forwarding above). Renderer receipt remains unobserved, as allowed
without a receipt channel. This scoped review does not certify those items or
justify closing the issue.

CI-profile all-target Core clippy and release Core clippy passed with warnings
denied; release also enabled both large-stack-frame and large-future lints.
Neither Rust SDK nor its gitlink changed.
