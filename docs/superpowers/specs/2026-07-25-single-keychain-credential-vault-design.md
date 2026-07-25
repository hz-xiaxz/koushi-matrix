# Single-Keychain Credential Vault Design

## Goal

After legacy credential migration, starting the desktop application through
`./scripts/run.sh` performs at most one macOS Keychain read while preserving
all saved Matrix sessions and encrypted local-store keys.

The first launch after upgrading may still require access to each legacy
Keychain entry. That one-time cost is unavoidable because Koushi must decrypt
the existing credentials before it can migrate them without signing the user
out.

## Current problem

Koushi currently stores several independent generic-password entries:

- the last-session pointer;
- the saved-session index;
- one Matrix session per account;
- one local unlock secret per account.

Startup restoration reads the last-session pointer and Matrix session, then
loads the same local unlock secret separately for the SDK store and search
index. Draft, navigation, scheduled-send, room-preference, and read-state
restoration may load that secret again. A Tauri development build has no stable
signing identity, so macOS can ask for the login password for each access.

## Architecture

### Keychain entry

Keychain stores exactly one new credential:

`koushi-desktop:credential-vault-key:v1`

Its value is a randomly generated 256-bit master key. The key is represented by
a secret-owning type whose `Debug` implementation is redacted. It must never
appear in diagnostics, events, snapshots, errors, or serialized application
state.

### Encrypted vault

Koushi stores `credentials/credentials.v1.enc` below its application data
directory. The authenticated-encryption envelope has an explicit magic,
version, random nonce, ciphertext, and authentication tag. Its plaintext
contains:

- an optional last-session `SessionKeyId`;
- the saved-session index;
- Matrix session JSON keyed by `SessionKeyId`;
- local unlock secrets keyed by `SessionKeyId`.

The vault is the authoritative credential source after migration. Account
store, search index, composer draft, scheduled send, navigation, room
preference, and read-state keys are derived from the in-memory local unlock
secret and do not perform additional Keychain reads.

### Process lifetime

`CredentialStoreBackend` owns a mutex-protected vault state. Its first operation
coalesces concurrent initialization:

1. read the master key once;
2. read and authenticate the encrypted vault;
3. retain the decoded vault in memory for the process lifetime.

All subsequent reads and writes use that state. Mutations serialize through the
same mutex.

### Persistence

Every mutation writes a complete new encrypted vault to a sibling temporary
file, calls `sync_all`, atomically renames it over the destination, and syncs
the parent directory where supported. Failure before rename leaves the previous
vault authoritative.

## Legacy migration

When the new master-key entry or encrypted vault is absent, Koushi checks the
legacy saved-session index and last-session pointer.

If legacy state exists:

1. read every referenced Matrix session and local unlock secret;
2. validate and assemble the complete vault in memory;
3. create or load the master key;
4. durably persist and re-open the encrypted vault;
5. only after successful verification, delete legacy entries best-effort.

If any required legacy entry cannot be read, the migration makes no persistent
changes and session restoration fails closed. If deleting legacy entries
fails, the new vault remains authoritative and deletion is retried later.

If no legacy state exists, Koushi creates an empty vault on the first credential
write. Merely listing sessions on a fresh installation returns an empty list
without creating a Keychain entry.

## Compatibility and rollback

During the migration release, reads prefer a valid new vault and otherwise
fall back to legacy migration. Writes target the vault only after successful
initialization. No code path may merge partially read legacy data with an
existing vault.

A previous Koushi version cannot read the new vault after legacy entries are
deleted. The migration is therefore forward-only, like the existing encrypted
store format. It does not delete Matrix SDK databases or media caches.

## Error handling

- Missing master key with a valid vault is a fail-closed
  `LocalEncryptionUnavailable` condition; Koushi does not overwrite the vault.
- Authentication failure, malformed plaintext, or an unsupported version is a
  fail-closed credential-store failure.
- A failed atomic write leaves the in-memory mutation uncommitted and the prior
  on-disk vault intact.
- No raw backend, cryptographic, credential, or account identifiers are logged.
- Concurrent readers wait for the single initialization result instead of
  issuing duplicate Keychain requests.

## Testing

Tests use a counting in-memory credential backend and temporary application
data directory.

- A migrated startup performs exactly one `get_password`.
- Multiple saved accounts still perform exactly one `get_password`.
- Repeated store/search/draft/navigation access performs no additional
  Keychain reads.
- Legacy sessions and local unlock secrets survive migration byte-for-byte.
- A missing legacy entry leaves all legacy entries undeleted.
- Corrupt or unauthenticated vaults fail closed and are not overwritten.
- An injected write failure preserves the previous vault.
- Concurrent initialization performs one Keychain read.
- Secret material is absent from `Debug` and diagnostic output.

## Non-goals

- Removing the one-time Keychain prompts needed to read legacy entries.
- Changing Matrix login, logout, account switching, or soft-logout behavior.
- Changing Matrix SDK store encryption formats.
- Making production credentials available to unattended QA; QA continues to
  use its compile-time-gated file credential backend.
