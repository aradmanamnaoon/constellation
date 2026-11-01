# Offline model proposal boundary

`constellation-agent-loop` is a deterministic **Rust library slice**, not a
desktop coding-agent feature or a native consent implementation. Models can
propose only a bounded `read_file`, `create_file` or final message. The library
turns a proposal into a prepared `constellation-tools` operation, then stops
until an independent, trusted application host obtains user consent, verifies
current task authorization, and supplies the exact `OperationApproval`.

The model response must be **one JSON object**. For example:

```json
{"type":"tool","action":"read_file","path":"docs/notes.md"}
```

```json
{"type":"tool","action":"create_file","path":"draft.txt","content":"Review me before writing."}
```

```json
{"type":"final","text":"The requested draft is complete."}
```

The parser rejects extraneous fields (including model-provided `approved`,
`permission_granted`, IDs or tool receipts), duplicate top-level JSON keys,
unknown tool types, malformed/nontext content, control characters in paths,
Unicode lookalike paths (model-generated path labels are restricted to printable
ASCII), oversized responses and excessive tool iterations. Invalid JSON and
failed operation preparations both count toward 16 model attempts per turn;
at most eight successful tool proposals are permitted. The coordinator never
calls `PermissionGate::grant` or `LocalExecutor::approve` and exposes no
endpoint, browser, process, Git or unrestricted filesystem execution.

Calls to `AgentTurn::submit_model` prepare an immutable operation and return a
`TurnEvent::ApprovalRequired` preview with a unique operation ID. A trusted
native host must display exactly that preview and use its own consent
channel. For a proposed file create, the original immutable model content is
available to the trusted host from `pending_create_content_for_native_review`;
the test checks its exact byte length and SHA-256 against the committed
executor preview. This accessor yields no content for file reads. A read
proposal is deliberately **not prepared** in `submit_model`: the model sees an
identical metadata-free pending preview for an existing file, an absent file
and a file outside the workspace. After the user independently consents to
inspect the named read target, trusted native code must call
`prepare_pending_read_for_native_review`, inspect the canonical path, size,
SHA and executor digest in the prepared operation, and obtain final consent
before approving and dispatching. Failed private preparation does not return
file existence or size to the model. Denying an unprepared read abandons the
opaque operation ID without accessing the file; the executor cannot journal
that denial until an operation is prepared, so the host should separately
audit the initial read-decision event. Denials of prepared operations use
`AgentTurn::deny`, which persists a journal rejection
when the executor is configured with its authenticated journal. On approval,
the host grants the exact scope and obtains a single-use approval from the
executor; `dispatch_approved` checks it again. A post-approval cancellation
uses `cancel_approved` for a durable cancelled attempt receipt; ordinary
`cancel` returns `ApprovedCancellationRequired` without discarding the pending
operation. A changed
read is failed by the existing SHA-bound tool executor with no file content
returned. The crate exposes host-owned checkpoints but does not persist them.

## Trusted host multi-turn checkpoints

`AgentTurn::checkpoint_for_host()` exports a bounded, versioned, principal-bound
`HostTurnCheckpoint` (JSON/serde). It contains the consumed model attempts,
tool proposal count, turn state and monotonic revision, but no tool operation,
permission token or read content. A checkpoint is available only after
pending proposals are denied or dispatched, or after the turn is terminal.
`AgentTurn::restore_from_host(context, checkpoint, minimum_revision)` rejects
mismatched principals, invalid counters, pending-approval state and older
revisions. The application host must authenticate checkpoint bytes and store
the minimum revision in independently trusted durable state. These methods
cannot themselves verify HMAC, a task's durable state, or executor journal
replay; arbitrary JSON supplied by a model or renderer is never a checkpoint.

The settled checkpoint now also enforces **exact revision chronology**:
each consumed model attempt advances revision by one and each resolved tool
proposal (separately denied or dispatched) adds one; cancellation or terminal
budget exhaustion may add at most one extra revision. A fabricated settled
revision that skips denial/dispatch or inserts a phantom transition fails
closed even with a lower independently supplied revision floor. This checks
structural reachability, **not** whether a completion claim actually occurred:
the trusted host must authenticate terminal events and anchor the checkpoint
against rollback outside its writable store.

`submit_model_at_revision` fences late/duplicate asynchronous provider replies.
`ModelStepSource` and `advance_from_source` support an independently managed
deterministic model provider; provider errors also consume the bounded model
attempt budget. The model only proposes read/create/final JSON, never native
permission grants, approvals, Process/Git or executable tool calls.

For an in-flight blocking provider call the host can instead use
`advance_from_source_with_cancellation(revision, provider, executor, &token)`.
Cancellation before the provider call skips it. Cancellation while a provider
runs consumes one model attempt, rejects its response without creating a tool
approval, and makes the turn terminal (`LoopError::Cancelled`). A second check
following processing protects against a signal arriving during in-process
proposal handling. The host must durably anchor this terminal checkpoint and
never retry it automatically. The API does **not** forcibly stop a blocked
network transport or guarantee race-free external provider billing.

To integrate: native Rust must persist the authenticated checkpoint and a
monotonic floor with the authoritative task revision, reserve provider spend
before invoking `ModelStepSource`, and pass the expected revision with each
provider call. On `ApprovalRequired`, the host must journal pending operation
identity, obtain separate native human consent, use executor denial or
single-use approved dispatch, record its receipt, and only then checkpoint the
settled turn. A crash with unresolved approval must block resumption until the
executor journal is reconciled; the host must never automatically regrant a
model proposal or replay an approval. Tool observations given to the model
must be bounded and independently sanitized by the host.

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp\constellation-agent-loop-tests'
cargo fmt --manifest-path crates/constellation-agent-loop/Cargo.toml --all --check
cargo test --manifest-path crates/constellation-agent-loop/Cargo.toml --offline --locked --all-targets
cargo clippy --manifest-path crates/constellation-agent-loop/Cargo.toml --offline --locked --all-targets -- -D warnings
```

Ten proposal integration tests verify input forgery/duplicates, exact approval
requirements, denied write and authenticated journal reopening, changed-file
read rejection, pre-consent metadata non-disclosure, malformed attempts
exhaustion, one-shot use, budget/cancellation, cancellation after approval,
revocation/regrant replay, and traversal rejection. The separate offline
mock-model integration fixture runs the same real approved child-process test
before and after a separately approved CreateNew source fix, with audited
denial, process and write receipts. The Windows child invokes `rustc --test`
for syntax/type-check metadata; Unix also links and runs the generated Rust
unit test. Windows generated-code runtime execution remains unverified.
The twelfth-wave fixture separately runs a real approved Rust test child
(missing source → failure), receives a deterministic model-proposed fix
requiring **independent CreateNew consent**, then runs the identical approved
test child again (success), checks replay refusal, and restores a settled
checkpoint. Tests also cover impossible revision history, prompt-injected
approval fields, stale replies, and cancellation from another thread while
the model provider is blocked.
They do not constitute real model tool calling, native GUI consent
or secure OS sandbox testing. The remaining work is to wire this coordinator
through Rust-owned desktop task state and actual provider model-turn APIs,
provide native prompts and durable turn persistence, and run an end-to-end
fail→approved fix→test fixture on a private installed desktop.
