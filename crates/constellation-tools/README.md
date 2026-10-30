# Constellation tools: local execution contract

This Rust library implements a small, useful local execution layer. It is
**not yet connected to Tauri or the application scheduler**. The library
does not accept model output as permission to execute anything.

## What is implemented

- Text reads through `WorkspaceReader` and `OperationRequest::Read`.
- Create-only writes and SHA-256 compare-and-replace writes.
- Explicit absolute executable invocation with structured `argv`, a validated
  workspace-relative working directory, a deadline and bounded output.
- Git status/diff with fixed arguments. Git push requires its own
  `ToolAction::GitPush` grant, an exact HTTPS destination URL and branch,
  and a distinct one-use approval. Local Git URL rewrites and includes are
  refused for push. Push is **not** exercised against a live remote in tests.
- One-use `OperationApproval` with an operation ID and SHA-256 action digest.
  The digest binds the agent, room, task, permission resource, executable
  contents, cwd, arguments, timeout, write bytes and write mode.
- `ExecutionReceipt` reports the attempted action and its terminal outcome,
  error reason, exit status and output hash. Repeated IDs cannot re-execute.
- Cooperative `CancellationToken`: cancels pending work and terminates a
  directly spawned process. A 120-second ceiling applies to process timeouts.

## Integration sequence

```rust,no_run
use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    CancellationToken, LocalExecutor, OperationContext, OperationRequest, WriteMode,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let executor = LocalExecutor::new(root, 1_048_576, 65_536)?;
    let context = OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    };
    let prepared = executor.prepare(
        context,
        OperationRequest::Write {
            path: "approved-note.txt".into(),
            bytes: b"example".to_vec(),
            mode: WriteMode::CreateNew,
        },
    )?;

    let preview = prepared.preview();
    // Trusted native UI MUST display preview and collect explicit consent.
    // Only after that consent may trusted Rust host code create a grant:
    let mut gate = PermissionGate::new();
    gate.grant(preview.scope)?;
    let approval = executor.approve(&gate, OperationId::new(), &prepared)?;
    let cancellation = CancellationToken::new();
    let (receipt, _result) =
        executor.execute(&gate, &approval, prepared, &cancellation)?;
    // Persist the receipt and consent in the core event store before claiming
    // durable audit or crash-safe replay protection.
    assert!(executor.receipt(receipt.operation_id).is_some());
    Ok(())
}
```

The Tauri layer should be the only caller authorized to issue grants and
one-use approvals. The agent proposes an operation; Rust canonicalizes it and
provides `PreparedOperation::preview()`; the user approves the exact preview;
the host grants the scope; and `execute` checks the scope again. The host must
also associate cancellation with the durable task and record receipts in the
authoritative `constellation-core` event log.

## Boundaries and remaining security work

The approval ledger is **process-local and volatile**. It prevents duplicate
execution within one session, but process restart does not guarantee exactly
once effects. Approval/receipt persistence must be coordinated with the core
store before unattended or durable task execution.

Canonicalization and reparse checks reduce accidental path escape, but do not
prevent a malicious local process with filesystem access from racing path
replacement. Use handle-relative, OS-native file operations and restrictive
workspaces for adversarial directories. File replacement stages and syncs
content, but no multi-operation filesystem transaction is provided.

Cancellation terminates the direct child, not every descendant process. It
cannot undo completed filesystem operations, a Git push, remote requests or
other already completed effects. A future process runner needs child-tree
containment (Windows Job Objects / Unix process groups), stronger OS sandbox
policies, network egress restrictions and persistent cancellation state.

An explicitly approved general `ExecuteCommand` grant permits arbitrary
executables, including interpreters and shells. The native UI must treat
these as high-risk operations and expose the exact executable, executable
hash, arguments and effects. Git push additionally needs live endpoint
identity verification and stronger repository isolation for untrusted
worktrees. This crate alone is not a privilege boundary against a malicious
process already running under the same operating system account.

## Verification

```sh
cargo test --manifest-path crates/constellation-tools/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-tools/Cargo.toml --all-targets --offline -- -D warnings
```
