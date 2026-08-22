# Issue #544 Logical Window-state Restore

## Scope

Replace ambiguous physical-pixel window persistence with a versioned logical-pixel schema and suppress programmatic startup geometry echoes. Preserve single-window ownership, maximized restore, atomic file writes, off-screen clamping, and user-initiated persistence. No frontend geometry owner, timer/sleep stabilization, platform-specific fork, or compatibility shim that keeps unsafe legacy geometry active.

## Persisted contract

`PersistedWindowState` becomes schema version 2 with an explicitly mixed but unit-consistent desktop contract:

- `x_physical` / `y_physical` are global physical desktop coordinates, because mixed-DPI monitors do not share one coherent global logical origin;
- `width_logical` / `height_logical` are integer logical dimensions, preserving user-visible size across 1×/2× monitors;
- `capture_scale_factor` records the source monitor scale solely to reconstruct the saved physical rectangle for monitor intersection;
- `maximized` is unchanged.

Capture keeps Tauri's physical outer position, converts outer size through the current window scale factor, and rounds only the logical size. Restore selects a monitor by intersecting the saved physical rectangle (`logical size × capture scale`) with physical monitor work areas. It then computes target physical bounds from the logical size and selected monitor scale, clamps the physical position in that work area, calls `Size::Logical` for size and `Position::Physical` for global placement, and maximizes last. No calculation mixes logical origins from different monitors.

Records without version 2 are legacy physical-pixel records and fail closed to the configured logical default `1280 × 820`, physically centered in the selected primary work area by the same pure geometry function. They are not guessed or reinterpreted. The next genuine user geometry change writes version 2, so invalid legacy state cannot become sticky.

Minimum `760 × 620` validates logical dimensions. Off-screen intersection/clamping stays physical and target-size-aware. Maximized state restores after normal geometry.

## Startup persistence gate

`WindowStatePersistenceGate` has explicit `PreArm`, `Restoring`, and `Ready` phases. `on_window_event` fail-closes geometry persistence while the managed gate is absent or `PreArm`, covering window-creation events before `.setup()`. Setup manages the gate before restore. The restore path computes one exact `AppliedWindowGeometry` (logical size, physical position, maximized) with the same pure function used by setters, arms `Restoring` before any `set_size`/`set_position`/`maximize` call, then applies it.

`Restoring` tracks size and position independently by event kind. Resized/ScaleFactorChanged observations are suppressed until the observed logical size equals the expected size; Moved observations are suppressed until physical position equals expected. Intermediate creation/restore values, exact matches, and duplicate exact echoes never retire the gate. After both expected components have been observed, the first differing non-maximized resize/move is user intent: it moves to `Ready` and persists the complete current schema-v2 geometry. For a startup-maximized window, geometry events while `window.is_maximized()` are suppressed; unmaximize back to expected normal geometry remains suppressed, and the first subsequent differing event retires the gate. CloseRequested/Destroyed persist only in `Ready`.

Default fallback does not call opaque `center()`: the pure geometry function computes the primary work-area center, so expected centering and applied centering use identical rounding. This is value/fence based, not time based: no sleep, debounce, page-load guess, or secure-backup-state coupling. A secure-backup gate may remain visible arbitrarily long without rewriting geometry; a genuine resize/move after startup echoes settle is persisted.

## Verify first

Pure deterministic tests precede runtime changes:

1. A legacy `1077 × 853` record is rejected; captured at 2× that physical geometry resolves below `760 × 620` logical.
2. Physical sizes representing the same logical size at 1× and 2× capture identically; restore on either scale computes the same logical size and correct target physical bounds.
3. Mixed-DPI placement uses physical global coordinates: a 2× secondary capture restored with a 1× primary still selects/clamps to the correct physical work area.
4. Version-2 JSON round-trips; unversioned legacy JSON loads as no restorable state.
5. Minimum validation is logical; invalid state selects exact default `1280 × 820` and deterministic primary centering, including odd-pixel rounding.
6. Off-screen/multi-monitor physical clamping and primary fallback remain deterministic.
7. Pre-arm geometry events are suppressed; expected Resized/Moved/ScaleFactorChanged observations and duplicates are suppressed independently until both settle.
8. A differing user resize/move after expected observations retires the gate and persists; no later expected duplicate overwrites it.
9. Maximized restore events do not retire the gate; unmaximize-to-normal remains suppressed before subsequent user geometry persists.
10. Close/Destroyed during PreArm/Restoring does not persist; Ready close behavior remains.
11. Existing atomic-path, focus, close, and event-classification tests remain green.

## Implementation

Keep code in `apps/desktop/src-tauri/src/lib.rs`. Extract only small pure conversion/selection/gate functions required by deterministic tests. Reuse existing persistence path and atomic write. Do not add a crate/dependency, async task, timer, frontend command, or second state file.

## Gates

- `reviewer-flash-opencode-go` design verdict before implementation.
- `luna-implementer` at max thinking for verify-first implementation.
- Focused Tauri lib tests, full Tauri lib, formatting, clippy if configured, and platform-independent schema/gate evidence; macOS native inspection is confirmation only.
- `reviewer-flash-opencode-go` exact full-diff verdict after implementation.
- Integrated full local matrix, CI, merge, issue evidence, and build-artifact cleanup in the shared PR.
