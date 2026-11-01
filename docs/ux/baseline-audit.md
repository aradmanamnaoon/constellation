# Constellation UX baseline — 2026-11-01

**Source audited:** `d11ca883a751037954656aabc0371b24c918445c` on `fix/windows-startup-legacy-schema-20261101`. This is a source and existing-evidence audit, not a new installed/native user study. An older real native capture at `docs/qa/screenshots/windows-team-qa-current.png` illustrates a large composer relative to available transcript space, but is **not** a current-SHA screenshot.

**Audit method:** review of `apps/desktop/src/routes/+page.svelte`, `studio.css`, source-driven task flows, historical QA documentation and existing executable test scripts. Heuristic severities are expert judgments. There are no invented users, survey results or task completion measurements.

| Priority | Baseline defect (source lines at initial SHA) | User impact | Work in this UX branch |
|---|---|---|---|
| P1 | Static six-entry command palette (`+page.svelte:2655`) | Cannot quickly find rooms or actions | Searchable keyboard palette and room destinations; native verification open |
| P1 | Room sidebar list has no filter and shares a fixed-height layout (`studio.css:20,32-56`) | Old rooms become difficult to reach; long lists clip at zoom | Room filter, bounded independent scrolling; 200% native reflow pending |
| P1 | Chat and task execution share `selectedProviderId` (`+page.svelte:1894-1897,2096-2113`) | Changing one model can silently affect unrelated pending task requests | Each task now has its own explicit model selector and destination label |
| P1 | Task list only filters by room (`+page.svelte:266,2126-2296`) | Failed or old tasks hard to find | Keyword and state filters added; still no virtualization |
| P1 | New task form disables Save if prerequisites are missing without direct setup controls (`+page.svelte:2632`) | First-time task planning gets stuck | Resume task draft after creating missing room/agent |
| P1 | Draft messages remain component-only (`+page.svelte:107-115,378-387`) | Restart can lose unsent work | Open: needs secure, deliberate native persistence design |
| P1 | Sidebar team rows are inert and global even though room has `member_ids` (`+page.svelte:15,1964-1980`) | Poor model of selected vs available participants | Open: native membership contract and clear room roster needed |
| P1 | Saved workflows show only first 100 without paging (`+page.svelte:1559-1572,2429`) | Older history may be undiscoverable | Open: backend-supported paging/search |
| P1 | Evidence result caps and limited source context (`+page.svelte:825-829,899-907,2520-2525`) | Users can miss matches | Open: bounded native result paging |
| P1 | Provider settings offer add/test but limited lifecycle management (`+page.svelte:2543-2556`) | Hard to rotate/revoke/repair a provider | Open: Rust-command-backed edit/delete lifecycle |
| P2 | Transcript is plaintext with limited message actions (`+page.svelte:2021-2023`) | Long code/messages hard to inspect and copy | Open: safe code block/copy UI without unsafe HTML |
| P2 | Chat has no robust scroll anchor or unread jump (`+page.svelte:2012-2023`) | Older-message review loses location | Open: preserve visual reading position |
| P2 | 200-role list displays large grid with basic text search (`+page.svelte:2530`) | High cognitive load | Open: categories and progressive detail |
| P2 | Large chat/workflow lists remain mounted or loaded without end-to-end performance proof | Long-lived workspaces may lag | Open: synthetic 10k-event profile and windowing plan |

## Existing screenshots and test evidence

- `docs/qa/screenshots/windows-team-qa-current.png`: real earlier Tauri/WebView2 group-room screenshot, not the UX branch build. Visible vertically dense composer and prominent introductory room block.
- Existing `tests/windows-installed-uia-smoke.ps1`, `tests/helper-hidden-desktop.ps1`, and `tests/windows-evidence-backup-e2e.ps1` can support private UIA sessions; they pin source/binary identities and require new accepted evidence for the UX branch.
- `docs/quality/acceptance-ledger.md` still records release-wide UX acceptance as unverified. This branch does not retroactively upgrade those results.

## Validation rules

Measure task completion, clicks, keyboard traversal, horizontal overflow and contrast on both the base and rebuilt **native** executable at matched viewports. Mark unavailable dimensions `NOT RUN` rather than assigning scores. Synthetic fixtures must not contact paid providers or private profiles. Native consent must remain solely in Rust/OS review.
