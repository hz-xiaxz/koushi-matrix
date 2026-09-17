# Issue #878: macOS auto-update

## Goal

Ship a signed, non-blocking in-app update path for the macOS arm64 build while
keeping the small amount of update state and UI portable to Windows and Linux.
Only macOS is enabled in this phase.

## Constraints

- Use `tauri-plugin-updater`; do not implement download, signature validation,
  archive extraction, or app replacement ourselves.
- Keep the persisted `auto_check` preference in Rust settings. Existing stores
  backfill it to `true`.
- Keep the updater lifecycle in one Rust-owned desktop adapter module. React
  receives typed state and sends only settings/download/restart intents.
- Do not enable Windows or Linux until their signed release artifact contracts
  are separately approved.
- A failed check or install stays non-fatal and never delays startup or login.

## State and boundaries

The adapter exposes one tagged state:

`unsupported | idle | checking | available | downloading | ready | failed | installing`

`available` and `ready` carry only the public version string. `failed` carries a coarse stage
and kind, never a raw URL, response, local path, or library error. A single
managed updater slot owns the verified bytes and matching Tauri `Update` value.
No backend trait or multi-provider registry is introduced in this phase.

The adapter checks once after startup when `settings.values.updates.auto_check`
is enabled, checks again when that setting changes from off to on, and checks at
most once per 24-hour interval while enabled. State changes are emitted on one
desktop event and are also available from one initial-state command.

## Work

1. Amend the architecture/state-machine canon and add this plan to the index.
2. Add `UpdatesSettings { auto_check }`, patching, default/backfill tests, and
   the exact TypeScript mirror.
3. Add the updater plugin and one `app_updates` adapter module with focused unit
   tests for platform support and state transitions.
4. Wire typed state/events through the desktop backend, ask for confirmation
   when an automatic check finds an update, and render download/restart actions
   in User Settings.
5. Generate macOS updater artifacts only, verify the extracted app, upload the
   archive/signature, and publish `latest.json` with the release.
6. Extend release configuration tests, preflight checks, and the release
   runbook. Run focused Rust/frontend/release checks, then the practical local
   gate set.

## Completion boundary

Code and deterministic release checks can complete locally. Publishing requires
the updater private key and password in the protected `release-macos`
environment. Final acceptance still requires one physical macOS installed-app
upgrade from an older signed release to the next release.
