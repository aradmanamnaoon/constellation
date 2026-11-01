# Constellation Runner Protocol (offline primitives)

This is an **effect-free authentication library with bounded I/O framing
helpers**, not a standalone runner implementation. It defines
bounded versioned JSON envelopes authenticated with HMAC-SHA256 and explicit
workstation/runner identities, attribution, permission scope, job/cancellation/
result/receipt correlation and a locally approved pairing state.

**There is no network transport, laptop-off continuation, remote daemon,
OS/tool execution, scheduler, or automatic enrollment in this crate.**
Nothing here gives a model operating-system access or authorizes a requested
capability; a future trusted host MUST independently enforce permissions.

## Contract

### Stdio framing (sixth-wave additive codec)

`read_frame(&mut impl Read)` and `write_frame(&mut impl Write, bytes)`
encode a 4-byte big-endian unsigned payload length followed by 1–16,384
bytes. EOF is valid only between frames; truncated headers, payloads,
zero-length frames and excessive declared lengths fail closed *before*
allocating an unbounded buffer. These helpers never open sockets and do
**not** authenticate frames: use `Endpoint::receive_authorized` on every
decoded signed payload. The standalone `constellation-runner-service`
crate adds a local stdio-only binary and operator-controlled durable inbox.

- `Pair::enroll(workstation_id, runner_id, secret)` begins **pending**. Only an
  explicit trusted local `approve()` makes it usable. A deployment must
  securely exchange and approve a **random 32-byte** secret out of band.
- Two `Endpoint` instances (`Side::Workstation`, `Side::Runner`) use the same
  pair IDs and approved secret. `issue_job`, `issue_cancel`, `issue_result`
  and `issue_receipt` yield signed JSON; `receive` accepts only the peer's
  authenticated messages and checks sequence, timestamp, expiry, identity,
  attribution, permission scope, and correlated message digests.
- `issue_bound_job` additionally signs `TaskAttribution { owner_id, run_id,
  task_id }` together with the pair identities, key epoch, job ID, agent ID,
  expiry and exact capability/resource. Cancel, Result and Receipt inherit the
  same attribution, and recoverable job ledgers reject conflicting follow-up
  attribution. `issue_job` remains the unbound legacy option; attribution is a
  peer assertion, never a locally approved OS permission.
- For uncertain receipt delivery, `issue_receipt_reconciliation(original_sequence,
  ttl, clock)` creates a *new* signed and expiring `ReconcileReceipt` request.
  The receiving endpoint verifies the original message's recorded SHA-256,
  job/agent identity, owner/run/task and exact scope against its authenticated
  checkpoint. A host can issue a fresh receipt referencing the original
  message, even after the original frame expired, without re-accepting its
  Job/Cancel. Recovery requests consume bounded sequence capacity and reject
  replays, forged commitments and unknown originals.
- `reissue_result(job_id, ttl, clock)` signs a fresh result carrying the
  previously checkpointed outcome and output SHA-256 commitment. This
  cannot change the result or execute a tool. The workstation classifies a
  consistent new-sequence repeat as `DuplicateResult`; a conflicting outcome
  or digest fails. Old checkpoints without a result digest cannot reissue
  that historical result.
- `receive_authorized` verifies the envelope and invokes a **pure** local
  authorization callback for each new job before recording it. It returns the
  authenticated `Message` alongside its `ReceiveOutcome`. A denial does not
  advance sequence counters. The general `receive` API is a lower-level
  authentication primitive and never grants local tool execution.
- `rotate_key` changes the epoch and invalidates old messages and outstanding
  in-flight jobs; it rejects reuse of the previous secret. `revoke` blocks
  the endpoint until a new,
  explicitly enrolled pair is approved. No old epoch keys are retained.
- `checkpoint` emits an HMAC authenticated, bounded canonical snapshot of
  replay counters, jobs and receipts. `checkpoint_generation` increases on
  every new issued/received message and key rotation. `Endpoint::recover`
  verifies an externally pinned minimum generation and the approved pair's
  identity, epoch, secret and message policy. `Pair::resume_approved` must only
  reconstruct a pair from a trusted, rollback-protected host key store.
- `Clock` is injected for deterministic/offline operation; caller supplies
  trusted time. Default policy: expiry at most 5 minutes after issue, at most
  30 seconds into the future, and **never** accept an expired envelope.
- At most 1,024 tracked outgoing/incoming messages and 1,024 jobs per epoch,
  with a 16 KiB maximum wire JSON, 2 MiB checkpoint, 4 KiB instruction limit, and ASCII bounded
  identities/resource references. Full stores refuse additional writes rather
  than silently evict anti-replay or receipt state.
