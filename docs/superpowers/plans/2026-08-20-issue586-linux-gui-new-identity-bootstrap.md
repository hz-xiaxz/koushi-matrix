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
- Use a per-run file under the OS temporary directory, not the artifact directory or app data directory.
- Fill both values through generic WebDriver DOM helpers.
- Set the attempt fence before submitting so unrelated events/retries cannot issue duplicate bootstrap commands.
- Wait for the authoritative `I saved the recovery key` control before confirming.
- Remove the recovery-key file in a `finally` block on success, timeout, or any failure. Never read or log its contents.
- The product already clears both DOM fields before dispatch; screenshots/evidence occur only after readiness.

## Change

In `scripts/desktop-linux-gui-qa/local-session.mjs`:

1. Add one private helper that, once per `waitForLocalLoginReady` call, detects the exact new-identity bootstrap form, generates the temporary destination/passphrase, fills the two labeled inputs, clicks `Create secure backup`, waits for and clicks `I saved the recovery key`, and removes the temporary key file in `finally`.
2. Call it only while the authoritative parsed title is `awaitingVerification`; otherwise preserve the existing readiness loop and deadline.
3. Keep the existing absolute timeout budget. The helper receives the same deadline or remaining time; it must not restart a fresh full timeout per phase.
4. Import existing generic DOM functions directly from `webdriver.mjs`; do not duplicate polling/input/click logic or introduce a new lifecycle owner.

No product Rust/React/Tauri behavior, DTO, command, state, token registry, or scenario name changes.

## Verification

- Before-fix container `local-send` failure retained as the red check for both immutable and split runners.
- Add focused source-contract coverage proving:
  - automation is gated by `awaitingVerification` plus the exact bootstrap form;
  - the attempt is fenced before click;
  - passphrase uses `randomBytes` and never console/env/args;
  - destination uses the OS temp directory and `rmSync(..., { force: true })` in `finally`;
  - only DOM helpers are used; no direct Tauri invoke or local readiness mutation.
- Run the release contract suite, typecheck/lint, secret scan, deterministic Linux module/probe checks, and Playwright gate.
- Run the containerized Tuwunel `local-send` lane to green and retain only private-safe evidence tokens.
- Run full repository gates once after formal full-diff review, then CI 7/7.
