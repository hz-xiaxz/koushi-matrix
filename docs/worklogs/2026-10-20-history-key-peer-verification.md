# Historical recovery without manual peer verification

## Approved scope

The user authorized removing mandatory mutual manual verification for
historical-key recovery and merging the resulting PRs. Current room membership,
shared-history visibility, owner-signed devices, local blocks, verification
violations, exact device/master pins, encrypted transport and bounded replay
remain enforced. No outgoing Megolm sharing or invitation-window change is
included.

Canon consulted and amended: Repository Rules / Key Management, overview /
Security Model, state-machine / Historical key recovery, and the historical
recovery design. The guide, five shipped locale catalogs, SDK feedback notes,
and the QA token contract follow the new policy.

## Design review

Independent read-only security review approved the design subject to coherent
device/owner snapshots, final pin checks after awaits, actual forwarding
provenance, and negative coverage. Those requirements led to the SDK's narrow
`Device::cross_signing_sender_data` accessor: the public device wrapper
otherwise exposes its owner-signature decision and current identity through
different snapshots. The generic bundle import compares the supplied and
freshly resolved device's owner identity as well as its device keys.

Owner signatures establish that a device belongs to the presented identity;
they do not authenticate a first-contact user independently of the homeserver.
The approved tradeoff is recorded in the design's security assessment.

## Verification

- RED: the new SDK test rejected an owner-signed unverified forwarder.
- RED: production Core on disposable Synapse passed the offline/restart
  checkpoints, then timed out decrypting post-join history after manual
  verification was removed from the fixture.
- GREEN: focused SDK export/import tests cover unverified provenance,
  idempotence, unsigned locally verified devices, blocked devices, verification
  violations, and atomic rejection of mixed-room bundles.
- The production Core scenario now asserts unverified identities before and
  after recovery, retains existing restart/replay/security cases, and rotates
  only owner master keys during paused disclosure and import.

## Integrated review and gates

Independent read-only review found no blocking security or correctness issue
in the complete Core/SDK diff, including the new race tests. Its conditions
were satisfied by the following passing checks:

- SDK crypto library: 544 passed, 1 ignored.
- SDK public facade identity checks: 2 passed.
- Disposable Synapse production Core scenario: passed, including
  `history_recovery_without_manual_verification`,
  `history_recovery_identity_race_guard`, cached replay and cleanup tokens.
- Frontend: 2,162 tests passed; typecheck and lint passed.
- QA all-target compilation, architecture/QA structure checks, documentation
  links, SDK-submodule guard, secret scan, release structural guard, formatting
  and diff whitespace checks passed.

The full workspace, merge-gate clippy matrix, both local homeserver smoke
lanes and required hosted CI are checked before merge.
