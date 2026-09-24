# Issue #998: degraded-network implementation record

Canon consulted: `REPOSITORY_RULES.md`, `docs/architecture/overview.md`,
`docs/architecture/state-machine.md`, and
`docs/policies/engineering-rules.md`. The architecture, state-machine, and
user-guide text are updated in this change.

The SDK adapter now preserves classified secure-backup inspection errors.
Account core retains an established send gate after inconclusive probes,
deduplicates backup state notifications, and inspects routinely every 30
minutes. The timeline manager distinguishes SDK enqueue from remote terminal,
releases the matching composer after enqueue, builds reply relations locally,
and re-enables only recoverably failed room queues on bounded backoff. Room
core owns pinned-event fetch workers and fences stale results. The Tauri
selection command completes after the room selection is published. Space
member-hydration enqueue is actor-owned and no longer blocks a saturated
observer mailbox.

Verification on the pinned SDK revision:

- `cargo test -p koushi-sdk --lib`: 152 passed.
- `cargo test -p koushi-core --lib`: 1148 passed, 9 ignored.
- Focused backup tests after observer deduplication: 26 passed.
- `cargo test -p koushi-state`: passed.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib`: 185 passed.
- Desktop Vitest: 1497 passed; typecheck and lint passed.
- Disposable Tuwunel Core `send_queue` QA: `send_fail`, `resend`, `fifo`,
  `cancel_send`, and `unsent_restart` passed.
- Disposable Tuwunel Core `reply` QA: `reply`, `reply_quote`, `pin_event`,
  `pinned_state`, and `unpin_event` passed.

Network-drop and pinned-fetch latency injection
remain valuable follow-up coverage before claiming every condition in #998's
full acceptance matrix.
