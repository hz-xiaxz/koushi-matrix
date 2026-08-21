# Implementation Plan Index

Which dated plan governs which area. Read the relevant plan before implementing
in that area; it is step 6 of the read order in [AGENTS.md](../../AGENTS.md).

Plans are historical once their phase ships — they record the intended sequence
and the deliberate limits of that phase, not the current contract. When a plan
and [state-ownership.md](state-ownership.md) disagree about today's behavior, the
code and the canon win; fix whichever document is wrong.

## Runtime and roadmap

- Headless core runtime:
  [2026-06-12-headless-core-runtime-implementation.md](../superpowers/plans/2026-06-12-headless-core-runtime-implementation.md)
- Phase 10+ product surface and release roadmap:
  [2026-06-13-roadmap-phases-10-18.md](../superpowers/plans/2026-06-13-roadmap-phases-10-18.md)
- Local GUI room/space/reply operations:
  [2026-06-13-local-gui-basic-operations.md](../superpowers/plans/2026-06-13-local-gui-basic-operations.md)

## Umbrella #12 — Core Batch A / GUI Batch B

Batch Rust-owned Phase A contracts first, then serialize the shared GUI surface,
then run the #9/#31 integration gate.

- Design/split:
  [2026-06-15-remaining-core-phase-a-batch-design.md](../superpowers/specs/2026-06-15-remaining-core-phase-a-batch-design.md)
- Implementation:
  [2026-06-15-remaining-core-phase-a-batch-implementation.md](../superpowers/plans/2026-06-15-remaining-core-phase-a-batch-implementation.md)

Before starting each new task in that batch, refresh open GitHub issues and apply
the plan's issue reconciliation addendum. New GUI-only presentation items such as
space tooltips do not bypass the Rust-owned Phase A rule for product behavior.

## Feature areas

Phase A is Rust/headless work and comes before Phase B GUI wiring.

