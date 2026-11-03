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

## Private native Windows acceptance (2026-11-03)

- **Source:** `47e10b352390af6554634506cfe38434e34d0fcb`, clean detached
  verification checkout. Production `tauri build --no-bundle` completed with
  offline Cargo and one Rust build job; this is a built executable, **not** an
  installed/upgrade-tested NSIS package.
- **Executable SHA-256:**
  `22306c7f8dad0bfaf878233d6355ae8ce0c35991f297d6ebf091288d87e7a833`.
- **Harness:** `tests/windows-installed-uia-smoke.ps1 -TestTaskJourney
  -TestRoomTeam`, with the exact executable hash, a separate non-input Win32
  desktop, fresh isolated native SQLite/TEMP/WebView2 directories, and no
  provider calls, real user data, physical keyboard or physical mouse input.
- **Passing run:** `94629dc41a314968a4c69062f661ff0f`; UIA found 62 real
  native WebView descendants and the six main destinations. The application
  started and closed successfully with the isolated SQLite database.
- **Observed and persisted:** Task draft return, task save and filtering;
  explicit empty room team and its audit record; denial of worker removal
  when its saved task still references it; creation and subsequent removal of
  a second genuinely unused worker, with the first worker and saved task
  retained and exactly one `agent_removed` event in SQLite.
- **Screenshot:** local ignored evidence
  `target/native-worker-removal-uia-final-20261103/installed-window-94629dc41a314968a4c69062f661ff0f.png`,
  SHA-256 `44899e1913ad7fb3e12f924972903feedf7e4abc00a758dfcbcc9779cdcfc1db`.
  Visually inspected: the success notice is visible and only the referenced
  worker remains in the sidebar.

Two earlier, explicitly failed private-desktop probes revealed **test harness**
assumptions rather than data-integrity failures. WebView2 did not expose an
`InvokePattern` or `TogglePattern` for one `aria-expanded` roster action, so
the harness now accepts genuine UIA `ExpandCollapsePattern` and
`LegacyIAccessiblePattern` default actions. It also locates the added worker
by its unique accessible action rather than expecting a bare child text node.
The isolated failed-run database confirmed that the second worker had actually
been saved. No live input-desktop fallback was attempted.

The passing result demonstrates the scoped Windows **executable** interaction
only. It does not validate installed NSIS lifecycle, complete keyboard/screen-
reader usability, Linux installed behavior, macOS, Android, hosted CI, signing,
or a final multi-platform release.
