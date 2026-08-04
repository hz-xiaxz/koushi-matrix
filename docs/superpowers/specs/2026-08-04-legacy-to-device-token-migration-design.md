# Legacy To-Device Token Migration Design

Date: 2026-08-04
Status: Approved in conversation

## Problem

Koushi now requires Simplified Sliding Sync, but an account store previously
used by classic `/sync` can contain that API's compound `next_batch` value in
the crypto store's shared to-device token slot. The pinned Matrix Rust SDK
copies that value into `extensions.to_device.since`. Synapse requires the
Sliding Sync to-device token to be a decimal stream position, so it rejects
the request with HTTP 400 `M_INVALID_PARAM` before the first response.

## Decision

Add a narrow compatibility migration in the pinned SDK at the point where the
to-device token is restored for a Sliding Sync request:

- no token remains `None`;
- a non-empty ASCII-decimal token is retained unchanged;
- every other token is treated as classic-sync legacy state and omitted from
  the request;
- the account session, crypto identities and keys, room/event cache, and
  Sliding Sync position remain untouched.

Omitting the incompatible token lets the server start the to-device stream at
its initial position. The first successful Sliding Sync response persists a
valid decimal token through the existing crypto-store path, making the
migration self-completing without a separate marker or database rewrite.

The diagnostic boundary records only `present`, `sliding`, or `legacy`
classification and whether migration was applied. It must never expose the
token value, length, prefix, or homeserver identifiers.

## Alternatives Rejected

- Recreate the whole SDK database: unnecessarily discards useful cached state
  and risks losing locally held encryption material.
- Retry any HTTP 400 without `since`: can hide unrelated malformed requests
  and adds a second network attempt to every affected startup.
- Accept only server-side recovery: Synapse correctly enforces the Sliding
  Sync request schema, so the incompatible client state must be migrated.

## Verification

Unit tests prove that decimal tokens are retained and compound/empty tokens
are omitted. A request-level SDK test proves a legacy token is absent from the
serialized Sliding Sync request. The existing Koushi diagnostic tests prove
that only the coarse classification is user-copyable. Before the PR, run the
focused Rust tests, workspace formatting/checks appropriate to the touched
crates, build the macOS application, and rebuild the arm64 DMG.
