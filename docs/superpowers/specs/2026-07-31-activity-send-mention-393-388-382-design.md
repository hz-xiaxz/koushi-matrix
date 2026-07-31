# Activity, Send Lifecycle, and Edit Mentions Design

Status: approved for implementation on 2026-07-31.

## Goal

Implement issues #393, #388, and #382 from origin/main while preserving Rust ownership, privacy-safe diagnostics, and the existing interactive-send guard contract.

## #393 Activity

ActivityState gains a Rust-owned session-scoped remembered tab. A fresh session defaults to Recent; ActivityTabSelected updates the remembered tab; closing does not discard it; and opening uses it. An open request is idempotent while Opening or Open, so it does not restart streams or replace a newer tab selection. Request IDs continue to fence stale asynchronous open completion.

ActivityProjection keeps a deterministic global Recent window of 200 rows, sorted by timestamp descending, room ID ascending, and event ID ascending. Rows needed for current event-backed Unread state, fully-read marker comparisons, or an in-flight mark-read reconciliation are retained outside that window. Other observed history and obsolete cleared-event tombstones are pruned after authoritative state settles.

Open/tab selection and projection pruning emit count-only diagnostics. No message content or Matrix identifiers enter diagnostic fields.

## #388 outgoing send lifecycle

Each outgoing send receives an internal generated correlation token and a private-safe lifecycle trace. Trace records are emitted at lifecycle boundaries from submission acceptance through SDK enqueue, local echo, worker/encryption/network stages where available, SDK terminal observation, SendCompletionCoordinator binding, and interactive guard release.

The trace carries only send kind, elapsed durations, queue/scheduler counts, coarse outcome/error classes, and terminal-delivery mode. It never contains message text, room/event/user IDs, raw transaction IDs, or credentials. The existing priority-0 guard remains owned by the pending send and is released only after terminal settlement.

## #382 mention-aware edits

Inline edit uses the existing room-scoped mention candidate projection and the same popup/key/IME behavior as the normal composer. The edit transport carries a typed MentionIntent in addition to the visible body. The target event's effective mention set seeds the edit draft; adding, retaining, and removing mentions updates the intent without inferring semantics from display text in Core.

Core constructs Matrix edit content from the typed intent. Text-like edits produce the correct top-level revision mentions and complete m.new_content.m.mentions; media-caption edits preserve the complete original attachment and apply the same two-level mention semantics. The path covers supported text/notice/emote messages and audio/file/image/video captions from both main and thread timelines, including encrypted and unencrypted events.

## Verification

Each issue starts with a failing reducer/core/UI test, then the minimal implementation turns that test green. Focused Rust and TypeScript tests run after each area, followed by repository contract gates, typecheck/lint, and the relevant browser-headless scenarios. The final branch diff is read including all new files before publishing.
