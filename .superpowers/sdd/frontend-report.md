# Secure Backup Gate frontend report

Status: DONE_WITH_CONCERNS

Implemented the React/TypeScript portion of Koushi #462/#463. Startup admission now requires a ready secure-backup gate before exposing the shell; once the shell has been exposed, runtime degradation preserves the read-only shell, drafts, receiving, local decryption, diagnostics, and logout while pausing encrypted sending. The gate mirrors the Rust-shaped tagged DTO, including typed failures and snake_case pending buckets.

The gate uses the existing `SessionVerificationGate`, IME-safe controls, injected secure operations, and dedicated client methods. Recovery uses an uncontrolled `SecureImeTextField`. Setup and explicit re-enable share one passphrase/native artifact flow: production invokes the existing Tauri save dialog pattern, while tests inject `chooseSecureBackupDestination`. The selected filesystem path is held only in a ref until the command call; the UI renders only selected/not-selected status and never renders or logs the path. Explicit re-enable retains the account-wide warning for other Matrix clients.

## TDD evidence

Recorded failing checks before implementation:

- Initial secure-gate focused tests: 6 failures in 27 tests.
- Initial catalog/browser-fake checks: 7 failures.
- Dedicated client and secure re-enable correction: 2 failures across 3 focused files, including missing client methods and missing re-enable setup fields.
- Native destination-selector acceptance: 3 failures across 2 focused files before replacing the typed path input.

Passing checks after implementation:

- `npm --prefix apps/desktop test -- --run`: 69 files, 1,271 tests passed.
- `npm --prefix apps/desktop run typecheck`: passed.
- `npm --prefix apps/desktop run lint`: passed, including IME-safe text-input and AGENTS documentation checks.
- `git diff --check -- apps/desktop/src`: passed.

## Files changed

- `apps/desktop/src/App.tsx`: startup/runtime gate admission, runtime banner/composer lock, native destination picker wiring, and dedicated secure operations.
- `apps/desktop/src/SessionVerificationGate.test.tsx`: focused checking, recovery clearing/masking, setup, explicit confirmation, selector flow, upload, failure/retry, diagnostics, and no-shell coverage.
- `apps/desktop/src/backend/browserFakeApi.ts` and `.test.ts`: required gate fixtures and dedicated browser operations.
- `apps/desktop/src/backend/client.ts` and `.test.ts`: `recover_secure_backup`, `bootstrap_secure_backup`, and `retry_secure_backup_inspection` invoke paths; setup and re-enable share bootstrap semantics with passphrase plus destination.
- `apps/desktop/src/domain/types.ts`: required secure gate DTO and `SNAPSHOT_SCHEMA_VERSION = 4`.
- `apps/desktop/src/domain/coreEvents.generated.json`, test harness, IPC mock, and affected snapshot tests: schema 4 updates and required ready gate fixtures.
- `apps/desktop/src/i18n/messages.ts` and `.test.ts`: bilingual gate, selector, failure, runtime, and account-wide warning copy.
- `apps/desktop/src/components/panes.tsx`, `apps/desktop/src/styles.css`, and diagnostics coverage: runtime composer restriction/banner styling and integration tests.

## Commit

Frontend implementation commit: `d132b9fe` (`feat(desktop): add secure backup gate UI`).

## Concerns

- Rust/Tauri source and unrelated backend/vendor changes were intentionally left untouched. The TypeScript client targets the existing command names, but a native Tauri build/runtime invocation was not part of the frontend-only verification.
- The report is committed as a follow-up report-only commit after the implementation commit above.
