# User-owned runner: current capability and deployment gate

Constellation's long-running remote worker is **not deployable yet**. The
`constellation-runner-protocol` crate models authentication, pairing,
revocation, bounded signed checkpoints and replay checks in offline tests.
There is no production runner daemon, Linux `systemd` service, durable
scheduler, paired desktop transport, container image, verified download or
installed mobile monitor. Accordingly, **a desktop task does not continue
after the computer turns off** in this build.

The intended arrangement is a user-owned Linux machine or VM, connected via
a user-configured private network or authenticated channel. It must accept
explicit task and tool scopes, reject replay and unauthorized agents, isolate
projects and secrets, retain bounded receipt and cancellation state, and
restore or cancel work after reboot. Pairing, trust keys and all granted
capabilities must be inspectable and revocable on both ends. Models, providers,
VM resources and any storage are the user's own resources and may incur costs.

## What must be verified before enablement

1. Package an actual runner process for the target Linux distribution and
   document a minimal-privilege service account and restart behavior.
2. Pair two real machines through an authenticated transport without exposing
   an unauthenticated administration port. Confirm revocation and key rotation.
3. Execute a bounded, independently approved fixture task after closing the
   desktop, and verify durable task state across a runner restart.
4. Exercise offline/reconnect, cancellation, process descendants, network
   limits, malicious payloads, clock skew, duplicate messages and backup
   restore with matching audit receipts.
5. Publish versioned binaries, signature/checksum and rollback instructions
   with fixed source/CI provenance.

The source tests can be run from the repository root with
`cargo test --manifest-path crates/constellation-runner-protocol/Cargo.toml --locked --offline`.
These prove protocol invariants only. See
[`docs/mission/active-task-plan.md`](mission/active-task-plan.md) task **P18**.
