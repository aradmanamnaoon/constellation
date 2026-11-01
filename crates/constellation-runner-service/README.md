# Constellation runner service — local authenticated inbox

This is an executable **local stdio** runner inbox and operator CLI with a
separate **explicitly opt-in localhost TCP** listener, not a network-paired
remote daemon and **not an OS tool executor**. It accepts
bounded HMAC-authenticated messages from an out-of-band provisioned workstation,
atomically records jobs in a private SQLite queue, and returns signed receipts
only after committing its recovery state. Pending jobs, cancellations, exact
scope grants, revocation and replay counters survive process restarts.

## Operator CLI

```text
constellation-runner-service generate-key /private/runner.key
constellation-runner-service init /private/runner.sqlite /private/runner.key workstation-1 runner-1
constellation-runner-service status /private/runner.sqlite /private/runner.key
constellation-runner-service approve-pair /private/runner.sqlite /private/runner.key
constellation-runner-service serve-stdio /private/runner.sqlite /private/runner.key
constellation-runner-service serve-loopback /private/runner.sqlite /private/runner.key 43781
constellation-runner-service list /private/runner.sqlite /private/runner.key
constellation-runner-service grant /private/runner.sqlite /private/runner.key job-1 draft_text doc-1
constellation-runner-service grant-bound /private/runner.sqlite /private/runner.key job-1 owner-1 run-1 task-1 draft_text doc-1
constellation-runner-service revoke-grant /private/runner.sqlite /private/runner.key job-1
constellation-runner-service revoke-pair /private/runner.sqlite /private/runner.key
```

`generate-key` uses the OS CSPRNG to create a *new* private 32-byte key
file as 64 hexadecimal characters (mode 0600 on Unix). Secrets are never
hardcoded, placed in SQLite or printed. On Unix the service rejects
group/other-readable key or database files and symlink database paths.
The operator must independently transfer the key securely and verify both
peer identities before `approve-pair`. This is local user approval, **not**
remote device attestation or a secure pairing ceremony. Revocation is
irreversible for the database/key; start with a fresh pair and key.

`serve-stdio` reads at most 1,024 messages, each as a 4-byte big-endian
length and 1–16,384 bytes of canonical HMAC-authenticated JSON. Truncated,
oversized, invalid, expired, replayed or wrong-key messages fail closed.
Each successful Job is queued as `pending` with **no execution authority**.
An authenticated Cancel marks a job `cancelled`. A local operator can grant
only an exact job ID/capability/resource, but no shell, file or process
execution exists even after granting. The server writes only a framed
authenticated receipt **after** committing the inbox and checkpoint.
It never prints private instructions or rejected message bodies.

If a Job or Cancel receipt is committed but lost during transport, the
workstation may send a fresh signed `issue_receipt_reconciliation` request
referencing the original sequence and exact signed digest. The service verifies
its authenticated durable receive ledger, including pair, job, agent,
owner/run/task and requested capability/resource, and creates a **new** signed
receipt for the original message. The old instruction or cancellation is never
reapplied. The new request, new receipt, checkpoint and replay generation are
committed atomically before sending anything. Fresh reconciliation works after
the original frame expired; reusing the original wire still fails closed.
Wrong same-key task/scope/digest claims, stale query replays, revoked pairs and
corrupted checkpoints cannot recover a receipt. This works over stdio and the
explicitly enabled localhost TCP listener; the workstation controls retries.

The protocol also exposes `Endpoint::reissue_result` for hosts that durably
saved an original outcome and output SHA-256 digest. This service does **not**
produce results or run jobs; outcome retransmission is a protocol primitive
for a future trusted executor, not a real command-execution feature.

An opt-in signed `issue_bound_job` envelope carries explicit owner ID, run ID
and task ID alongside the signed workstation/runner device identities, job ID,
agent ID, key epoch and exact requested capability/resource. These values are
persisted with the inbox across restarts. `grant-bound` requires the operator
to enter the **exact** owner/run/task/job/scope combination; the legacy
`grant` command cannot approve a bound job. Cancels retain signed attribution
and durably remove a grant. Existing version-1 SQLite inbox files are migrated
transactionally to schema version 2 while keeping signed checkpoint and grant
state. Neither command enables OS execution.

### Optional authenticated localhost TCP ingress (fourteenth wave)

`serve-loopback DB KEY PORT` must be **explicitly started by the local
operator** with a numeric, nonzero port. It binds **only** to `127.0.0.1`;
there is no public address, wildcard bind option, default network listener,
service autostart, or remotely selectable network target. It refuses to bind
before the independent `approve-pair` action. The standalone
`serve_loopback` library API similarly rejects a non-loopback listener.

