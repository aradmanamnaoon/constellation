# UX continuation — task, workflow and evidence history (2026-11-02)

The previously published GitHub release preview is **not** the current UX build. A partially uploaded replacement release draft was cancelled and deleted before publication. Do not tag or publish these changes until the broader UX requirements and platform release gates are addressed. The existing, accurately titled desktop preview remains unchanged.

## Changes made in this UX slice

### Saved workflows: all-page discovery

The earlier UI exposed the first 100 local workflows and then merely warned that older records were missing. The trusted native command now loads up to **25 workflows per page**, fetches one additional record to correctly identify whether another page exists, returns the validated offset, query and maximum page size, and refuses invalid offsets. The underlying SQLite repository still enumerates rows in deterministic workflow-ID order. The UI has previous/next controls and a search field that applies a bound, literal case-insensitive ID substring query against the entire local repository, not just the current page. `%` and `_` are ordinary text, not SQL wildcard operators. Results are read-only; workflow execution and every paid provider request remain separate actions with their existing permissions. On explicit Save, the new workflow ID is selected in history so the user can inspect the persisted record immediately.

The database integration test saves **51** synthetic workflow definitions, verifies three disjoint ordered pages (25/25/1), empty pages, exact has-more flags, invalid page offsets, case-insensitive ID filtering and literal wildcard behavior. No provider network request is sent by this test.

### Tasks: bounded rendering without altering authorization

The local task list retains its existing room, keyword and status filters, but renders only **40 task cards per page**. The pure `paginateLocal` utility clamps stale/invalid page selections without reordering the underlying task collection. Filtering and home-task navigation return the user to the first matching page, and the page indicator discloses both visible and matching counts. This is **renderer pagination**: the trusted Core snapshot still contains all tasks. It does not claim database-level paging or full virtualization for 10,000-task workspaces. Every per-task model selector and separate native execution consent remains unchanged.

### Saved Evidence: bounded, source-verified ranked pages

Previously the saved library returned only ten top-ranked quotations regardless of additional matches. The native command now pages **10 ranked hits** at a time, with the same deterministic BM25 sort, a one-record lookahead and an explicit ceiling of **1,000 ranked hits per search**. Each page returns validated offset/page metadata. In the renderer, every result still requires fetching its original saved source and independently verifying the exact UTF-8 byte range, quote bytes, source identity/path and SHA-256 before display or clipboard use. The current unsaved-text search remains separate. Paging is offline, performs no model calls and grants no new document access permissions.

An isolated SQLite integration test persists **31** synthetic local text sources, checks pages 10/10/10/1 without duplicates, verifies empty and invalid offsets and checks `model_called=false`. This does not prove factual truth of any quotation or retrieve matches beyond the bounded top 1,000.

## Observed tests and scope

- `node --test --test-concurrency=1 tests/*.test.mjs`: **147/147 PASS** on Windows.
- Desktop `cargo test --lib --locked`: **132/132 PASS**, including the 51-workflow and 31-source database pagination regressions. Rust `cargo clippy --all-targets --locked -- -D warnings`: **PASS**; Rust format checks **PASS**.
- `pnpm --dir apps/desktop check`: **0 errors, 0 warnings**; production Vite build **PASS**.
- **Latest current-source Windows release-mode build** (including saved Evidence pagination) `pnpm --dir apps/desktop tauri build --bundles nsis`: **PASS**. The application executable `apps/desktop/src-tauri/target/release/constellation-desktop.exe` is **SHA-256 `F0C50EA2342BDD75908F40FF6DE15083EAF9AEBCB09E73FA651EECF45EB49841`**.
- **Latest native Windows WebView2 GUI acceptance**: `tests/windows-evidence-backup-e2e.ps1 -TestWorkflows -SkipBackup` with that exact binary hash and a private, isolated synthetic profile returned **exit 0**, run `5fe2b4fa615144a78858a0717417af83`, `blockers=[]`, `process_closed=true`, `workflow_fixture_request_count=0`, with real Evidence / workflow-saved / workflow-cancelled screenshots. This run included native UIA positive workflow-ID search, negative no-match search and clear-filter interaction. The Evidence screen was physically rendered, but **31-source Evidence pagination was verified in a real SQLite backend test rather than through 31-source WebView GUI paging**; do not conflate those levels.

### Still outstanding UX acceptance

Unsent chat drafts are not durably preserved on app restart; large Core snapshots and 10,000-item stress/virtualization remain untested; full native physical keyboard shortcuts, screen-reader announcement/focus, real browser 200% zoom/reflow, high-DPI, RTL/IME, provider credentialed revocation, installer update lifecycle, and native Android/macOS work remain open. These are not erased by green frontend/SQLite tests or a Windows screenshot. See `complete-execution-plan.md` and `local-ci-replacement-20261102.md`.
