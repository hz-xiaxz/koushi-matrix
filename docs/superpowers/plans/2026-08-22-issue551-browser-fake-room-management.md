# Issue #551 browser fake room-management projection extraction

Status: design approved. Scope is the first behavior-preserving `browserFakeApi.ts` ownership seam.

## Baseline

- Base: `893462aefda7f11fafb845089da3bb8802c106f2` after App audit PR #632.
- `browserFakeApi.ts`: 6,514 newline-delimited lines / 214,982 bytes / SHA-256 `24dc97cb753f4e8f9d6aa602ba63ccb31582f10f45bf797fab0518f173c0c688`.
- Contract counts: `DesktopApi`170 methods; `BrowserFakeApi`170 contract methods +33 private methods; 15 fields/7 persistent maps.
- Focused baseline: browser fake86; Tauri client25.

## Ownership decision

Create private direct module `backend/browser-fake/roomManagement.ts`. Move exactly these six pure declarations in order:

1. `defaultRoomManagementState`
2. `editableRoomPermissionFacts`
3. `readonlyRoomPermissionFacts`
4. `roomMemberRoleFromPowerLevel`
5. `applyRoomSettingChange`
6. `roomModerationAllowed`

Add only export modifiers. Import all six directly into `browserFakeApi.ts` from the new module; no barrel/re-export/helper object.

Destination has one type import from `../../domain/types` for `DesktopSnapshot`, `RoomModerationAction`, `RoomPermissionFacts`, `RoomSettingChange`, and `RoomSettingsSnapshot`.

## Exact contracts

Preserve default/idle room-management DTO, permission facts, power-level thresholds, tagged setting-change mapping and moderation guards exactly. Existing call sites remain unchanged in session clearing/snapshot construction, room settings, setting update, moderation and member-role update.

No DesktopApi/BrowserFakeApi public path, method, field, map, timer, ID, event, async ordering, clone or cleanup owner moves. Client/harness/wire/Rust code remain untouched.

The wall-clock IDs, module-level link-preview fixture mutation, and missing cleanup/caps for prepared bytes/leases/submission ledger are explicitly out of this move-only scope and require separate verify-first lifecycle decisions during residual audit.

## Deterministic exactness

- declarations6/6/order, parent0, exports6;
- bodies/types/switch order exact modulo export;
- destination import path1, source direct import6;
- call sites and all retained declarations/class methods exact;
- public contract/resources/dependencies delta0;
- duplicate/missing/excess declarations0.

## Verification

Run browser fake86, client25, typecheck/lint/build, full Vitest/Playwright, boundary/security/exactness/diff and full repository gates. After full-diff approval, integrate latest main, PR CI7/7 and merge. Browser fake checkbox remains open for subsequent ownership seams/lifecycle re-evaluation/final audit.

## Review gate

- Read-only reconnaissance measured the full fake and selected the contiguous resource-free room-management projection seam.
- `reviewer-flash` independently traced the contiguous pure closure, all call sites, DTO/guard exactness, tests/counts and contract/resource exclusions and recorded `Correct-to-implement`.
- Shell baseline is 6,514 newline characters (`wc -l`); editor display may include a final unterminated line.