| Area | Phase A | Phase B |
| --- | --- | --- |
| Media / file timeline | [2026-06-15-media-phase-a.md](../superpowers/plans/2026-06-15-media-phase-a.md) | — |
| Media preparation/cache retention (#547) | [2026-08-18-issue547-memory-bounds.md](../superpowers/plans/2026-08-18-issue547-memory-bounds.md) | [2026-08-18-issue547-memory-bounds.md](../superpowers/plans/2026-08-18-issue547-memory-bounds.md) |
| Muted-room native Dock attention (#543) | [2026-08-18-issue543-muted-dock-badge.md](../superpowers/plans/2026-08-18-issue543-muted-dock-badge.md) | [2026-08-18-issue543-muted-dock-badge.md](../superpowers/plans/2026-08-18-issue543-muted-dock-badge.md) |
| Rust lifecycle ownership / leak cleanup (#550) | [2026-08-18-issue550-rust-lifecycle-ownership.md](../superpowers/plans/2026-08-18-issue550-rust-lifecycle-ownership.md) | [2026-08-18-issue550-rust-lifecycle-ownership.md](../superpowers/plans/2026-08-18-issue550-rust-lifecycle-ownership.md) |
| Feature-seam decomposition (#551) | [2026-08-18-issue551-feature-seam-decomposition.md](../superpowers/plans/2026-08-18-issue551-feature-seam-decomposition.md) | [2026-08-18-issue551-feature-seam-decomposition.md](../superpowers/plans/2026-08-18-issue551-feature-seam-decomposition.md) |
| Remaining QA decomposition (#551) | [2026-08-20-issue551-remaining-qa-decomposition.md](../superpowers/plans/2026-08-20-issue551-remaining-qa-decomposition.md) | [2026-08-20-issue551-remaining-qa-decomposition.md](../superpowers/plans/2026-08-20-issue551-remaining-qa-decomposition.md) |
| SDK feature-seam decomposition (#551) | [2026-08-20-issue551-sdk-decomposition.md](../superpowers/plans/2026-08-20-issue551-sdk-decomposition.md) | [2026-08-20-issue551-sdk-decomposition.md](../superpowers/plans/2026-08-20-issue551-sdk-decomposition.md) |
| RoomActor feature-seam decomposition (#551) | [2026-08-21-issue551-room-actor-decomposition.md](../superpowers/plans/2026-08-21-issue551-room-actor-decomposition.md) | [2026-08-21-issue551-room-actor-decomposition.md](../superpowers/plans/2026-08-21-issue551-room-actor-decomposition.md) |
| AccountActor feature-seam decomposition (#551) | [2026-08-21-issue551-account-actor-decomposition.md](../superpowers/plans/2026-08-21-issue551-account-actor-decomposition.md) | [2026-08-21-issue551-account-actor-decomposition.md](../superpowers/plans/2026-08-21-issue551-account-actor-decomposition.md) |
| Timeline ownership decomposition (#551) | [2026-08-21-issue551-timeline-actor-decomposition.md](../superpowers/plans/2026-08-21-issue551-timeline-actor-decomposition.md) | [2026-08-21-issue551-timeline-actor-decomposition.md](../superpowers/plans/2026-08-21-issue551-timeline-actor-decomposition.md) |
| TimelineView message-body decomposition (#551) | [2026-08-21-issue551-timeline-view-message-body.md](../superpowers/plans/2026-08-21-issue551-timeline-view-message-body.md) | [2026-08-21-issue551-timeline-view-message-body.md](../superpowers/plans/2026-08-21-issue551-timeline-view-message-body.md) |
| TimelineView message-metadata decomposition (#551) | [2026-08-21-issue551-timeline-view-message-meta.md](../superpowers/plans/2026-08-21-issue551-timeline-view-message-meta.md) | [2026-08-21-issue551-timeline-view-message-meta.md](../superpowers/plans/2026-08-21-issue551-timeline-view-message-meta.md) |
| TimelineView receipt-surface decomposition (#551) | [2026-08-21-issue551-timeline-view-receipts.md](../superpowers/plans/2026-08-21-issue551-timeline-view-receipts.md) | [2026-08-21-issue551-timeline-view-receipts.md](../superpowers/plans/2026-08-21-issue551-timeline-view-receipts.md) |
| TimelineView media decomposition (#551) | [2026-08-21-issue551-timeline-view-media.md](../superpowers/plans/2026-08-21-issue551-timeline-view-media.md) | [2026-08-21-issue551-timeline-view-media.md](../superpowers/plans/2026-08-21-issue551-timeline-view-media.md) |
| TimelineView row decomposition (#551) | [2026-08-21-issue551-timeline-view-row.md](../superpowers/plans/2026-08-21-issue551-timeline-view-row.md) | [2026-08-21-issue551-timeline-view-row.md](../superpowers/plans/2026-08-21-issue551-timeline-view-row.md) |
| TimelineView transport contract decomposition (#551) | [2026-08-21-issue551-timeline-view-transport-contract.md](../superpowers/plans/2026-08-21-issue551-timeline-view-transport-contract.md) | [2026-08-21-issue551-timeline-view-transport-contract.md](../superpowers/plans/2026-08-21-issue551-timeline-view-transport-contract.md) |
| TimelineView virtualization-model decomposition (#551) | [2026-08-21-issue551-timeline-view-virtualization.md](../superpowers/plans/2026-08-21-issue551-timeline-view-virtualization.md) | [2026-08-21-issue551-timeline-view-virtualization.md](../superpowers/plans/2026-08-21-issue551-timeline-view-virtualization.md) |
| TimelineView event-projection classification (#551) | [2026-08-21-issue551-timeline-view-event-projection.md](../superpowers/plans/2026-08-21-issue551-timeline-view-event-projection.md) | [2026-08-21-issue551-timeline-view-event-projection.md](../superpowers/plans/2026-08-21-issue551-timeline-view-event-projection.md) |
| TimelineView anchor/session ownership (#551) | [2026-08-21-issue551-timeline-view-anchor-session.md](../superpowers/plans/2026-08-21-issue551-timeline-view-anchor-session.md) | [2026-08-21-issue551-timeline-view-anchor-session.md](../superpowers/plans/2026-08-21-issue551-timeline-view-anchor-session.md) |
| TimelineView projection commit boundary (#551) | [2026-08-21-issue551-timeline-view-projection-boundary.md](../superpowers/plans/2026-08-21-issue551-timeline-view-projection-boundary.md) | [2026-08-21-issue551-timeline-view-projection-boundary.md](../superpowers/plans/2026-08-21-issue551-timeline-view-projection-boundary.md) |
| TimelineView viewport observation (#551) | [2026-08-21-issue551-timeline-view-viewport-observation.md](../superpowers/plans/2026-08-21-issue551-timeline-view-viewport-observation.md) | [2026-08-21-issue551-timeline-view-viewport-observation.md](../superpowers/plans/2026-08-21-issue551-timeline-view-viewport-observation.md) |
| TimelineView subscription lifecycle (#551) | [2026-08-21-issue551-timeline-view-subscription-lifecycle.md](../superpowers/plans/2026-08-21-issue551-timeline-view-subscription-lifecycle.md) | [2026-08-21-issue551-timeline-view-subscription-lifecycle.md](../superpowers/plans/2026-08-21-issue551-timeline-view-subscription-lifecycle.md) |
| Timeline viewport scheduler teardown (#551) | [2026-08-21-issue551-viewport-scheduler-teardown.md](../superpowers/plans/2026-08-21-issue551-viewport-scheduler-teardown.md) | [2026-08-21-issue551-viewport-scheduler-teardown.md](../superpowers/plans/2026-08-21-issue551-viewport-scheduler-teardown.md) |
| Linux GUI new-identity bootstrap QA (#586) | [2026-08-20-issue586-linux-gui-new-identity-bootstrap.md](../superpowers/plans/2026-08-20-issue586-linux-gui-new-identity-bootstrap.md) | [2026-08-20-issue586-linux-gui-new-identity-bootstrap.md](../superpowers/plans/2026-08-20-issue586-linux-gui-new-identity-bootstrap.md) |
| Live signals (receipts, markers, typing, presence) | [2026-06-15-live-signals-phase-a.md](../superpowers/plans/2026-06-15-live-signals-phase-a.md) | [2026-06-15-live-signals-phase-b-gui.md](../superpowers/plans/2026-06-15-live-signals-phase-b-gui.md) |
| E2EE trust state machine | [2026-06-14-e2ee-trust-phase-a.md](../superpowers/plans/2026-06-14-e2ee-trust-phase-a.md) | — |
| Rust-owned settings | [2026-06-14-rust-owned-settings-phase-a.md](../superpowers/plans/2026-06-14-rust-owned-settings-phase-a.md) | — |
| i18n substrate | [2026-06-14-i18n-substrate-phase-a.md](../superpowers/plans/2026-06-14-i18n-substrate-phase-a.md) | [2026-06-14-i18n-substrate-phase-b.md](../superpowers/plans/2026-06-14-i18n-substrate-phase-b.md) |
| Cross-platform font/emoji substrate | [2026-06-15-font-emoji-phase-a.md](../superpowers/plans/2026-06-15-font-emoji-phase-a.md) | [2026-06-15-font-emoji-phase-b-gui.md](../superpowers/plans/2026-06-15-font-emoji-phase-b-gui.md) |
| Timeline navigation aids (#41) | [2026-06-16-timeline-navigation-phase-a.md](../superpowers/plans/2026-06-16-timeline-navigation-phase-a.md) | — |
| Account work scheduler | [2026-07-25-account-work-scheduler-phase-a.md](../superpowers/plans/2026-07-25-account-work-scheduler-phase-a.md) | — |
| Startup latency observability (#123) | [2026-06-23-startup-latency-observability-phase-a.md](../superpowers/plans/2026-06-23-startup-latency-observability-phase-a.md) | — |
| Initial index-0 key-share diagnostics (#509) | [2026-08-13-index0-share-diagnostics.md](../superpowers/plans/2026-08-13-index0-share-diagnostics.md) | — |
| Bounded index-0 duplicate share (#510) | [2026-08-13-index0-reshare.md](../superpowers/plans/2026-08-13-index0-reshare.md) | — |
| Initial Megolm Olm-claim repair (#523) | [2026-08-14-initial-megolm-olm-repair.md](../superpowers/plans/2026-08-14-initial-megolm-olm-repair.md) | — |
| Element X Megolm send parity (runtime-disable #510/#523) | [2026-08-15-element-x-megolm-send-parity.md](../superpowers/plans/2026-08-15-element-x-megolm-send-parity.md) | — |
| Room-subscription ownership (#518) | [2026-08-14-room-subscription-ownership.md](../superpowers/plans/2026-08-14-room-subscription-ownership.md) | — |
| Session-resident room subscriptions (#532) | [2026-08-15-room-subscription-residency.md](../superpowers/plans/2026-08-15-room-subscription-residency.md) | — |
| Room-key rotation correlation diagnostics | [2026-08-14-room-key-rotation-correlation-diagnostics.md](../superpowers/plans/2026-08-14-room-key-rotation-correlation-diagnostics.md) | — |
| Eviction-resistant Megolm rotation attribution (#591) | [2026-08-21-issue591-rotation-ledger.md](../superpowers/plans/2026-08-21-issue591-rotation-ledger.md) | [2026-08-21-issue591-rotation-ledger.md](../superpowers/plans/2026-08-21-issue591-rotation-ledger.md) |
| Formatted-body newline preservation (#522) | [2026-08-14-formatted-body-newlines.md](../superpowers/plans/2026-08-14-formatted-body-newlines.md) | — |
| Active prepend anchor preservation (#520) | [2026-08-14-active-prepend-anchor.md](../superpowers/plans/2026-08-14-active-prepend-anchor.md) | — |

Font asset loading and any bundled font package must update
`THIRD_PARTY_NOTICES.md` with version, local path, license, and provenance — see
[state-ownership.md](state-ownership.md#settings-composer-and-scheduled-send).
