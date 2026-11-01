# Agents, manually reviewed files, and tool permissions

**Current implementation status:** Constellation can run explicitly requested
text-only AI tasks and has an experimental, *human-triggered* local file action
panel. It does **not** yet have an autonomous coding agent, installed MCP
servers, unrestricted filesystem access or a general-purpose shell runtime
exposed through the desktop UI.

## Try a safe file action

1. In the installed desktop app, create a room, add an agent and save a task.
   Keep that task in the **Pending** state.
2. Under **Tasks**, expand **Review a local file action** on that task.
3. Choose **Create a new text file** or **Read an existing text file**.
   Enter a path relative to Constellation's private
   `local-file-artifacts` directory. To create a file, enter 1–16,384 UTF-8
   bytes of content. Existing files are never replaced.
4. Press **Preview exact file action**. Review the absolute canonical file
   path and the full proposed content for creates. For reads, the renderer
   receives only an opaque approval ticket; the file SHA-256, byte length and
   internal immutable operation digest remain private until native review.
5. Press **Approve this exact file action** to request an independent
   Windows-native review (or **Reject without executing** to deny immediately).
   The native window displays the task, operation ID, canonical path, content
   SHA-256 and immutable operation digest. For a new file, it also contains
   scrollable read-only original source text and a complete byte-offset hex
   view; **No** is the default. For reads the dialog shows a SHA-256 snapshot
   commitment rather than the content itself. A changed file is rejected
   before its contents can be returned. Check the proposal before choosing
   native **Yes**. The tool executes only after the native proof is verified
   and the saved task is rechecked. Completed results display checksum and status.

The Rust host reads the saved task's real agent, room and task IDs before
preparing an exact-scoped permission. It rejects a nonexistent or already
finished task and checks task state again before execution. A native operation
ID is generated per preview; requests cannot be modified after preparation.
New writes use `CreateNew`, which refuses to overwrite existing files.
Read approvals are bound to a SHA-256 digest and byte length taken from a
bounded UTF-8 snapshot at preparation. A same-path edit before dispatch fails
with an auditable receipt and does not return the new content to the UI.

**No model-generated text can directly call the approval flow.** The UI does
not feed these file results into a model or transmit them to an API.
Tauri exposes only these two bounded manual actions. Process execution,
Git operations, deletion, external paths and implicit project-wide
permissions remain unavailable through the desktop host.

## Security, audit, and honest limitations

The native tool boundary uses
`constellation-tools::LocalExecutor::with_authenticated_journal`, with a
stable 32-byte audit secret stored separately in the OS credential manager.
The OS entry is scoped to a SHA-256 fingerprint of the private journal's
canonical path; isolated test profiles cannot overwrite the active user's
journal key. The key is generated on the first requested local file action,
not at app startup. A missing key for an existing journal, changed key,
malformed HMAC, replayed operation ID or interrupted approval fails closed
instead of silently resetting permissions.

The operation log is created outside the artifact workspace. Its entries
document decisions and attempts without copying file contents or secrets.
An action's returned status is shown in the UI. If IPC fails after approval,
the result may be uncertain: inspect the file and audit log rather than
blindly retrying. Reopening the app cannot reuse the old approval.

This is **not** a hardened security sandbox. Current same-user Windows
processes may access the application's files and, depending on OS policy,
credential storage. An OS-compromised process may race filesystem changes.
The HMAC log detects modified or missing middle records, but without an
independently trusted monotonic checkpoint it cannot always detect a
validly signed *tail rollback*. The native Windows review cannot prevent
automation by a compromised same-user OS process and is not a secure
desktop or independent hardware human-attestation mechanism. The
independent task SQLite event ledger does not yet atomically
reference every file receipt. These limitations block autonomous coding
execution and unrestricted process/network permissions.

## How to test

From the repository root, with a suitable Windows/macOS toolchain:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp\constellation-cargo-target'
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --offline --lib tool_host::tests
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --offline -- -D warnings
```

The optional **real** OS key-store roundtrip test is normally inert; it is
explicitly enabled with `CONSTELLATION_TEST_OS_TOOL_JOURNAL=1`. It creates
one random temporary profile with a distinct credential entry and removes
that entry on exit; it never uses the active project's credential namespace.

The source-pinned SHA-bound Windows candidate passed a real **five-dialog**
unselected-desktop GUI sequence, including an equal-length file mutation
after read preview, which was rejected without content disclosure. See
[`docs/qa/windows-sha-bound-read-private-2026-10-30.md`](qa/windows-sha-bound-read-private-2026-10-30.md)
for its executable checksum and 14 matching journal envelopes. **Later
concurrent source changes have not been requalified as a new Windows
installer**, so the frozen candidate's result applies only to its recorded
source. Linux native consent currently fails closed and has no positive
native approval UI implementation.
Library tests alone do not close that gate.