- An exact duplicate receipt is reported as `DuplicateReceipt` (no repeated
  transition). Duplicated jobs and cancellations are always rejected.
  Reissued results are accepted only with the *same* authenticated outcome and
  output hash, and return `DuplicateResult` without repeating a job transition.
  A different signed message using a reused sequence is rejected too.
- `Secret` is 32 bytes, cannot be serialized or debug-printed, and zeroizes
  on drop. Authentication failures have generic messages with no secrets.

### Example

```rust
use constellation_runner_protocol::{
    Capability, Clock, Endpoint, MessagePolicy, Pair, PermissionScope, Secret, Side
};

struct FixedClock;
impl Clock for FixedClock { fn now_ms(&self) -> i64 { 1_000_000 } }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = [0xA5_u8; 32]; // Demo ONLY: use a CSPRNG for real pairing secrets.
    let mut ws_pair = Pair::enroll("laptop-1", "runner-1", Secret::from_bytes(key)?)?;
    let mut runner_pair = Pair::enroll("laptop-1", "runner-1", Secret::from_bytes(key)?)?;
    ws_pair.approve()?;
    runner_pair.approve()?;
    let mut workstation = Endpoint::new(Side::Workstation, ws_pair, MessagePolicy::default())?;
    let mut runner = Endpoint::new(Side::Runner, runner_pair, MessagePolicy::default())?;
    let clock = FixedClock;
    let wire = workstation.issue_job(
        "job-1", "agent-1",
        PermissionScope { capability: Capability::ReadEvidence, resource_id: "doc-1".into() },
        "Summarize this source without tools", 20_000, &clock
    )?;
    let accepted = runner.receive(&wire, &clock)?;
    assert!(accepted.is_new());
    Ok(())
}
```

## Trust and persistence boundaries

Neither a shared HMAC key nor a valid signed `PermissionScope` substitutes for
runner-side host authorization. The key must remain out of logs, source,
workspace sync and any LLM context. Pairing is local state only: this crate
does not perform a secure handshake, certificate/device attestation, key
storage, transport encryption, or connect devices. Messages contain plaintext
instructions; they require an authenticated **encrypted** transport in a
future integration.

Replay state is now serializable to authenticated checkpoints, but **this crate
does not write any durable state**. To prevent replay across crashes, the host
MUST commit the checkpoint and an independently rollback-protected generation
watermark before emitting any wire message, acknowledging delivery, or starting
an effect. On incoming jobs, the host must also durably queue the authenticated
instruction and its job identity in the same transaction before scheduling
execution: checkpoint state alone deliberately excludes instructions. Revalidate
actual permissions at execution time. On storage failure, discard that in-memory
endpoint and recover the last fully committed state without running effects.
To recover a lost result the host must transactionally commit its original
result's output digest/outcome and the endpoint checkpoint before transmitting
the wire. On restart, only `reissue_result` is permitted: the output bytes and
any original effects are outside this library and are never regenerated here.
The host must also checkpoint each reconciliation request before sending it,
and each newly created receipt before releasing it to the transport.
The host must securely persist and pin approval/revocation and the key epoch;
`minimum_generation` cannot be read from the untrusted checkpoint itself.

An old checkpoint can admit replay if the independent watermark or secret store
is also rolled back. `Endpoint::new` always starts fresh and must be used only
for newly enrolled secrets; recovering with an old approved key requires
`Endpoint::recover`. If durable state is unavailable, fail closed and enroll a
**new random key and epoch** before exchanging any messages. A durable
transactional job queue, encryption at rest, trusted OS key store, snapshot
atomicity, rollback anchor, provider authorization and real native integration
still need implementation. Key rotation on both devices is coordinated out of
band; it invalidates old in-flight work and receipts. All these requirements
also apply when restoring a workstation after a crash.

No automatic retry, scheduling, execution, cancellation of a real operation,
or laptop-off continuation is claimed. Explicit receipt reconciliation
does not establish network delivery or transport-level acknowledgement. A `Result`
contains an output SHA-256 commitment, not the output itself. A future host
must independently validate any actual output bytes against that digest.

## Verification

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'constellation-runner-target'
cargo fmt --manifest-path crates/constellation-runner-protocol/Cargo.toml --check
cargo test --manifest-path crates/constellation-runner-protocol/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-runner-protocol/Cargo.toml --all-targets --offline -- -D warnings
```
