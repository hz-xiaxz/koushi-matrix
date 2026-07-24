# Single-Keychain Credential Vault Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve saved sessions while reducing post-migration macOS Keychain access during `./scripts/run.sh` startup to one master-key read.

**Architecture:** Add a redacted 256-bit vault master-key type to `koushi-key`, and add a focused `credential_vault` module to `koushi-core`. The OS credential backend lazily migrates legacy per-record Keychain entries into one authenticated, atomically replaced encrypted file, caches the decoded vault for the process lifetime, and leaves file/in-memory QA backends unchanged.

**Tech Stack:** Rust 2024, `chacha20poly1305`, `serde_json`, `zeroize`, `tempfile`, existing `CredentialBackend`.

## Global Constraints

- After migration, one process performs at most one Keychain `get_password`, including multiple accounts and repeated store/search/draft/navigation access.
- Existing sessions and local unlock secrets must survive migration.
- Migration is fail-closed and deletes legacy entries only after the new vault has been durably written and re-opened.
- Secret values and account identifiers must not enter logs, diagnostics, snapshots, or `Debug`.
- QA file and in-memory credential backends retain their current behavior and never access the OS Keychain.
- Long-running suites run only after the implementation is complete.

---

### Task 1: Vault master-key primitives and counting test backend

**Files:**
- Modify: `crates/koushi-key/src/lib.rs`
- Test: `crates/koushi-key/src/lib.rs`

**Interfaces:**
- Produces: `VAULT_MASTER_KEY_LEN`, `credential_vault_key_account_name()`, `CredentialVaultMasterKey::{generate,to_storage_string,from_storage_string,as_bytes}`, and `CredentialStore::{save_vault_master_key,load_vault_master_key,delete_vault_master_key}`.
- Produces for tests: `InMemoryCredentialBackend::{get_password_count,contains_entry}` without exposing stored values.

- [ ] **Step 1: Write failing tests**

Add tests that require:

```rust
let key = CredentialVaultMasterKey::generate();
let restored =
    CredentialVaultMasterKey::from_storage_string(key.to_storage_string().as_str()).unwrap();
assert_eq!(restored.as_bytes(), key.as_bytes());
assert!(!format!("{key:?}").contains(key.to_storage_string().as_str()));

let backend = InMemoryCredentialBackend::default();
let store = CredentialStore::with_backend("service", backend.clone());
store.save_vault_master_key(&key).unwrap();
assert_eq!(store.load_vault_master_key().unwrap().as_bytes(), key.as_bytes());
assert_eq!(backend.get_password_count(), 1);
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p koushi-key credential_vault_master_key
```

Expected: compile failure because the master-key API is absent.

- [ ] **Step 3: Implement the primitives**

Use `Zeroizing<[u8; 32]>`, base64 storage, a constant account name
`koushi-desktop:credential-vault-key:v1`, and a redacted `Debug`
implementation. Extend the in-memory backend state with a monotonically
increasing read count and key-existence query.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p koushi-key credential_vault_master_key
```

Expected: all matching tests pass.

### Task 2: Authenticated credential-vault file

**Files:**
- Create: `crates/koushi-core/src/credential_vault.rs`
- Modify: `crates/koushi-core/src/lib.rs`
- Test: `crates/koushi-core/src/credential_vault.rs`

**Interfaces:**
- Produces: `CredentialVaultData`, a private versioned serde payload, and
  `CredentialVaultFile::{load,store}`.
- `CredentialVaultData` provides CRUD methods for last session, saved-session
  membership, Matrix sessions, and local unlock secrets without exposing
  plaintext through `Debug`.

- [ ] **Step 1: Write failing round-trip and corruption tests**

Construct a vault with two `SessionKeyId` values, sessions, and unlock secrets.
Require an encrypted round trip, require that the file bytes contain none of
the plaintext session JSON or identifiers, and require a modified ciphertext
to return a coarse corrupt/unavailable error without overwriting the file.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p koushi-core credential_vault_file
```

Expected: compile failure because `credential_vault` is absent.

- [ ] **Step 3: Implement encryption and decoding**

Use:

```rust
const MAGIC: &[u8] = b"KOUSHI-CREDENTIAL-VAULT-V1\0";
const VERSION: u8 = 1;
```

Serialize an explicit versioned payload, encrypt it with
`ChaCha20Poly1305` and a random 12-byte nonce, authenticate on load, and map
all format/IO failures to a private coarse `CredentialVaultError`.

- [ ] **Step 4: Implement atomic replacement**

