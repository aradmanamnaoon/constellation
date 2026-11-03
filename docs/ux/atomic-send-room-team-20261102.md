# Local message reliability and room-team navigation

**Project:** Constellation desktop

**Date:** 2026-11-02

**Branch:** `ux/competitor-rebuild-20261101` (draft PR #1, not a release)

## Atomic local message and draft clear

Previously, saving a message and clearing its unsent draft were two separate native SQLite operations. Abrupt process termination in between could display already sent text in the composer after restart, inviting duplicate submission.

`StudioStore::append_user_message_and_clear_draft` now commits the local user message, append-only `MessageAppended` event, and empty per-room draft tombstone in one `TransactionBehavior::Immediate` transaction. Preconditions are the exact room, message/operation UUIDs, a nonempty validated message, and the current saved draft revision **and** matching trimmed text. The existing recognized-schema guard is retained. If another window changed the draft, the transaction rejects the send without writing a message, event, or tombstone. An exact same-operation retry reads the audited earlier result and does not write another message. A different operation with stale revision cannot duplicate the prior local send.

The new Tauri `studio_send_saved_draft` command holds the existing message write lock, uses only `MessageSender::User`, and does not execute provider requests or file tools. The renderer flushes the draft first; `DraftPersistence.savedRevision` then gives the exact committed revision. After native confirmation, `confirmSent` adopts the advanced tombstone revision in memory **without issuing a second database write**. A newer edit arriving before the confirmation remains intact and is saved against the advanced revision. Native errors preserve the draft and stable message/operation IDs for an explicit retry. No browser localStorage receives draft content.

This guarantees atomicity within the single **studio SQLite database**, not across other application databases, OS vaults, or remote providers. It is not an AI execution or authorization path.

## Room-team discoverability

The room sidebar now labels the roster as **ROOM TEAM**, displays a selected state for each agent, and gives every agent row a keyboard-focusable action to open the existing room-team editor. **Edit room team** includes the selected count. Opening the editor focuses its summary and scrolls into view; membership mutations still require the existing explicit save and native revision-bound `studio_set_room_members` action. Adding a teammate remains separate from selecting or authorizing a provider. When no room is selected, the action opens room creation instead of silently changing global membership.

Real private Windows GUI inspection caught an empty-room auto-scroll regression: the room introduction could be partly outside the visible viewport because refresh always scrolled the chat container to its bottom. Empty rooms now reset to scroll position zero; only rooms containing conversation messages scroll to the latest message.

## Verification and remaining limitations

- Core test `local_send_clears_draft_in_same_transaction_and_replay_cannot_duplicate` covers invalid expected revision, no partial effects, exact saved text, persisted empty tombstone, cross-restart idempotent replay, stale conflicting sends, and prevention of old-window draft resurrection.
- JavaScript `composer-draft-ux.test.mjs` covers atomic confirmation with no extra draft save, retention of a later unsent edit, original CAS/queue behavior, and no browser draft storage.
- `room-team-sidebar-ux.test.mjs` checks the accessible room-team controls, source routing, explicit editor focus, and native atomic send wiring.
- Windows source validation: 9/9 Core Rust tests, 132/132 desktop Rust unit tests and strict Clippy, 160/160 frontend tests, Svelte 0 errors/0 warnings.
- **Latest native Windows WebView2 acceptance PASS** on exact release-mode binary SHA-256 `CD7DD9C9221C73FCCCDFA80F1246E70A04CF837E8DDD392976042D099CFC682A`, built with `pnpm --dir apps/desktop tauri build --bundles nsis`. Isolated private two-launch test `tests/windows-evidence-backup-e2e.ps1 -TestDrafts -SkipBackup` exited 0, recovered the exact unsent synthetic draft after an actual process restart, saved one local message, observed an empty composer, closed the process and reported `blockers=[]` with zero provider requests. Screenshot: `apps/desktop/src-tauri/target/ux-final-draft-regression-20261102/draft-locally-sent-compact-room-c694fb1c4f2e48089ad9387d4ed467b4.png` (ignored QA evidence).
- **Private Windows task/room-team GUI PASS** on the same exact binary: `tests/windows-installed-uia-smoke.ps1 -TestTaskJourney -TestRoomTeam` exited 0, confirmed task prerequisites and local persistence, checked explicit room membership save and audit, with isolated OS profile, zero provider requests and `process_closed=true`. Screenshot `apps/desktop/src-tauri/target/ux-empty-room-native-final-20261102/installed-window-e02476078a5f4da092b087c45a917aa4.png` was visually inspected: the empty-room heading starts at the top of the transcript instead of being auto-scrolled out of view, and the new Edit room team action appears in the sidebar.
- SQLite still builds full snapshots for large workspaces; true 10,000-message native performance, screen-reader/IME/200% page zoom, Mac/Android native parity, and Windows NSIS upgrade/uninstall require independent evidence.

## Release policy

These changes are an independently useful reliability/navigation slice, not full UX transformation acceptance. Keep PR #1 draft and avoid new tags/releases until all required real platform-specific checks pass.
