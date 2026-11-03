# Room team persistence and local routing — November 2, 2026

**Scope:** real local team selection for each room, not a security permission grant or model execution.

## Implemented contract

- The room's selected agent IDs are persisted in the existing SQLite `rooms.data` JSON, without schema DDL changes or a migration that could invalidate historic strict SQLite attestation. Legacy `Room` JSON has `member_ids=None`, giving older rooms the existing suggested-first-agent behavior. `Some([])` preserves an explicit user selection of no teammates instead of silently reintroducing the default.
- `StudioStore::set_room_members` validates a maximum of eight unique existing agent IDs. It commits the updated room together with a `RoomMembersChanged` audit event in a single SQLite transaction. The caller must supply the previously observed member IDs; mismatches reject stale writes with `OperationConflict`, with no change to the record or audit history. Reusing the exact operation ID is idempotent.
- Native `studio_set_room_members` parses typed UUIDs and exposes only the room selection. `studio_bootstrap` includes `member_ids` and `members_configured`; no credential, file operation, model provider request, or consent boundary changes.
- Rooms UI allows changing checked agents, reviewing an unsaved state, explicitly saving it, or discarding local changes. The room reopens with the persisted team selection, and the current model is still separately reselected on room switches. The copy distinguishes a local routing preference from actual authorization.

## Acceptance and limitations

Core contract test `room_team_members_are_durable_audited_idempotent_and_legacy_compatible` passes for restart, explicit empty selection, duplicate/unknown agent rejection, exact retry, stale writes and event recording. The broader desktop Rust library suite and source-level JS/Svelte checks also pass. Native Windows UIA acceptance is recorded separately in `qa-results-2026-11-02.md` after it executes on the latest source-pinned binary.

Membership does **not** implement human invitation, authentication, workspace-level access control or collaboration over a network. Team members are selected AI agents for the room. This feature is not proof of platform-native Android/macOS parity, and it is not evidence of UI keyboard/assistive-technology acceptance until those tests run.