Create the parent directory, write a `NamedTempFile` beside the destination,
`sync_all` the file, persist it atomically, then best-effort sync the parent
directory. Add a test-only fault before persist and prove the previous payload
remains byte-for-byte unchanged.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test -p koushi-core credential_vault_file
```

Expected: all matching tests pass.

### Task 3: Lazy OS-Keychain migration and process cache

**Files:**
- Modify: `crates/koushi-core/src/store.rs`
- Modify: `crates/koushi-core/src/credential_vault.rs`
- Test: `crates/koushi-core/src/store.rs`

**Interfaces:**
- `OsCredentialStore::with_backend(data_dir, backend)` owns the legacy
  `CredentialStore`, `CredentialVaultFile`, and
  `Arc<Mutex<Option<CredentialVaultData>>>`.
- Every existing `CredentialStoreBackend` operation keeps its current
  signature.

- [ ] **Step 1: Write a failing migrated-startup read-count test**

Seed the new master-key entry and an encrypted vault containing two accounts.
Create a fresh `StoreActor::with_os_backend`, then call:

```rust
backend.load_last_session();
backend.load_saved_sessions();
backend.load_matrix_session(&first);
store.account_store_config(&first);
store.account_search_index_config(&first);
backend.load_matrix_session(&second);
store.account_store_config(&second);
```

Assert `get_password_count() == 1`.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p koushi-core migrated_credential_vault_reads_keychain_once
```

Expected: assertion failure because current operations read separate Keychain
entries repeatedly.

- [ ] **Step 3: Route OS operations through the cached vault**

Change only `CredentialStoreBackend::OsKeychain`. Its first operation loads the
master key and encrypted vault under one mutex; every subsequent operation
reads or mutates the decoded in-memory data. Persist mutations before replacing
the cached value.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p koushi-core migrated_credential_vault_reads_keychain_once
```

Expected: the test passes with exactly one backend read.

- [ ] **Step 5: Write failing legacy migration tests**

Cover:

- complete legacy state migrates all sessions and unlock secrets;
- a missing Matrix session or unlock secret leaves every legacy entry present;
- successful durable migration makes the vault authoritative;
- legacy deletion failure does not invalidate the vault;
- a master key without a vault resumes migration using that same key.

- [ ] **Step 6: Verify RED**

Run:

```bash
cargo test -p koushi-core legacy_credentials_
```

Expected: failures because migration is absent.

- [ ] **Step 7: Implement migration**

Load the legacy index and last-session pointer, collect and validate all
referenced credentials into memory, load or create the master key, persist and
re-open the vault, install it in the cache, then delete legacy entries
best-effort. Do not create a key or vault when a fresh installation only lists
saved sessions.

- [ ] **Step 8: Verify GREEN**

Run:

```bash
cargo test -p koushi-core legacy_credentials_
```

Expected: all migration tests pass.

### Task 4: Concurrency, fail-closed behavior, and compatibility

**Files:**
- Modify: `crates/koushi-core/src/store.rs`
- Modify: `crates/koushi-core/src/credential_vault.rs`
- Test: `crates/koushi-core/src/store.rs`

**Interfaces:**
- No new public API; this task hardens the Task 3 boundary.

- [ ] **Step 1: Write failing concurrency and corruption tests**

Start multiple threads against clones of one `OsCredentialStore`, synchronize
their first read with a barrier, and require one backend read. Corrupt the
vault, then require every credential operation to fail without creating or
overwriting credential files.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p koushi-core credential_vault_concurrent credential_vault_corrupt
```

Expected: one or both tests fail until initialization results are coalesced and
cached errors are handled consistently.

- [ ] **Step 3: Implement minimal synchronization and error caching**

Keep initialization, migration, and mutation under the shared mutex. Cache a
successful decoded vault; on error leave the cache uninitialized so a later
explicit operation may retry without ever treating a corrupt vault as empty.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p koushi-core credential_vault_concurrent credential_vault_corrupt
```

Expected: all matching tests pass.

### Task 5: Final verification and implementation commit

**Files:**
- Verify all modified files.

- [ ] **Step 1: Run focused suites**

```bash
cargo test -p koushi-key
cargo test -p koushi-core credential_vault
cargo test -p koushi-core legacy_credentials_
```

- [ ] **Step 2: Run repository gates**

```bash
cargo fmt --all -- --check
cargo clippy -p koushi-key -p koushi-core --all-targets -- -D warnings
cargo test -p koushi-core
git diff --check
```

- [ ] **Step 3: Audit the stated requirement**

Confirm from tests and source that:

- the migrated startup path performs one backend read;
- multiple accounts and derived-key consumers add no reads;
- migration preserves all credential classes;
- corruption and write failure are fail-closed;
- QA backends remain unchanged;
- no secret-bearing type has a revealing `Debug`.

- [ ] **Step 4: Commit implementation**

```bash
git add crates/koushi-key/src/lib.rs crates/koushi-core/src/lib.rs \
  crates/koushi-core/src/credential_vault.rs crates/koushi-core/src/store.rs
git commit -m "feat: consolidate credentials behind one keychain key"
```
