# Issue #536 — macOS: truthful notification-sound outcome and reliable playback

Status: design v3 approved; implementation complete (post-implementation review pending). Normative:
REPOSITORY_RULES.md, docs/architecture/overview.md,
docs/agents/state-ownership.md, docs/architecture/state-machine.md.

## Problem

On macOS a new-message Dock-badge increase does not produce an audible
notification sound, while diagnostics report `attention_sound_outcome
outcome=played`. Reproduces after #529 (use the macOS user-preferred alert
sound).

Work base: `origin/main` (PR #529 merged, `1a5a320f`), where the macOS
adapter calls
`AudioServicesPlayAlertSound(kSystemSoundID_UserPreferredAlert)` and
unconditionally returns `Played`. (The checked-out local HEAD predates
#529 and still shows `AudioServicesPlaySystemSound(1007)`; the fix branch is
cut from `origin/main` so the reproduction matches the plan.)

Observed diagnostics (2026-08-16):

```text
native.attention attention_badge_sound_delta previous=0 current=1 delta=1
desktop.native_badge stage=apply_settled outcome=applied count=1
native.attention attention_sound_outcome outcome=played
```

## Root cause

The macOS adapter (introduced by #503, changed by #529) is:

```rust
#[cfg(target_os = "macos")]
impl NativeAttentionSoundBackend for PlatformNativeAttentionSoundBackend {
    async fn play(&self) -> NativeAttentionSoundOutcome {
        unsafe { AudioServicesPlayAlertSound(MACOS_USER_PREFERRED_ALERT_SOUND_ID) };
        NativeAttentionSoundOutcome::Played
    }
}
```

Two defects:

1. **False positive outcome.** `AudioServicesPlayAlertSound` returns `void`
   and is fire-and-forget. `Played` proves only that the FFI call was made,
   not that playback started or was audible.
2. **Silent-by-configuration path.** `kSystemSoundID_UserPreferredAlert`
   (0x1000) plays the alert the user selected in Sound settings. When the
   alert sound is set to "None", macOS stores an empty string under
   `com.apple.sound.beep.sound` (NSGlobalDomain) and the system plays
   nothing; a zero alert volume is equally silent. The #503 path
   (`AudioServicesPlaySystemSound(1007)`, a plain UI-sound effect) played on
   the affected Mac, so the regression is specific to the user-alert path.

## Chosen fix

Replace the macOS playback path with `NSSound` (AppKit), which plays through
the app's audio output and does **not** depend on the "Play sound effects"
preference, the alert-volume setting, or the user-alert "None" choice.
Playback is driven on the main thread via `AppHandle::run_on_main_thread`
(the existing badge path already does this) because AppKit release notes
historically recommend main-thread `NSSound` playback, and there is no CI
lane that compiles or runs the macOS code, so the code must be as boring as
possible.

Outcome is truthful:

- User alert sound set to "None" (empty string) -> `Failed` (no sound source).
- Sound source cannot be loaded (`NSSound::soundNamed` /
  `initWithContentsOfFile:byReference:` return nil) -> `Failed`.
- `NSSound::play()` returns false (playback could not start) -> `Failed`.
- Main-thread dispatch (`run_on_main_thread`) fails, the main-thread
  callback is dropped, or the main-thread dispatch timeout expires ->
  `Failed` (same pattern as the badge path). The core admission phase keeps
  its existing 2 s timeout, which intentionally maps to `Skipped`, not
  `Failed`.
- Otherwise -> `Played` ("playback accepted/started"; audibility still
  requires a real output device and is verified manually).

### Sound source resolution

`com.apple.sound.beep.sound` (read through `NSUserDefaults`; NSGlobalDomain
is in the standard search list) is interpreted as:

| Setting value            | Interpretation               |
| ------------------------ | ---------------------------- |
| key absent (`None`)      | user kept the default        |
| empty string (`""`)      | user chose "None" -> `Muted` |
| starts with `/`          | absolute path to a sound file|
| otherwise                | named system sound           |

For the default case the adapter plays the macOS default alert sound name
`Funk` (present in `/System/Library/Sounds` on every supported macOS, so no
version-dependent lookup is needed). A named source is loaded with
`NSSound::soundNamed`; a path source with
`initWithContentsOfFile:byReference:`. `NSSound` retains itself while
playing, so dropping the object after `play()` returns true does not stop
playback.

The pure resolution and outcome-mapping logic is factored into small
functions so the adapter-level tests cover the outcome mapping without an
audio device:

```rust
#[cfg(target_os = "macos")]
enum MacosAlertSoundSource { Muted, Named(String), Path(String) }

#[cfg(target_os = "macos")]
fn resolve_macos_alert_sound_source(setting: Option<&str>) -> MacosAlertSoundSource

#[cfg(target_os = "macos")]
fn macos_sound_outcome(loaded: bool, started: bool) -> NativeAttentionSoundOutcome
// (!loaded || !started) -> Failed; otherwise Played
```

### Adapter shape (no trait change)

`NativeAttentionSoundBackend::play(&self)` keeps its signature. The macOS
platform adapter holds the `tauri::AppHandle`; other platforms keep the
unit struct:

```rust
#[cfg(target_os = "macos")]
struct PlatformNativeAttentionSoundBackend { app: tauri::AppHandle }

#[cfg(not(target_os = "macos"))]
struct PlatformNativeAttentionSoundBackend;
```

`play_native_attention_sound` gains an `AppHandle` parameter (same pattern as
`set_native_attention_badge`, which already receives `app: AppHandle`) and
constructs the adapter:

```rust
#[tauri::command]
pub(crate) async fn play_native_attention_sound(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<NativeAttentionSoundOutcome, &'static str> {
    #[cfg(target_os = "macos")]
    let backend = PlatformNativeAttentionSoundBackend { app };
    #[cfg(not(target_os = "macos"))]
    let backend = PlatformNativeAttentionSoundBackend;
    Ok(dispatch_native_attention_sound(&state.runtime, &backend).await.0)
}
```

The dispatch/concurrency tests keep using `FakeBackend` /
`ControlledBackend` without an app handle, so no `tauri::test::mock_app()`
and no runtime-generic trait are needed (this also avoids the MockRuntime
pitfall where queued main-thread closures are not executed by the mock).

Dependencies (already present; feature flags only):

- `objc2-app-kit`: add feature `NSSound`
- `objc2-foundation`: add feature `NSUserDefaults`
- Remove the `AudioToolbox` FFI declaration and
  `MACOS_USER_PREFERRED_ALERT_SOUND_ID` (no longer used on macOS).

## Canon amendments (same change)

`docs/architecture/state-machine.md` (Date bumped to 2026-08-16): the native
adapter line changes from "macOS AudioToolbox" to "macOS AppKit NSSound"
and gains the truthful-outcome semantics (`failed` for "None", unloadable
source, playback start failure, main-thread dispatch failure/timeout;
`played` only when `NSSound` accepted playback). Outcome tokens and the
dispatch state machine are unchanged. The amendment was requested by the
design reviewer (Luna, frontier model); final approval is this reviewed
design before implementation.

## Unchanged behavior (must stay intact)

- Sound is still driven by positive Dock-badge deltas in the TS
  `createDesktopBadgeSoundDispatcher` (cooldown 3 s, in-flight
  deduplication, `policy.sound` setting, `capabilities.sound` gate).
- Focus suppression and candidate gating remain Rust-owned
  (`recompute_native_attention_from_rooms` / `native_attention` reducer).
- `NativeAttentionSoundOutcome` enum and TS outcome tokens are unchanged.
- Linux/Windows adapters are untouched.

## Tests

1. `#[cfg(target_os = "macos")]` unit tests in
   `apps/desktop/src-tauri/src/commands/native_attention.rs`:
   - `resolve_macos_alert_sound_source`: absent -> default `Funk`, empty ->
     `Muted`, absolute path -> `Path`, plain name -> `Named`, whitespace-only
     value treated as a name (fail-closed: such a value would fail to load
     and map to `Failed`).
   - `macos_sound_outcome`: (false, _) -> `Failed`, (true, false) -> `Failed`,
     (true, true) -> `Played`.
2. Existing dispatch/concurrency tests keep their fakes; the macOS adapter
   construction (with `AppHandle`) is exercised on macOS only.
3. TS tests in `desktopAttention.test.ts` are unchanged (outcome tokens are
   stable).

There is no macOS CI lane, so the macOS-specific code is not compiled or
executed in this environment. Mitigations: keep the macOS code minimal, rely
on the Luna review pass over the macOS-specific diff, and verify on the
affected Mac with the packaged build and `cargo test` before merge.

## Open questions / risks

- `com.apple.sound.beep.sound == ""` means "None" per Apple StackExchange and
  nix-darwin; if a future macOS stores the default differently, the empty
  row must be revisited. Fail-safe: an empty string never produces sound,
  which matches the "intentionally muted" clause of the issue.
- Whether the affected Mac's silence was caused by "None" or by alert volume
  0 is not distinguishable from the issue evidence; `NSSound` fixes both
  (volume-independent app output) while still respecting an explicit "None".
- `Played` means "playback accepted/started", not "audibly heard"; manual
  hardware verification remains required (issue acceptance criterion 5).
- Main-thread dispatch via `run_on_main_thread` matches the badge path;
  `NSSound` objects are created and played within one main-thread block and
  retain themselves while playing. On non-macOS targets the `app` parameter
  is unused; if the compiler warns, a cfg-gated `let _ = &app;` silences it.

## Verification commands (Linux, as far as possible)

```bash
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run test -- --run src/domain/desktopAttention.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib  # dispatch tests
git diff --check
```

macOS (on the affected Mac, after packaging):

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
# packaged app: unfocused window, message arrives with badge 0 -> sound, diagnostics == played
# set alert sound to "None" -> no sound, diagnostics == failed
```

## Review gate

- Design v1 reviewed by Luna (read-only, different model family): blocking
  findings recorded and addressed in v2 (canon conflict, AppHandle/MockRuntime
  incompatibility, dispatch failure/timeout mapping, outcome-test coverage,
  base-branch reconciliation).
- Design v2 re-reviewed by Luna: blocking findings (command signature lacks
  AppHandle; cfg-gated adapter construction) addressed in v3.
- **Design v3 verdict (Luna, pre-implementation): Correct-to-merge.**
- **Implementation diff verdict (Luna, post-implementation): blocking
  findings** — `NSSound::alloc()` requires `use objc2::AnyThread` (added);
  plan metadata must record the verdict (this edit). Non-blocking: macOS
  compile/test run still required on a real Mac (no macOS CI).
- Post-fix diff re-review by Luna: verdict recorded below.
