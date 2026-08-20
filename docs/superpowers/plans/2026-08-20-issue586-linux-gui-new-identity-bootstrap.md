# Issue #586 Linux GUI new-identity bootstrap automation

## Verify-first evidence

The documented containerized Tuwunel `local-send` lane fails after login at:

```text
session=awaitingVerification sync=stopped errors=0
```

`waitForLocalLoginReady` times out because the runner waits only for `session=ready`. Replacing the split entrypoint temporarily with the immutable pre-decomposition runner from merge `1adf0d565695bd767bc609ada1b97dfd33aad9d0` reproduces the identical state and timeout. The Issue #551 move is therefore not causal.

The `signed-out` lane remains green with auth-screen, title, screenshot, DBus, and window-state evidence.

## Ownership and security contract

Rust remains the sole owner of session and verification-gate state. The Linux runner may only drive the same DOM controls a user sees and then wait for authoritative QA-title state.

Only a new-identity bootstrap offer may be automated:

- the title reports `session=awaitingVerification`;
- the real DOM exposes `Recovery key destination`, `Backup passphrase`, and `Create secure backup` controls.

An existing-identity gate without that bootstrap form remains fail-closed and times out; the runner must never synthesize readiness, invoke Tauri directly, submit a recovery secret, or auto-approve SAS.

The bootstrap passphrase and generated recovery key are secrets:

- Generate the passphrase in the runner with `randomBytes`; never put it in CLI args, environment variables, process output, error messages, QA title, screenshots, or retained state.
- Create a unique per-attempt directory with `mkdtempSync` under `os.tmpdir()` and choose a child destination path that does not exist. Never touch or pre-create the file: the product deliberately uses `create_new(true)`, refuses overwrite/symlink targets, and creates the recovery-key file itself with mode 0600.
- Track the temporary directory on the local-session object. Remove it in the helper `finally`, and have `cleanupLocalGuiScenario` best-effort reap any tracked directory as a second cleanup net.
- Fill both values through generic WebDriver DOM helpers.
- Set the attempt fence before submitting so unrelated events/retries cannot issue duplicate bootstrap commands.
- Wait for the authoritative `I saved the recovery key` control before confirming.
- Remove the recovery-key file in a `finally` block on success, timeout, or any failure. Never read or log its contents.
- The product already clears both DOM fields before dispatch; screenshots/evidence occur only after readiness.

## Change

In `scripts/desktop-linux-gui-qa/local-session.mjs`:

1. Add one private helper that, once per `waitForLocalLoginReady` call, detects the exact new-identity bootstrap form, creates the unique temporary directory and non-existing destination, generates the passphrase, fills the two labeled inputs, clicks `Create secure backup`, waits for and clicks `I saved the recovery key`, and removes the temporary directory in `finally`.
2. Let `waitForLocalLoginReady` receive the local-session object (rather than only its browser) so the one session owner tracks bootstrap temp directories and teardown can reap them. Update callers mechanically.
3. Call the helper only while the authoritative parsed title is `awaitingVerification`; otherwise preserve the existing readiness loop and deadline.
4. Keep the existing absolute timeout budget. Form detection and both clicks use only the remaining time; no phase restarts a fresh full timeout.
5. Fence before field fill/submit. If the product rejects bootstrap and returns to `awaitingVerification`, suppress any second attempt and let the existing readiness deadline produce the existing fail-closed timeout; do not replace it with a retry or a mid-flow secret-bearing error.
6. Import existing generic DOM functions directly from `webdriver.mjs`; use `elementCount` plus `xpathLiteral` for exact form detection and existing field/click helpers for actions. Do not duplicate polling/input/click logic or introduce a new lifecycle owner.

No product Rust/React/Tauri behavior, DTO, command, state, token registry, or scenario name changes.

## Verification

- Before-fix container `local-send` failure retained as the red check for both immutable and split runners.
- Add focused source-contract coverage proving:
  - automation is gated by `awaitingVerification` plus the exact bootstrap form;
  - the attempt is fenced before click;
  - passphrase uses `randomBytes` and never console/env/args;
  - destination is a non-existing child of a unique `mkdtempSync` directory, with recursive forced removal in helper `finally` and session teardown;
  - only DOM helpers are used; no direct Tauri invoke or local readiness mutation.
- Run the release contract suite, typecheck/lint, secret scan, deterministic Linux module/probe checks, and Playwright gate.
- Run the containerized Tuwunel `local-send` lane to green and retain only private-safe evidence tokens.
- Run full repository gates once after formal full-diff review, then CI 7/7.
