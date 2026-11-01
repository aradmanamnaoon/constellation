# Sync and backups: capability and recovery limits

## Local backup and recovery

The experimental desktop implements encrypted export and isolated recovery
of the two application SQLite databases. Windows private-desktop test evidence
includes live-WAL data recovery, no-clobber restoration to a new directory,
and verification that both recovered SQLite images pass `PRAGMA integrity_check`.
Other platforms, attached documents, global atomic snapshots across the
two databases and seamless activation of a restored project require further
tests. **Keep an additional independent backup of irreplaceable data.**

To test an existing build with disposable data, use its explicit backup/export
feature, select a new destination, store the encryption password separately,
then recover to a different empty directory. Verify the restored rooms,
messages and workflows against the source before relying on the archive.
Never put archive passwords in issue trackers, logs or the repo.

## Optional cloud sync is not yet operational in the GUI

`constellation-sync` contains an offline-tested encrypted revision protocol,
local compare-and-swap checks and an opt-in S3-compatible HTTPS adapter that
requires conditional writes. It rejects unsafe destinations and unsupported
conditional-write semantics. **No live third-party S3 service, two-device
encrypted restore, conflict resolution or GUI opt-in workflow has passed
acceptance.** Do not enter production cloud credentials in a development
build or assume it synchronizes across devices.

The eventual flow is opt in → verify a supported S3 endpoint's actual
atomic conditional-write behavior → encrypt before upload → preserve an
independent recovery key → pair a second device → test concurrent conflicting
edits and recovery → support revocation and rollback. An endpoint claiming
S3 compatibility is insufficient: it must demonstrate the exact required
CAS semantics. User-owned buckets, traffic and storage may cost money;
there is no operator-hosted free storage guarantee.

Reproduce the local protocol test suite with
`cargo test --manifest-path crates/constellation-sync/Cargo.toml --locked --offline`.
See tasks **P16–P17** in the
[`23-workstream plan`](mission/COMPLETE_EXECUTION_PLAN.md) and precise
[`acceptance inventory`](quality/acceptance-ledger.md).
