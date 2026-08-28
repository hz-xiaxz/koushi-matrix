# Issue #552 staged-caption mutation sequencing ownership

Status: implemented, locally verified and approved by exact-final-diff review; pending PR/CI.

## Scope

Phase 5B reconciles the main/thread `caption:*` lanes of `latestTextMutationQueueRef`. It changes no Rust, SDK, Tauri, IPC/DTO or caption behavior unless evidence disproves retention.

## Ownership decision

Retain these lanes as documented renderer-specific dirty-editor submission/result owners.

Rust remains sole owner of staged-upload items, caption DTOs, target residency, removal, prepared variants and send content. React owns the mounted caption editors and the order in which dirty input intents cross the async command boundary:

- caption edits originate in IME-safe mounted inputs before command submission;
- Tauri `update_staged_upload_caption` waits until Rust projects the requested caption, so serializing started operations preserves editor intent order across the terminal boundary;
- concurrent invokes could allocate/submit in a different scheduling order than input intent, and Rust has no editor revision in the command;
- browser fake snapshots share one generation, so returned-result order cannot be recovered by app-store admission alone;
- per-item keys include main/thread target identity and staged id;
- clear/send/remove/target replacement invalidates the exact lane so a removed item cannot be resurrected by a late snapshot;
- the keyed queue skips superseded not-yet-started edits, applies only the latest result, and deletes settled/invalidate state.

Deleting the lanes safely would require a caption-specific editor revision carried through Rust command admission and correlated terminal, plus browser parity. That is unnecessary while this bounded renderer owner is explicit and no caption product state is duplicated.

## Deterministic evidence

1. Queue A/B/A with a real main-caption key serializes started work, skips superseded pending work and applies only the latest result.
2. Main and thread caption keys run independently.
3. Exact invalidation suppresses a late result after staged-item removal/clear.
4. Rejected started writes do not block the latest queued caption.
5. App source contract requires both caption API calls to be inside keyed `applyLatestTextMutationSnapshot` lanes and requires invalidation in main/thread clear/removal paths.
6. Existing dialog tests preserve dirty captions until Rust acknowledgement and IME composition/selection.

## Implementation

1. Add ownership comments at main/thread caption mutation functions.
2. Strengthen queue tests with concrete main/thread keys and rejection/invalidation evidence.
3. Add App source-contract coverage for both queued funnels and invalidators.
4. Update ownership canon, inventory, umbrella plan and index.

## Rejected alternatives

- Delete the lanes and trust Rust target/staged-id checks: those prove residency, not editor-intent order.
- Trust snapshot generation: browser generations are equal and intent precedes submission.
- Add a generic Rust mutation framework: violates the family-specific migration rule.
- Add caption revisions now: no demonstrated defect justifies a wire/state expansion.

## Local verification evidence

- focused App/queue/dialog caption contracts: 111/111;
- full Vitest: 1516/1516;
- Playwright DOM tier: 263/263;
- typecheck, lint/IME/docs and production build: passed;
- secret scan, Tauri adapter boundary, SDK submodule sync, diagnostic isolation and domain-crate platform guards: passed.

No Rust/Tauri/SDK source or contract changes; the exact PR head runs the complete Rust/Tauri/QA/dependency CI matrix.

## Acceptance

- Rust exclusively owns staged caption product state and send semantics.
- Renderer queue ownership is limited to mounted-editor pre-submit/terminal result ordering.
- Main/thread target keys and removal invalidation are explicit and bounded.
- No stale result can restore a removed item or older caption.
- Phase 5 text-mutation sequencing is fully reconciled after Phase 5A + 5B.
