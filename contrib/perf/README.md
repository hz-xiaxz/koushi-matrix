# Daily-operation latency bench (contrib)

These are measurement scripts, not a gate. They time what a user does every day in the real Linux WebView:

- launch and session restore
- first login
- room switch (cold and warm)
- back-pagination
- send
- typing
- entering a Space

They run against a disposable local tuwunel seeded with a synthetic account of realistic size.

**Platform.** `bench.mjs` runs on Linux only. It drives WebKitGTK through `tauri-driver` and `WebKitWebDriver`. tauri-driver has no macOS WKWebView backend. The harness also needs Xvfb, a private D-Bus session, `/proc`, `top -H` and `perf`. `seed.mjs` and `summarize.mjs` are plain Node and work anywhere tuwunel runs.

Nothing here runs in CI or `npm run lint`. The numbers are noisy and machine-dependent: compare runs on the same machine and the same build, never against fixed thresholds. Turn a finding into a headless RED test before fixing it (see [verification](../../docs/agents/verification.md)).

## Files

| File | What it does |
| --- | --- |
| `seed.mjs` | Starts a private tuwunel in `<workdir>/hs` and registers a main user and 12 helpers. It then fills rooms, DMs, big rooms and two Spaces, and writes `<workdir>/state.json`. |
| `bench.mjs` | Starts the seeded tuwunel, Xvfb, a private D-Bus and tauri-driver. It launches `koushi-desktop`, logs in (or restores the session), runs the scenarios, and writes `run-<ts>/` next to `<appDataDir>`. |
| `summarize.mjs` | Prints p50 and max for each metric across one or more `results.json` files. |

The scripts reuse the Linux GUI lane helpers in `scripts/desktop-linux-gui-qa/` and `scripts/lib/local-homeserver-qa.mjs`.

## Prerequisites

You need the [Linux GUI host packages](../../docs/agents/environment.md#linux-gui-host-packages), `tauri-driver`, and `tuwunel` on `PATH` (see [the binary search path](../../docs/agents/environment.md)).

You also need a debug app binary with the QA window title. QA hooks need `debug_assertions`, so keep the `dev` profile. You may raise its opt level so renderer and core costs look closer to a release build:

```bash
VITE_KOUSHI_QA_TITLE=1 npm --prefix apps/desktop run build
CARGO_PROFILE_DEV_OPT_LEVEL=2 KACHE_EXPLAIN_MISS=1 cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --profile dev --features tauri/custom-protocol
```

## Run

Seed once. The default scale takes about 3 minutes: 150 rooms × 40 messages, 20 DMs × 30, 3 big rooms × 3,000 messages, and 2 Spaces with 30 children each.

```bash
PATH=/tmp/koushi-desktop-local-qa-bin:$PATH node contrib/perf/seed.mjs /tmp/koushi-perf/seed
```

Measure first login with a full initial sync. The first login also creates and verifies the new identity:

```bash
PATH=/tmp/koushi-desktop-local-qa-bin:$HOME/.cargo/bin:$PATH node contrib/perf/bench.mjs /tmp/koushi-perf/seed /tmp/koushi-perf/app --fresh
```

Then measure session restore, which is the everyday launch. Reuse the same app data dir and repeat as often as needed:

```bash
PATH=/tmp/koushi-desktop-local-qa-bin:$HOME/.cargo/bin:$PATH node contrib/perf/bench.mjs /tmp/koushi-perf/seed /tmp/koushi-perf/app
```

Summarize the results:

```bash
node contrib/perf/summarize.mjs /tmp/koushi-perf/run-*/results.json
```

Log in once per seed and use restore afterwards. A second `--fresh` login on the same account is a new device of a cross-signed user, and it stops at the verification gate.

### Options

| Setting | Effect |
| --- | --- |
| `--only=switch,paginate,send,space` | Run only the listed scenarios. |
| `PERF_APP=path` | App binary (default `target/debug/koushi-desktop`). |
| `PERF_TOP=1` | Record per-thread CPU once a second to `top.log` (about 100 MB per full run). |
| `PERF_RECORD=1` | Run `perf record -g` on the app and WebKit web processes during the switch suite. This needs `kernel.perf_event_paranoid` ≤ 1, which you set yourself. |
| `PERF_ENV_<NAME>=value` | Pass `<NAME>=value` to the app, for example `PERF_ENV_JSC_useJITDump=1`. |
| `PERF_PORT`, `PERF_SMALL_ROOMS`, `PERF_SMALL_MSGS`, `PERF_BIG_ROOMS`, `PERF_BIG_MSGS`, `PERF_DMS` | Seed scale (`seed.mjs`). Keep `PERF_SMALL_ROOMS` ≥ 68 and `PERF_DMS` ≥ 9, so enough rooms stay outside the Spaces and enough DMs exist for the bench to pick. |

## What the metrics mean

All times are in milliseconds and measured in the page with `requestAnimationFrame`.

- **`restore.milestonesPageMs`**: page time when the room row, header, first message row and composer appeared, and when the QA title reported `session=ready` and `timeline_subscribed=true`. `wallSinceSpawn` is the wall clock time from WebDriver session start.
- **`switch.cold` / `switch.warm`**: time from clicking a room row until:
  - `header`: the header shows the room.
  - `anyRow`: a row of the room is among the last 8 rows.
  - `newest`: the room's newest seeded message is among the last 8 rows.
  - `settled`: no row, scroll-height or first-row change for 400 ms.

  `maxGap` is the longest frame gap, and `gaps50` counts frames longer than 50 ms. `timeout` means `newest` was not seen within 12 s.
- **`paginate.big`**: scrolls the big room to the top. `older` is the time until the first mounted row changes. `rowsBefore` and `rowsAfter` count mounted rows, not loaded items: a windowed timeline keeps them small.
- **`send`**: `cleared` is the time until the composer empties, `echo` until the local echo row appears, and `eventId` until the row carries its server event id.
- **`typing.bigRoom`**: from keydown to the next frame, for real WebDriver key input.
- **`space.enter` / `space.home`**: time until the rail button is active and the room list settles.

Each run also saves:

- `diagnostics.json`: the app's private-data-free diagnostics ring.
- `marks.log`: wall clock time marks for each step, to line up with `top.log` and `perf`.
- `run.log`: app, driver and D-Bus output.

## Known limits

- **Single machine.** Xvfb renders without GPU acceleration, so absolute numbers are higher than on a desktop session.
- **Space children are not timed.** They are hidden from Home, so the bench times only rooms outside the Spaces.
- **Already-open rooms are skipped.** Clicking the room that is already open is recorded as `skipped: "already-active"`.
- **Last 8 rows only.** `newest` looks only at the last 8 mounted rows. A timeline that opens away from the live edge therefore shows up as a timeout.
