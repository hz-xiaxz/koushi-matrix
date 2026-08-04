# Re-enable Device-to-Device SAS Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-enable production SAS emoji verification while retaining the existing mandatory instability warning, then validate and merge the change.

**Architecture:** Delete the frontend-only build flag introduced by #370 and derive availability directly from Rust-owned `VerificationGateState.methods`. Reuse the existing dialog, commands, session states, and emoji renderer without changing SDK/Core protocol behavior.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, Playwright, Rust/Tauri, GitHub Actions

## Global Constraints

- Do not add a dependency, a replacement feature flag, or a second availability policy.
- Do not change SDK/Core SAS state machines, command contracts, cleanup semantics, or existing English/Japanese warning copy.
- Keep recovery-key verification primary when available and require explicit confirmation before SAS starts.
- Show no-recovery guidance only when recovery, bootstrap, and SAS are all unavailable.
- Preserve untracked `HANDOFF.md` and all unrelated behavior.

---

### Task 1: Make Rust-projected SAS availability the production policy

**Files:**
- Modify: `apps/desktop/src/SessionVerificationGate.test.tsx:17-220`
- Modify: `apps/desktop/src/App.tsx:774-789,1036-1047,1122-1144`
- Modify: `apps/desktop/playwright.config.ts:39-53`
- Modify: `AGENTS.md:1286-1328`

**Interfaces:**
- Consumes: `session.gate.methods: VerificationMethodCapability[]`, including `existingDeviceSas`.
- Produces: `deviceVerificationAvailable: boolean` derived solely from `methods.includes("existingDeviceSas")`; no new public interface.

- [x] **Step 1: Replace disabled-default assertions with failing enabled-default contracts**

Remove `enableDeviceVerificationForTest()` and environment stubbing. Replace the first three tests with contracts equivalent to:

```tsx
test("production requires warning confirmation before starting device verification", async () => {
  const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
  snapshot.state.domain.session = {
    kind: "awaitingVerification",
    user_id: "@u:example.invalid",
    homeserver: "https://example.invalid",
    device_id: "D",
    gate: {
      methods: ["existingDeviceSas", "recoveryKey"],
      account_kind: "existingIdentity"
    }
  };
  const startOwnUserSas = vi.fn(async () => snapshot);
  render(
    <SessionVerificationGate
      snapshot={snapshot}
      onSnapshot={() => undefined}
      onSignOut={() => undefined}
      operations={{ startOwnUserSas, submitRecovery: async () => snapshot }}
    />
  );

  fireEvent.click(screen.getByRole("button", { name: "Verify with another device" }));
  expect(startOwnUserSas).not.toHaveBeenCalled();
  const dialog = screen.getByRole("dialog", { name: "Try device verification?" });
  expect(within(dialog).getByText(/can be unreliable/)).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Try device verification anyway" }));
  expect(startOwnUserSas).toHaveBeenCalledTimes(1);
});

test("production renders the Rust-owned seven-emoji SAS comparison", async () => {
  // Reuse the existing verifying snapshot with seven emojis.
  // Assert seven `.session-verification-emojis span` nodes and the
  // “They match”, “They do not match”, and “Cancel” buttons.
});

test("SAS-only availability is actionable instead of a no-recovery dead end", async () => {
  // Use awaitingVerification with methods: ["existingDeviceSas"].
  expect(screen.getByRole("button", { name: "Verify with another device" })).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "No recovery key available" })).toBeNull();
});
```

Remove every `enableDeviceVerificationForTest()` call from later SAS tests. Keep `afterEach(cleanup)`; no environment cleanup remains.

- [x] **Step 2: Run the focused test and verify RED**

Run:

```bash
npm --prefix apps/desktop test -- --run src/SessionVerificationGate.test.tsx
```

Expected: FAIL because the production-default SAS button and emoji comparison are hidden and SAS-only state still renders no-recovery guidance.

- [x] **Step 3: Remove the frontend feature gate with the smallest production change**

Delete `deviceToDeviceVerificationEnabled()`. Replace the gated values with:

```tsx
const deviceVerificationAvailable = methods.includes("existingDeviceSas");
const sasVerifying =
  session.kind === "verifying" && session.method === "existingDeviceSas";
```

Change no-recovery guidance to:

```tsx
{awaiting &&
  !canUseRecoverySecret &&
  !deviceVerificationAvailable &&
  !methods.includes("bootstrap") && (
    <div className="gate-no-recovery">
      <h2>{t("gate.noRecoveryKeyTitle")}</h2>
      <p>{t("gate.noRecoveryKeyCopy")}</p>
    </div>
  )}
```

Keep the existing confirmation dialog and SAS action handlers unchanged.

- [x] **Step 4: Delete obsolete test/build configuration**

Remove `webServer.env.VITE_KOUSHI_ENABLE_DEVICE_VERIFICATION` and its #370 comment from `apps/desktop/playwright.config.ts`. Confirm no source reference remains:

