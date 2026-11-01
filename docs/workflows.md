# Workflow plans — current implementation and trust boundary

Constellation stores user-authored, multi-step plans locally. A plan can
represent researcher → writer → reviewer, or parallel independent work whose
results are reviewed together. The workflow subsystem is implemented in Rust
at `crates/constellation-workflows` and backed by a separate SQLite database
(`constellation-workflows.sqlite3`) using WAL and FULL synchronous mode.

This is a **bounded, explicitly invoked text workflow runner**, not a coding
agent or autonomous OS tool runner. Creating, previewing, listing and cancelling
a plan do not contact models. In Tasks → Workflows, a user may press **Run text
step** for a ready step or **Run remaining text steps** for at most three
dependency-ordered steps. The latter may send up to three separately charged
provider requests. Nothing is scheduled automatically, and no filesystem,
shell, Git or remote runner privileges are granted to model output.

## Build a bounded plan

Version 1 stores up to 64 steps. Every step must have a distinct ID, an
existing agent with the exact saved role, an explicitly configured provider,
text instructions, dependency IDs and 1–10 maximum attempts. The Rust engine
checks missing dependencies, cycles, duplicate steps, invalid/oversized input,
and cross-window revision conflicts before accepting a plan.

For illustration, the API structure is:

```json
{
  "schema_version": 1,
  "workflow_id": "research-writing-review-01",
  "steps": [
    {
      "id": "research",
      "role": "Researcher",
      "agent_id": "<existing-agent-uuid>",
      "provider_id": "<configured-provider-uuid>",
      "instruction": "Find evidence and identify uncertainties.",
      "depends_on": [],
      "max_attempts": 2
    },
    {
      "id": "write",
      "role": "Writer",
      "agent_id": "<existing-writer-uuid>",
      "provider_id": "<configured-provider-uuid>",
      "instruction": "Draft with explicit supporting evidence.",
      "depends_on": ["research"],
      "max_attempts": 2
    },
    {
      "id": "review",
      "role": "Reviewer",
      "agent_id": "<existing-reviewer-uuid>",
      "provider_id": "<configured-provider-uuid>",
      "instruction": "Check accuracy and unsupported claims.",
      "depends_on": ["write"],
      "max_attempts": 2
    }
  ]
}
```

The strings in angle brackets are placeholders in this example, not usable
IDs. The actual UI must bind its configured agents and providers.

## Host commands and guarantees

The Tauri backend provides `studio_preview_workflow`,
`studio_create_workflow`, `studio_list_workflows`,
`studio_cancel_workflow`, `studio_run_workflow_step`,
`studio_run_workflow` and `studio_retry_workflow_step` (mutations require
the expected revision).
Previewing is inert; saving starts at revision 1; cancelling changes a saved
revision and prevents pending steps from running. The native UI surfaces a
bounded page of up to 100 saved workflows; this is not full-history paging.

The repository maintains exact-attempt completion, revision compare-and-swap,
validates serialized data and refuses to redispatch unfinished operations
restored after a process crash. SQLite checksums detect accidental payload
corruption but are not cryptographic authentication against an attacker able
to rewrite the database and checksum together.

For an explicitly run step, the trusted Rust host validates the saved role
and provider, prepares a bounded text-only prompt, and atomically saves a
`Running` attempt **before** polling the external provider. A confirmed
successful text response (maximum 16 KiB) and the terminal state are stored
in the **same SQLite revision**. Failed calls record the failed attempt;
retry requires a separate user action. Results from prerequisite steps are
passed to downstream steps with explicit untrusted-content labels, within
the 64 KiB request input cap. Stale revision conflicts reject late success
after cancellation, and restored `Running` steps become `Blocked` so a
crash cannot silently replay a billable request. Actual tool execution,
unattended retry, streaming, provider billing verification and multi-platform
installed-app tests are still open.

The existing encrypted backup UI currently protects the **main workspace
database**. The separate workflow database must be exported and restored as a
consistent part of a future comprehensive workspace backup; do not interpret
the current backup control as an archive of all projects and workflow data.

## Verification

On a host with Rust installed, run:

```sh
cargo test --manifest-path crates/constellation-workflows/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-workflows/Cargo.toml --all-targets --offline -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib --offline
```

Windows systems with little free space on the project drive can direct Cargo
build output to a separate drive through `CARGO_TARGET_DIR`. Cross-platform
hosted CI, live workflow orchestration, safe coding-agent execution, and
independent installed GUI verification are separate acceptance requirements.
