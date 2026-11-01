# Constellation tools: local execution contract

This Rust library implements a small, useful local execution layer. A narrow
manual file-action adapter now exists in the native Tauri host for explicitly
reviewed reads and create-new writes inside an app-owned private folder.
The standalone Process, Git, replace and external-path operations are **not**
connected to the desktop UI or an autonomous model-driven scheduler.
The library does not accept model output as permission to execute anything.

## What is implemented

- Text reads through `WorkspaceReader` and `OperationRequest::Read`.
- Create-only writes and SHA-256 compare-and-replace writes.
- Explicit absolute executable invocation with structured `argv`, a validated
  workspace-relative working directory, a deadline and bounded output. The
  permission resource binds the executable, cwd, argument fingerprint and
  timeout. The operation digest additionally binds the approved working
  directory's OS file identity; a replaced ordinary directory fails closed.
  Windows holds non-share-delete handles for its ancestor chain through spawn.
  Unix opens the exact directory through `O_NOFOLLOW` handles and calls
  `fchdir` on that descriptor in the child before executing the program.
- Git status/diff with fixed arguments. Git push requires its own
  `ToolAction::GitPush` grant, an exact HTTPS destination URL and branch,
  and a distinct one-use approval. Local Git URL rewrites and includes are
  refused for push. Push is **not** exercised against a live remote in tests.
- Git subprocesses use isolated PATH restricted to the approved executable's
  own parent directory. The Git child receives neither ambient Git variables
  nor global/system configuration; status/diff prohibit all Git transports.
  Explicit local repository executable-capable settings are denied on prepare
  and rechecked at dispatch. Other Process operations are unchanged.
- One-use `OperationApproval` with an operation ID and SHA-256 action digest.
  The digest binds the agent, room, task, permission resource, executable
  contents, cwd identity, arguments, timeout, write bytes and write mode.
- `ExecutionReceipt` reports the attempted action and its terminal outcome,
  error reason, exit status and output hash. Repeated IDs cannot re-execute.
- Optional private, append-only JSONL approval journal via
  `LocalExecutor::with_journal(root, max_file_bytes, max_output_bytes, path)`.
  It syncs approved/started/completed records before continuing. Reopening
  restores receipts, but **never** revives pending or interrupted approvals.
  Journal corruption, size limits and concurrent owners fail closed.
- `LocalExecutor::with_authenticated_journal` verifies an ordered HMAC-SHA256
  chain using a caller-managed 32-byte key stored separately from the journal.
  `LocalExecutor::deny(operation_id, &prepared)` records a rejection that
  prevents the operation ID from being approved or dispatched later.
- Create-only writes are staged and synced, then atomically published with a
  no-overwrite hard link. Interrupted staging does not leave a partial target.
