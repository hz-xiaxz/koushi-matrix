# Diagnostics stay attachable: privacy note, file export, and issue templates (2026-10-13)

The diagnostics dialog could only copy the report to the clipboard. The report
runs to hundreds of lines, the dialog never said whether it is safe to share, and
the repository had no issue templates, so reports arrived without version,
platform, reproduction steps, or any diagnostics to work from.

Canon consulted: `REPOSITORY_RULES.md` ("Security Rules", "User-Facing Text And
Localization", "GitHub Issue Language", "Documentation And Work Records"),
`docs/policies/engineering-rules.md` ("Logging and Diagnostics", "Property
Display And Editing" 1), and `docs/architecture/i18n.md` (catalog ownership,
placeholder preservation, Japanese catalog completeness).

## Change

- `apps/desktop/src/components/dialogs.tsx`: the diagnostics dialog states the
  private-data guarantee next to its actions and offers **Save to file…** beside
  **Copy diagnostics**. Both actions report in-flight, done, and failed states in
  an inline `role="status"` line instead of failing silently; a dismissed native
  save dialog returns the line to rest rather than claiming success.
- `apps/desktop/src/i18n/messages.ts`: five catalog ids in both catalogs —
  `diagnostics.privacyNote`, `diagnostics.save`, `diagnostics.saveFailed`,
  `diagnostics.saved`, and `diagnostics.saving`. The note is product text, so it
  lives in the catalog, not in the component.
- `apps/desktop/src-tauri/src/commands/diagnostics.rs`: new
  `save_diagnostics_report` command. The adapter owns the native save dialog,
  offers `koushi-diagnostics.txt`, defaults to the platform Downloads directory,
  and writes the renderer-built report verbatim. A dismissed dialog writes
  nothing; a write failure returns a fixed token that names no path. The report
  is private-data-free by construction (structured kinds, counts, and lengths),
  which is what the dialog now tells the user and why it may cross the command
  boundary as text. Registered in `apps/desktop/src-tauri/src/lib.rs`.
- `.github/ISSUE_TEMPLATE/`: `bug_report.yml` (version, platform, steps, and the
  diagnostics file attached rather than pasted, with the guarantee repeated),
  `feature_request.yml`, and `config.yml` (blank issues disabled, links to the
  user guide, troubleshooting page, and releases).
- `docs/help/troubleshooting.md`: the bug-report steps now say to attach the
  saved `koushi-diagnostics.txt` when the problem involves sign-in, startup,
  sync, encryption, or a crash.

No canon amendment was needed: the change adds no Matrix semantics, no state
machine, and no new QA scenario or token.

## Verification

- GREEN `apps/desktop/src/components/DiagnosticDialog.test.tsx` (8 tests): the
  note renders in both locales without placeholder leftovers, save reports
  `diagnostics.saved`, a dismissal returns to rest, a failure reports
  `diagnostics.saveFailed`, and copy still writes the full report.
- GREEN `apps/desktop/src/backend/client.test.ts` (40 tests, including the new
  `save_diagnostics_report` argument shape),
  `desktopApi.contract.test.ts` (3), `i18n/messages.test.ts` (24), and
  `App.test.tsx` (90).
- GREEN `cargo test -p koushi-desktop --lib export_tests` (3 tests): the report is
  written byte-for-byte, a write failure keeps the destination out of the error
  text, and the default file name stays plain text.
- GREEN `scripts/check-rust-test-structure.mjs`,
  `scripts/check-tauri-adapter-boundary.mjs`,
  `scripts/check-command-snapshot-contract.mjs`,
  `scripts/check-agents-docs.mjs`, `scripts/user-help.mjs --check`, and ESLint
  over the touched files.
- Not covered by automation: the dialog's visual fit and the native save
  dialog's Downloads default on macOS, Windows, and Linux. Manual inspection is
  confirmation, not correctness evidence.
