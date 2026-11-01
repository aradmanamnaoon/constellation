# Navigation and information architecture — first implemented slice

Constellation is one local workspace containing rooms with messages and configured agent identities. Tasks and workflows may refer to rooms and agents; saved evidence is local; settings govern provider routing and encrypted recovery.

The primary rail remains **Home / Rooms / Tasks / Evidence / Role library / Settings**. The existing Rust data ownership and Tauri IPC boundary are unchanged. A global `Ctrl+K` switcher now indexes available destinations and recent room names; it can begin room, agent and provider setup. Sidebar search filters the current room list without altering underlying room selection.

Task views now support room, keyword and explicit status filters. Every pending task selects its own provider. The selector is kept in temporary UI state and is intentionally blank after restart, so opening another task or changing chat provider cannot silently select a paid endpoint. Native runtime still authorizes every actual model request.

Creating a task without rooms or agents offers local setup actions, preserving the entered instructions while the user creates the missing prerequisite and returns to the plan.

### Next information architecture decisions

1. Determine actual room membership semantics from the Rust domain before treating all globally created agents as room members.
2. Support task/workflow history pagination from durable native APIs, with saved views only after tests prove stable state identity.
3. Develop a scoped room/evidence inspection pane that never quietly adds retrieved content to provider requests.
4. Keep tool, provider, journal and backup approvals on the native trusted boundary and preserve explicit request/restore consequences.

### Component boundaries

- `src/lib/workspace-navigation.ts`: pure, typed room/destination matching; zero IPC and no filesystem, model or network side effects.
- `src/lib/WorkspaceCommandPalette.svelte`: searchable accessible command UI, keyboard selection and user-intent callbacks only.
- `routes/+page.svelte`: existing navigation state and domain command orchestration; to be incrementally split further.
- `routes/studio.css`, `palette.css`: reusable semantic color tokens and responsive layout styles.
