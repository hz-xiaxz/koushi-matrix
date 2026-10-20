# 2026-10-20 macOS release: unchecked Tauri staple

## What happened

Release run 39413145338 (v0.29.0) failed twice in **macOS arm64 signed and
notarized DMG** at `stapler validate Koushi-macos-arm64.dmg app` with exit 65,
`Koushi.app does not have a ticket stapled to it`. The app was notarized
(`Accepted`) and the build printed `Stapling app...`. Windows and Linux built;
nothing was published. Runner image and Tauri CLI (2.12.1) were identical to
the successful v0.28.0 run.

## Root cause

`tauri-macos-sign` (`staple_app`, tauri-cli-v2.12.1) runs
`xcrun stapler staple -v Koushi.app` with `.output()` and never checks the
status, so when Apple's CloudKit ticket service does not return the fresh
ticket yet, the staple fails silently and the unstapled app goes into the DMG
and the updater archive. v0.28.0 passed by timing.

## Fix

- `scripts/macos/xcrun`: shim placed first on `PATH` only for signed builds
  with notarization credentials. Retries `stapler staple` / `stapler validate`
  (15/30/60/120/120 s), records each attempt in a log, passes every other
  `xcrun` call through.
- `scripts/desktop-build-dmg.mjs`: prints that log and validates the built
  app's ticket after Tauri, failing the build if it is missing.
- No signing, notarization, Gatekeeper, checksum or workflow gate changed.

## Canon consulted / changed

`docs/releases/desktop-release.md` (contract and failure recovery) updated.

## Verification

`node --test scripts/macos-xcrun-shim.test.mjs` (retry to success, bounded
failure with log, validate retry, passthrough) on Linux with a fake xcrun;
`releaseConfiguration.test.ts` (unchanged `--print-command` output). The real
proof is the next protected macOS release run.
