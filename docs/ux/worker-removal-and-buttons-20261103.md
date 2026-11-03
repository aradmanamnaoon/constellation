# Worker-removal and actionable controls — 2026-11-03

## User-facing changes

In Rooms, each worker row now has a separately named **Remove unused worker**
button. This opens a confirmation panel before an actual local workspace
removal request. Successful removal is **not** a mass delete: historical
messages, tasks, saved room teams and workflow references are never silently
reassigned or erased. The native application refuses removal when those
records depend on that worker. The UI explains why and leaves the worker and
its history in place.

The room team editor also contains **Remove [worker] from room** buttons.
This is distinct from deleting a worker: it only changes the current room's
routing selection through the existing optimistic-concurrency native API,
without modifying the global roster. If the team has unsaved edits, removal
adjusts the local draft rather than discarding other selections; the user
must explicitly save those edits.

Room-search clear and empty-state navigation use properly named buttons;
worker selections are real accessible toggle buttons rather than text-like
checkbox labels. Provider URL and model ID have explicit copy buttons
instead of relying on selecting small text.

## Security, migration and provenance

- Core worker removal runs in a single SQLite **IMMEDIATE** transaction;
  compares the full expected worker identity (name, role, provider ID,
  creation timestamp), records an AgentRemoved event and supports exact
  operation-ID replay.
- It refuses removal when saved room-team membership, message sender history
  or task ownership references the worker. No database schema migration or
  destructive backfill is needed.
- The native host additionally rejects removal while generation is active or
  any workflow is saved. This is deliberately conservative until workflow
  per-worker attribution can be certified across all workflow states.
- It never sends a model request, runs a tool, accesses ordinary user files
  or deletes credentials. The preferred provider remains independent.
- Room team edits use expected-membership checks; routing preferences are
  **not** authorizations to access AI models or tools.

## Verification and limitations

Rust unit tests cover idempotent replay, stale worker identity, persisted
audit and restart, saved room membership, historical agent messages, task
ownership, and removal after explicit room-member removal. Frontend
contract tests cover both removal actions, actual buttons and native command
wiring. Exact binary hidden GUI regression is required separately to confirm
real interaction and persisted results.

This limited change does **not** establish completion of the prior 12
workstreams, 188 acceptance rows or macOS/Android security/installer gates.
