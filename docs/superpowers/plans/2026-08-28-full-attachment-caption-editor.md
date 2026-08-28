# Full Attachment Caption Editor Plan

Date: 2026-08-28
Status: Implemented

## Goal

Replace the upload-staging caption's plain single-line input with the same shared editor used by the main/thread composer: multiline IME-safe editing, formatting toolbar, emoji, structured mentions, math formatting mode, and the Rust-resolved send shortcut. Keep the staging dialog's existing attachment preparation controls and Send button; attaching more files, replies, and scheduled send do not apply inside a caption editor.

## Design

1. Reuse `Composer` in `editorOnly` mode for every staged caption. Do not clone its editor, toolbar, emoji picker, mention autocomplete, keyboard resolver, or IME logic.
2. Keep the existing dialog Send button. Add one narrow optional Composer callback so unmodified forward Tab from a caption (after mention autocomplete handling) focuses that Send button, preserving the behavior established in PR #709.
3. Make `StagedUploadItem.caption` a Rust-owned `ComposerDocument | null` instead of a flattened `FormattedMessageDraft`. `update_staged_upload_caption` accepts the document, so toolbar markdown and mention identity survive snapshot acknowledgement and main/thread rerenders.
4. At the Tauri send boundary, derive the existing `UploadMediaRequest.caption: FormattedMessageDraft` from the authoritative staged `ComposerDocument` using its plain body, formatted body under the Rust-owned math setting, and mention intent. No media-send or Matrix event contract changes.
5. Keep the existing latest-caption mutation ordering and target fences. Browser Fake and Playwright harness mirror the new typed document argument; they do not become semantic owners.
6. Pass the already-owned main/thread mention candidates, query callback, shortcut resolver, room label, math setting, and math-setting callback into the staging dialog. Caption send remains attachment-only and never sends or clears the ordinary composer draft. Wire both shared-Composer send callbacks to attachments only when every staged item is sendable, preserving the current no-op while preparation is pending.
7. Normalize a whitespace-only caption document to `None` in production and Browser Fake, and settle caption updates by document equality rather than flattened plain-body equality.

## Scope

- Equivalent editing means multiline text, bold/italic/link/list/code toolbar, emoji, structured user/room mentions, math-mode formatting, IME behavior, and configured Enter/Mod+Enter semantics.
- Excluded because they are not caption editing: attach-another-file control, reply mode, scheduled send, and a second nested Send button.
- No new editor, parser, dependency, frontend product-state store, or speculative abstraction.

## RED→GREEN Evidence

- React: the staging dialog renders the shared editor/toolbar; formatting and mention selection emit a structured `ComposerDocument`; IME and send shortcut behavior remain shared; Tab reaches the existing Send button.
- Rust/Tauri: a staged document with markdown and a structured mention becomes one `FormattedMessageDraft` with formatted HTML and mention intent at media-send construction.
- Browser/headless: `update_staged_upload_caption` carries `document`, preserves it in the Rust-shaped snapshot, and attachment Send remains separate from normal text Send for main and thread targets.

## Review Gate

- Pre-implementation — `reviewer-flash`: Correct-to-merge. Implementation must preserve sendability gating and empty-document normalization/document-equality settlement.
- Post-implementation — `reviewer-flash`: Correct-to-merge.
- Post-Playwright layout follow-up — `reviewer-flash`: Correct-to-merge.

## Verification

```bash
cargo test -p koushi-state --lib
cargo test -p koushi-desktop --lib
npm --prefix apps/desktop test -- src/components/dialogs.test.tsx src/components/composer.test.tsx src/backend/client.test.ts src/backend/browserFakeApi.test.ts
npm --prefix apps/desktop run test:ui-headless -- e2e/composer-send-queue-upload.spec.ts
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
node scripts/check-ime-text-inputs.mjs
node scripts/check-sdk-submodule.mjs
cargo fmt --all -- --check
git diff --check
```
