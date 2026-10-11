# Windows signed updater implementation

Contracts consulted: REPOSITORY_RULES.md, architecture overview Desktop
Application Updates, desktop update state machine, verification discipline,
desktop release runbook, and the proposed SignPath code-signing policy.

Independent design review identified Tauri updater 2.11.0's Windows
`std::process::exit(0)` after installer launch. Windows must transfer the
artifact to the shutdown coordinator and finish successful Core cleanup before
invoking the installer; it cannot reuse macOS's install-before-cleanup ordering.
NSIS owns relaunch. Failed cleanup suppresses installation; installation
failure after cleanup ends in ordinary exit.

Packaging generates NSIS exe and sig directly, preserves both platform entries
in stable/beta manifests, and publishes Windows assets in the moving beta
channel. A protected release-windows environment requires the existing offline
updater keypair. No secrets are accessed by this implementation task.
Authenticode remains pending SignPath approval.

Verification: Windows key-guard test and multi-platform manifest test were
observed failing before implementation and then passing. Lifecycle notification
download rejection was reproduced as a failing Rust test. Shutdown ordering,
cleanup failures, duplicate transfer and installer failure have focused checks.
Final verification results are recorded in the implementation PR.

Independent integrated review found no remaining functional/security blocker;
its cfg and platform-contract findings were corrected. Windows desktop clippy
with all targets and warnings denied passed. Typecheck, 35 focused frontend
tests, manifest tests, configuration preflight and SDK synchronization passed.
The source-map-js high advisory was repaired to 1.2.2. Windows secret scanning
required fileURLToPath to resolve the repository root and now passes.

Integration validation (2026-10-11): main was integrated, preserving ignored
updates, refresh failure state, and notification-only release links together.
A failing lifecycle regression reproduced loss of the notification capability
and link after refresh failure; the corrected projection passes. All 242 Linux
Rust library tests, 42 focused frontend/config tests, typecheck, lint, Rust
structure, SDK synchronization, agents docs, user-help and release configuration
checks pass. Windows CI now exercises updater and shutdown ordering tests.

The protected release-windows environment is provisioned, as recorded in
issue #1339 comment 6105903661. Its public key matches release-macos and its
secrets were loaded from the offline backup. Presence is separate from actual
signed packaging evidence. The optional Windows trial workflow validates an
installer with disposable keys, including tampering and wrong-key rejection,
without production secrets, uploads, release writes or tag changes.
Production-key packaging, old-to-new installation confirmation and SignPath
approval remain separate release follow-ups.
