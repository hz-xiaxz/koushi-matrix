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

Existing validation limitations: the Rust structure checker reports eight
findings outside updater surfaces; the Windows agents-docs checker reports
path normalization/retired-scenario failures; user-help --check reports a
missing Using Koushi index. Protected signing environment provisioning,
signed packaging, old-to-new installation confirmation and SignPath approval
remain pending. Do not merge or publish based only on the focused checks.
The final Windows Rust library run passed 214/216 tests, including all eight
new updater checks. Two existing native-attention tests failed because their
runtime setup requires HOME, which is absent on this Windows host.