Every new connection uses **v2 mutual, domain-separated HMAC-SHA256 proof of
possession** of the previously provisioned 32-byte shared key, using fresh
OS-CSPRNG 32-byte server/client nonces. The transcript is bound to a SHA-256
commitment of the operator-approved workstation ID, runner ID and key epoch.
The `authenticate_loopback_client_bound` API additionally rejects a mismatched
expected peer identity or stale epoch *before* sending its client proof.
The older local-only `authenticate_loopback_client` helper still authenticates
the server-advertised binding, but callers requiring a known peer must select
the bound API. Protocol-v1 (`CRLS1`/`CRLC1`) handshakes are rejected.
A wrong key/replayed handshake closes
the connection and cannot receive a signed receipt. The challenge is public
random data, not a credential or approval. The host then requires the
**existing** signed canonical message protocol with its identity/epoch,
expiry, replay, cancel and durable SQLite receipt checks: the handshake
cannot grant a scope, enqueue instructions without a signed message, or
execute tools. Job data/receipts are **plaintext on TCP loopback** after
authentication; this is not a confidentiality-safe remote or LAN transport.

The server accepts at most 1,024 connections per invocation, counting failed
handshakes; 32 framed protocol messages per connection, with 16 KiB maximum
per message, a 3-second absolute handshake deadline and a 30-second absolute
session deadline. Neither a slow-dripping TCP byte stream nor an invalid
HMAC gets a permanently held session or an authenticated response. Accepted
connection errors are isolated. Each new connection reloads the approved
pair from the owner-restricted DB, and **each signed frame** checks approval
and commits signed messages under the existing transactional SQLite replay
checkpoint. Separate local CLI `revoke-pair` takes effect on subsequent
acceptance, including on a running daemon. A denied request gets no receipt.

`authenticate_loopback_client` is an additive Rust helper for a client that
already possesses the independently shared key. It only connects to
loopback and performs mutual proof; it does not allocate privilege, sign
jobs, or perform a remote network discovery/pairing ceremony.

SQLite `runner_state` includes a signed Endpoint checkpoint/generation;
`inbox` contains job instructions and scopes. Both and the outgoing receipt
sequence update in one `BEGIN IMMEDIATE` transaction. A failed inbox insert
rolls back the entire acceptance/replay change; concurrent independent
processes accept a signed Job once. Queue capacity is 1,024 jobs per pair.

## Limitations and integration

- Neither stdio nor loopback TCP is an encrypted network channel. The optional
  TCP listener is limited to loopback, implements mutual proof and bounded
  connections/timeouts, **not** TLS/SSH/mTLS, remote pairing, remote egress,
  automatic long-term retries or a secure operating-system vault. The shared
  key must be independently provisioned and protected; another local process
  with the key can submit signed inert jobs. Do not expose/tunnel this socket
  across a network without a separate independently reviewed encrypted
  tunnel and authentication design.
- WAL/FULL supplies SQLite transactional durability, not anti-rollback
  protection against a privileged local attacker restoring both the
  database and secret. Production needs a separately trusted generation
  watermark and key-epoch/revocation anchor. Expired receipt loss is now
  recoverable only while both peers retain matching signed per-epoch ledgers,
  have available bounded sequence capacity and can create a fresh signed
  reconciliation request. No automatic retry/scheduler or remote transport
  delivery guarantee exists.
- Job/owner/run/task IDs are signed claims from an out-of-band paired key and
  still require a local operator grant. There is no remote identity provider,
  run scheduler, end-to-end task supervision, or Linux service integration.
- Instructions are plaintext inside the owner-restricted SQLite file;
  encryption at rest and explicit off-device data consent are **missing**.
  Restrict access to its directory and avoid sensitive data in this slice.
- An authenticated PermissionScope is a request, not a grant. A future tool
  executor must recheck grants transactionally immediately before each
  effect, implement an OS sandbox, durable task supervision, explicit local
  consent and actual cancellation. Current `grant` only changes metadata;
  it does not execute any user commands. No native UI/Android client or
  Linux package/service unit is connected; this wave tests on Windows only.

## Offline Windows build checks

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp\constellation-prime-core-independent-20261031'
cargo test --manifest-path crates/constellation-runner-protocol/Cargo.toml --locked --offline --all-targets
cargo test --manifest-path crates/constellation-runner-service/Cargo.toml --locked --offline --all-targets
cargo clippy --manifest-path crates/constellation-runner-protocol/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo clippy --manifest-path crates/constellation-runner-service/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo fmt --manifest-path crates/constellation-runner-protocol/Cargo.toml --all --check
cargo fmt --manifest-path crates/constellation-runner-service/Cargo.toml --all --check
```

The fourteenth-wave tests additionally use actual localhost TCP connections,
an actual separate `serve-loopback` CLI process plus a second operator CLI
process that revokes its key, and real signed protocol Job/Cancel receipts.
The negative fixtures cover invalid shared keys, replay across independent
connections, unsigned frames before the handshake, tampered signed frames
after it and oversized frame lengths. **These are Windows-local service tests,
not Linux installed integration, remote encrypted runner pairing, continuous
24/7 scheduling, or native application acceptance.**
