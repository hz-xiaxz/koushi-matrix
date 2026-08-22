# Issue #544 Logical Window-state Restore

## Scope

Replace ambiguous physical-pixel window persistence with a versioned logical-pixel schema and suppress programmatic startup geometry echoes. Preserve single-window ownership, maximized restore, atomic file writes, off-screen clamping, and user-initiated persistence. No frontend geometry owner, timer/sleep stabilization, platform-specific fork, or compatibility shim that keeps unsafe legacy geometry active.

## Persisted contract

`PersistedWindowState` becomes schema version 2 with integer logical `x`, `y`, `width`, `height`, and `maximized`. Capture converts Tauri physical outer position/size through the window's current scale factor and rounds to logical integers. Restore uses `Position::Logical` and `Size::Logical`.

Records without version 2 are legacy physical-pixel records and fail closed to the configured logical default `1280 × 820`, centered. They are not guessed or reinterpreted. The next genuine user geometry change writes version 2, so invalid legacy state cannot become sticky.

Minimum `760 × 620`, restored geometry validation, monitor work areas, intersection selection, and clamping operate in logical units. Each monitor work area is converted using that monitor's scale factor before selection. Maximized state is restored after size/position.

## Startup persistence gate

A Tauri-managed `WindowStatePersistenceGate` is installed before restore and records the expected logical startup geometry (restored or default). Geometry events whose observed logical size/position equal that expected programmatic geometry are suppressed. The first differing geometry event is treated as user intent, retires the gate, and persists the complete current logical geometry. Closing/destroying while only startup geometry has been observed does not overwrite the last good state.

This is value/fence based, not time based: no sleep, debounce, page-load guess, or secure-backup-state coupling. The startup/verification/secure-backup gate may remain visible arbitrarily long without rewriting geometry; a real user resize/move during it is persisted because it differs from the expected programmatic geometry.

## Verify first

Pure deterministic tests precede runtime changes:

1. A legacy `1077 × 853` record is rejected; at 2× its physical geometry resolves below `760 × 620` logical.
2. Capture at 1× and 2× produces the same logical size; restoring that logical state is scale-independent in both directions.
3. Version-2 JSON round-trips; unversioned legacy JSON loads as no restorable state.
4. Minimum validation is logical; invalid state selects default `1280 × 820` and centering.
5. Off-screen/multi-monitor logical clamping and primary fallback remain deterministic.
6. Programmatic Resized/Moved/ScaleFactorChanged observations matching expected geometry are suppressed.
7. A differing user resize/move during startup retires the gate and persists; no backwards overwrite follows.
8. Maximized state round-trips and restores after geometry.
9. Existing atomic-path, focus, close, and event-classification tests remain green.

## Implementation

Keep code in `apps/desktop/src-tauri/src/lib.rs`. Extract only small pure conversion/selection/gate functions required by deterministic tests. Reuse existing persistence path and atomic write. Do not add a crate/dependency, async task, timer, frontend command, or second state file.

## Gates

- `reviewer-flash-opencode-go` design verdict before implementation.
- `luna-implementer` at max thinking for verify-first implementation.
- Focused Tauri lib tests, full Tauri lib, formatting, clippy if configured, and platform-independent schema/gate evidence; macOS native inspection is confirmation only.
- `reviewer-flash-opencode-go` exact full-diff verdict after implementation.
- Integrated full local matrix, CI, merge, issue evidence, and build-artifact cleanup in the shared PR.