- Cooperative `CancellationToken`: cancels pending work. On Windows, spawned
  commands and their descendants are attached to a Job Object with
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and the complete job is terminated on
  cancellation, timeout, output-limit or completion of the direct child.
  On Linux and macOS the executor creates an independent process session,
  verifies its group identity and terminates that group on cancellation,
  timeout, output-limit or direct-parent exit (see the caveats below).
  A 120-second ceiling applies to process timeouts.

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
    // The host must link this receipt and its consent to the core event store.
    assert!(executor.receipt(receipt.operation_id).is_some());
    Ok(())
}
```

The Tauri layer now hosts the bounded manual file workflow. It binds a pending
saved task to its actual room and agent, canonicalizes its private path and
provides `PreparedOperation::preview()`; the UI explicitly requests
approval, and Rust grants only the original exact scope. OS-level independent
human consent, a model proposal loop, and atomic receipt association with
the authoritative `constellation-core` event log remain open production gates.

## Durable journal configuration

Use `LocalExecutor::with_journal(..., absolute_journal_path)` for durable
approvals and receipts. Create a private, pre-existing journal **parent
directory outside the agent's workspace**. Its append-only log is bounded to
32 MiB and 100,000 records, and includes digests, scopes and outcomes, but not
file content or process output. Because scopes include absolute paths, the
host must protect the directory from untrusted local users. On Unix, newly
created files use mode `0600`; on Windows, use an appropriate inherited ACL.

The journal takes an exclusive `.lock` sidecar. A stale lock left by a crash
requires deliberate host recovery after verifying that no executor owns the
log. Never automatically remove a lock on startup. Corrupt or incomplete
records fail closed, and incomplete executions are permanently consumed.
This prevents replay, but **does not provide exactly-once external effects**:
the effects of a crashed operation may have occurred even when no completion
receipt exists.

For tamper detection, use
`LocalExecutor::with_authenticated_journal(root, max_file_bytes, max_output_bytes, journal_path, key)`.
The trusted host must load the same unpredictable 32-byte key from storage
separate from the journal on every launch (for example, the OS credential
store). The key is never saved to the log. Modified records, reordered or
inserted records, a different key, transplantation to another canonical
journal path, and mixing with the older unsealed JSONL format cause opening
to fail. Explicit denials are synced to the
journal and remain denied across restarts. As with approvals, only trusted
host code may record the user's denial decision.

An HMAC chain cannot detect deletion of a valid trailing sequence or
restoration of an older complete journal without a trusted checkpoint stored
outside the journal. The host needs an independently stored last tag or
monotonic checkpoint for rollback detection. The unsealed `with_journal`
format provides no cryptographic tamper protection, and neither mode provides
OS sandboxing or exactly-once execution after a crash.

## Boundaries and remaining security work

The default constructor `LocalExecutor::new` still has a **volatile** approval
ledger. The optional durable journal is independent from the authoritative
`constellation-core` event store; the integrating host must reconcile both
for unattended task execution.

Canonicalization and reparse checks reduce accidental path escape, but do not
prevent a malicious local process with filesystem access from racing path
replacement after the last validation. Use handle-relative OS-native file
operations and restrictive workspaces for adversarial directories. File
replacement stages and syncs content, but no multi-operation filesystem
transaction is provided. Atomic create-only publishing requires a filesystem
that supports hard links; unsupported filesystems fail without publishing.

**Process/Git cwd race mitigation:** At preparation, the executor records the
OS identity of the approved workspace working directory. At dispatch it pins
the directory by handles and compares that identity again. A rename followed
by a replacement directory at the same pathname fails even if neither object
is a symlink. On Windows, `FILE_SHARE_DELETE` is excluded from the open
handles for every ancestor, so their ordinary rename/reparse replacement is
denied while the handles are held. On Unix, the child changes directory using
`fchdir` on the descriptor opened with `O_NOFOLLOW`, not through a mutable
pathname; opening and checking the directory happens before spawn. This
protects the *initial working-directory object* for Process and Git. It does
not restrict where an approved executable can subsequently traverse, write,
execute, or connect. External code with sufficient OS privileges can override
ordinary file-handle restrictions; Git config, hooks/helpers and the executable
path have distinct remaining race and policy boundaries.

On Windows the executor creates the kill-on-close Job Object first, launches
the approved executable with `CREATE_SUSPENDED` and `CREATE_NO_WINDOW`, assigns
the suspended process to the job, then finds and resumes its initial thread
through documented Toolhelp/OpenThread/ResumeThread Win32 APIs. Assignment or
resume failure kills and reaps the child before returning an error, including
when the host's own job policy denies nested job assignment. This removes the
normal spawn/assign interval in which the first child could run and spawn
unmanaged descendants. The job does not set breakaway permissions. The code
also terminates the job when the direct child exits, preventing descendants
holding stdout/stderr pipes from continuing after a successful command.

These guarantees assume ordinary Windows process creation with one suspended
initial thread, a trustworthy OS and no privileged external actor tampering
with thread state, process handles or job membership. Windows Toolhelp does not
return the primary thread handle from `Command::spawn`; its snapshot and
resume operations can fail, in which case execution fails closed. A hostile
already-running process with equivalent or elevated OS privileges can interfere
and is outside this containment guarantee. After `ResumeThread`, there is a
normal polling interval (currently 20 ms) before cancellation can be observed,
but the job is already assigned for this entire interval. This is **process
lifetime containment, not a sandbox**: it cannot undo completed filesystem
operations, Git pushes, network requests or inherited privileges. Windows
ACLs, restricted tokens, filesystem isolation, egress policy, persistent
cancellation state and further security reviews are separate requirements.

On Linux and macOS, `LocalExecutor` runs **both Process and Git commands**
through a new POSIX session (`CommandExt::pre_exec` calls `setsid` before
`exec`). If `setsid` fails, Rust's spawn protocol refuses the command before
its executable starts. The host confirms the direct child's `getpgid(pid)`
equals its unique PID and differs from the host's own group; invalid identity
fails closed. On cancellation, timeout, output-limit and direct-parent exit,
the executor sends `SIGKILL` to **that isolated group only** (`kill(-pgid,
SIGKILL)`) and reaps the direct child. A Unix guard also terminates the group
if a post-spawn error unwinds the operation. It never signals group zero, group
one, or the executor's group. Completion detection uses `waitid(P_PID,
WNOWAIT | WNOHANG | WEXITED)`, retaining the direct child as an unreaped zombie
until the group has been killed, so its PID/PGID cannot be recycled for an
unrelated process group before cleanup. The normal polling interval is 20 ms.

This is a **best-effort cooperative process-lifetime boundary**, not a Unix
security sandbox. A child can call `setsid` or `setpgid` to leave the original
group, create a new session, run privileged commands, access permitted files,
or make network requests. Reaping direct child and killing the group cannot
undo effects already completed; neither descendants that escaped the group nor
system daemons they launched are contained. Stronger isolation requires an
OS-enforced cgroup/supervisor, restricted credentials, sandbox policy, egress
restrictions, and trusted platform tests. The Linux-specific process-group
suite was executed in **Ubuntu x86_64 under WSL on 30 October 2026** after a
test-only mutable-binding compile fix: cancellation, timeout, output-limit
and direct-parent exit all passed with an independent control process left
alive (four passing tests, three intentionally ignored child fixtures).
These results do not establish native macOS behavior or a Linux desktop
package. Unknown Unix families also require their own platform checks.

An explicitly approved `ExecuteCommand` grant permits an exact invocation of
an arbitrary executable, including interpreters and shells. The native UI
must treat these as high-risk operations and expose the executable, hash,
arguments, working directory and effects. Git push additionally needs live
endpoint identity verification and stronger repository isolation for
untrusted worktrees. This crate alone is not a privilege boundary against a
malicious process already running under the same operating system account.

### Git-specific isolation and remaining boundaries

Git does **not** inherit the host's PATH, global/system config, interactive
credential prompts, or ambient `GIT_*` settings. Its child receives a
restricted executable-parent PATH, `GIT_CONFIG_NOSYSTEM=1`, a null global
config, `GIT_CONFIG_COUNT=0`, `GIT_TERMINAL_PROMPT=0`,
`GIT_NO_LAZY_FETCH=1`, `GIT_OPTIONAL_LOCKS=0` and
`GCM_INTERACTIVE=never`. `Status` and `Diff` allow no Git transports;
the separately approved `Push` allows only HTTPS. Fixed hook and external
diff overrides remain in argv. Git push signing and recursive submodule
pushes are disabled because they can trigger additional unreviewed commands
or remote effects.

The local `.git/config` is inspected both before approval and immediately
before execution. Dangerous sections (`filter`, `diff`, `credential`,
`include`/`includeIf`, `url`, unknown extensions) and process-capable
options (e.g. `core.fsmonitor`, `core.hooksPath`,
`core.sshCommand`, `core.worktree`, remote helper overrides) fail closed.
Common ordinary repository core, user, branch and remote settings still work.
A Git action refuses a non-`git`/non-`git.exe` executable *at dispatch*,
in addition to the original canonical executable SHA-256 and cwd identity
binding. This does not authenticate the vendor or signature of a file merely
named Git.

**This is not an OS or network sandbox.** A same-privilege adversary may
replace executable/repository data after the final identity/config check;
custom or nested Git state can expose further attack surface. Linked
worktrees with a `.git` pointer file, uncommon configuration sections,
and installations needing helpers outside the Git executable directory may
be refused instead of silently widening trust. Do not loosen the controlled
PATH to make an unusual installation pass. The Process permission continues
to permit the explicitly approved executable with its prior environment;
the Git-only restrictions do not reduce that separate risk.

Local Windows tests executed real `git init`/`git add`, approved
`git status`/`git diff`, and adversarial config/PATH fixtures in temporary
repositories. No `push`, `fetch`, Git remote network operation, package
installation, code signing, cross-platform certification, or kernel isolation
was performed. The existing full human approval protocol was not changed.

## Verification

```sh
cargo test --manifest-path crates/constellation-tools/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-tools/Cargo.toml --all-targets --offline -- -D warnings
```

On Windows `tests/windows_process_tree.rs` launches an approved test child,
which launches an actual grandchild and records its PID. The harness opens a
process synchronization handle *before* cancellation, timeout or output-limit
and verifies the grandchild terminates. A successful parent-exit case also
checks descendant cleanup. Fixtures use `CREATE_NO_WINDOW`, no network and
bounded local temporary workspaces.