```bash
rg -n "VITE_KOUSHI_ENABLE_DEVICE_VERIFICATION|deviceToDeviceVerificationEnabled|enableDeviceVerificationForTest" apps/desktop AGENTS.md
```

Expected: no matches.

- [x] **Step 5: Update durable policy notes**

Rename the AGENTS section to `Device-to-Device Verification Is Warning-Gated` and record:

- Rust `gate.methods` is the only availability owner.
- The action always opens the instability warning before dispatch.
- Recovery is recommended when present; SAS-only remains actionable.
- Seven emoji and match/mismatch/cancel remain Rust-snapshot-driven.
- Existing cleanup and privacy rules remain unchanged.

Remove statements that SAS is disabled, that tests opt in, and that recovery is the only path.

- [x] **Step 6: Run focused GREEN checks**

Run each command directly and record its exit status:

```bash
npm --prefix apps/desktop test -- --run src/SessionVerificationGate.test.tsx
(cd apps/desktop && npx playwright test e2e/session-verification-gate.spec.ts --workers=1)
npm --prefix apps/desktop test -- --run src/i18n/messages.test.ts
```

Expected: all pass; Playwright exercises the default production policy without a build-time opt-in.

- [x] **Step 7: Review and commit the implementation**

```bash
git diff --check
git diff -- apps/desktop/src/SessionVerificationGate.test.tsx apps/desktop/src/App.tsx apps/desktop/playwright.config.ts AGENTS.md
git add apps/desktop/src/SessionVerificationGate.test.tsx apps/desktop/src/App.tsx apps/desktop/playwright.config.ts AGENTS.md
git commit -m "feat: re-enable SAS device verification"
```

---

### Task 2: Validate the completed branch

**Files:**
- Verify: all files changed by Task 1 and the committed design/plan documents.

**Interfaces:**
- Consumes: completed frontend policy change.
- Produces: direct-exit local evidence suitable for the pull request.

- [x] **Step 1: Run frontend and IME gates**

```bash
npm --prefix apps/desktop test -- --run
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
node --test scripts/check-ime-text-inputs.test.mjs
node scripts/check-ime-text-inputs.mjs
npm --prefix apps/desktop test -- src/components/ImeTextControl.test.tsx
npm --prefix apps/desktop run qa:secret-scan
```

Expected: every command exits 0.

- [x] **Step 2: Run applicable Rust/Tauri and repository guards**

```bash
node scripts/check-sdk-submodule.mjs
cargo test -p koushi-state --test session_state
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
cargo fmt --all -- --check
git diff --check
```

Expected: every command exits 0, allowing only documented stable-toolchain warnings from `cargo fmt`.

- [x] **Step 3: Run the full browser-headless gate once**

```bash
(cd apps/desktop && npx playwright test --workers=1)
```

Expected: all browser-headless tests pass with the serialized repository configuration.

- [x] **Step 4: Self-review the complete branch**

```bash
git fetch origin
git diff --stat origin/main...HEAD
git diff origin/main...HEAD
git status --short
```

Confirm the diff contains only the design, plan, SAS policy/tests/config, and canon update; `HANDOFF.md` remains the only unrelated untracked file; no secrets, identifiers, stale flag references, generated drift, or unrelated changes exist.

- [x] **Step 5: Commit plan tracking updates if needed**

```bash
git add docs/superpowers/plans/2026-08-04-reenable-device-sas-verification.md
git commit -m "docs: record SAS verification implementation plan"
```

---

### Task 3: Open, validate, and merge the pull request

**Files:**
- No product-file changes unless review or CI identifies a reproducible defect.

**Interfaces:**
- Consumes: validated branch commits.
- Produces: merged remote default-branch commit and closed topic branch.

- [x] **Step 1: Reconcile and push**

```bash
git fetch origin
git rebase origin/main
node scripts/check-sdk-submodule.mjs
git push --set-upstream origin codex/reenable-sas-verification
```

Re-run affected focused gates after any nontrivial rebase resolution.

- [x] **Step 2: Open an accurate PR**

Create a non-draft PR describing the default re-enablement, mandatory warning, unchanged Rust/Core lifecycle, SAS-only guidance correction, and exact local validation evidence.

- [ ] **Step 3: Monitor and repair all required checks**

Use `gh pr checks --watch`. For every failure, inspect the failing job, reproduce the cause locally where possible, add or update the smallest behavioral check first, fix the root cause, rerun affected local gates, commit, push, and continue until all required checks pass.

- [ ] **Step 4: Merge and audit remote completion**

Merge through the repository-approved GitHub method and delete the topic branch. Verify:

```bash
gh pr view --json state,mergedAt,mergeCommit,statusCheckRollup
git fetch origin --prune
git merge-base --is-ancestor <topic-head> origin/main
git ls-remote --heads origin codex/reenable-sas-verification
git status --short
```

Require `MERGED`, all required checks successful, topic head ancestral to `origin/main`, no remote topic branch, and only preserved `HANDOFF.md` untracked before declaring completion.
