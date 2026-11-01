# Constellation workflows

A deterministic, bounded DAG planner with an optional durable SQLite workflow
repository. This standalone crate does **not** execute processes, contact
models or make tool grants. The repository persists JSON snapshots locally.

## Contract

`WorkflowDefinition` contains a versioned `workflow_id` and at most 64
`StepDefinition` entries. Each entry explicitly identifies its role, agent,
provider, instruction, dependencies and retry budget (1–10 attempts). Invalid
identifiers, unknown/duplicate dependencies, dependency cycles and oversized
inputs are rejected. `topological_order()` and `ready_steps()` are sorted
deterministically by step ID.

`WorkflowRun::new(definition)` creates the state machine. A trusted host calls
`ready_steps()`, then `start(step_id)` to obtain a `StepDispatch` proposal.
The host separately validates permissions and provider configuration and must
persist the run before attempting any physical effects. Alternatively, use the
repository's atomic `claim_ready` / `claim_next_ready` API to obtain a proposal
only after its Running state has committed. On receiving a confirmed
outcome, it calls `complete(step_id, attempt, outcome)`. Only successful
dependencies release a downstream step. Duplicate completions are idempotent,
conflicting/stale attempts are rejected, and retries require an explicit
`retry(step_id)` transition. `cancel_step`, `skip`, and `cancel` stop scheduling
work and propagate cancellation or skipping.

## Crash recovery and replay prevention

`WorkflowRun::to_json()` emits validated state. Both
`WorkflowRun::from_json_bounded(bytes)` and direct Serde deserialization
validate the persisted graph and **convert every persisted `Running` step
into `Blocked`**. That transition preserves the exact attempt counter and
previous completion history. A blocked step cannot be restarted, retried or
completed, including using the prior attempt ID. Dependent work remains
unready; unrelated ready steps can continue.

Blocked means that the previous effect's outcome is unknown. Trusted host code
must obtain an explicit user decision before calling `skip(step_id)`,
`cancel_step(step_id)`, or `cancel()` to abandon it. To run the work again,
the host must create a new, independently approved workflow; recovering this
run will never redispatch an interrupted attempt. These transitions do not
terminate already running OS processes: cancellation is a scheduling signal.

The host must save state durably before dispatch and record effects with
external idempotency where possible. JSON snapshots are not encrypted, signed,
or protected against rollback or malicious editing. The host must protect the
database and any separate artifacts before using them for privileged actions.

## Durable SQLite repository (schema 1)

`Repository::open(path)` and `Repository::new(path)` open the same file-backed,
versioned database with SQLite WAL journaling and FULL synchronization.
The caller supplies an absolute local database path. `rusqlite` uses the
bundled cached SQLite build. No file access occurs inside the DAG planner,
and the repository makes no provider requests or tool grants.

The simple host API comprises:

- `create(&run) -> StoredWorkflow`: inserts a validated run at revision 1;
  duplicate IDs are rejected.
- `get(workflow_id) -> Option<StoredWorkflow>`: validates the persisted JSON,
  schema version, row identity and checksum. A persisted running attempt
  returns as blocked, retaining its attempt ID.
- `list(limit, offset) -> Vec<StoredWorkflow>`: sorted by workflow ID;
  limits are between 1 and 100 records per page and offset starts at 0.
- `update(expected_revision, &run) -> StoredWorkflow`: validates the new
  snapshot and atomically compares its stored revision before updating.
  Stale revisions fail with `RepositoryError::RevisionConflict`.
- `claim_ready(workflow_id, step_id, expected_revision) -> ClaimedStep`:
  validates the current DAG readiness inside an IMMEDIATE transaction, commits
  the attempt increment and Running state before returning its dispatch. A
  stale revision or blocked, cancelled or unready step does not dispatch.
- `claim_next_ready(workflow_id, expected_revision) -> Option<ClaimedStep>`:
  atomically claims the lexicographically smallest ready step. Returns `None`
  without writing if the current graph has no ready work.
- `complete(&stored, step_id, attempt, outcome) -> StoredWorkflow`: performs
  exact-attempt completion in an IMMEDIATE transaction. Independent step
  completions can advance concurrently: the repository merges a callback onto
  a newer revision only when the targeted attempt is still the same persisted
  in-flight attempt. Duplicate callbacks return the committed revision;
  conflicting outcomes, cancellation and stale attempts are rejected.
- `complete_with_result(&stored, step_id, attempt, outcome, result_text)`:
  commits bounded successful text artifacts and their outcomes atomically, even
  when a different branch has advanced. Exact duplicate payloads are idempotent;
  conflicting result text and artifacts attached to failures are refused.

`StoredWorkflow { run: WorkflowRun, revision: u64 }` holds a validated
snapshot. The trusted host must **retain the live snapshot** returned when it
persists a new running attempt. A snapshot retrieved after a process restart
contains a `Blocked` step: it cannot complete or redispatch the uncertain
operation automatically. The host should obtain an explicit user decision
about the blocked attempt. `ClaimedStep` holds the dispatch and live snapshot;
retain it in process memory until the result is received. Reading the database
in a new call conservatively presents *all* persisted in-flight attempts as
Blocked, even if the original host still has an active request. The completion
API uses the retained live snapshot to reconcile an independently committed
branch; a restored Blocked snapshot cannot authorize an old completion.

```rust
use constellation_workflows::{CompletionOutcome, Repository, WorkflowRun};
# fn example(path: &std::path::Path, run: WorkflowRun) -> Result<(), Box<dyn std::error::Error>> {
let repo = Repository::open(path)?;
let saved = repo.create(&run)?;
let mut plan = saved.run.clone();
let proposal = plan.start("research")?;
let live = repo.update(saved.revision, &plan)?;
// Trusted host separately dispatches the explicitly configured provider.
let done = repo.complete(&live, &proposal.step_id, proposal.attempt, CompletionOutcome::Succeeded)?;
assert_eq!(repo.complete(&live, &proposal.step_id, proposal.attempt, CompletionOutcome::Succeeded)?.revision, done.revision);
let page = repo.list(100, 0)?;
assert!(!page.is_empty());
# Ok(())
# }
```

Each stored JSON payload is limited to 512 KiB. Every read validates its
schema, byte length, workflow ID, state and SHA-256 payload checksum. SQL
write operations use transactions; failures preserve the prior state.
Checksums detect accidental modifications and simple tampering, but are **not
authentication**: a process that can edit both the payload and checksum can
forge a valid record. The host must use protected storage and a trusted MAC or
external checkpoint if it needs resistance to malicious database rewrites or
rollback.

## Verification

```sh
cargo fmt --manifest-path crates/constellation-workflows/Cargo.toml --all -- --check
cargo test --manifest-path crates/constellation-workflows/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-workflows/Cargo.toml --all-targets --offline -- -D warnings
```
