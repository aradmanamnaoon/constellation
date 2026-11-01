# Current source-driven journey inventory

These are implementation traces; **native end-to-end completion is not yet claimed** for the UX branch.

| Journey | Path | Security/recovery behavior | Evidence still needed |
|---|---|---|---|
| Find room | Ctrl+K → type partial name → arrow/Enter → Rooms | Pure local filter; no native call to search | Actual hidden WebView2 keyboard and focus |
| Find section | Ctrl+K → Evidence/Tasks/Settings → Enter | Closes palette and navigates current app state | Native focus return, Escape, screen reader |
| Find many rooms | Rooms → Find a conversation → filtered room links | Does not modify persistent room data | 50+ rooms, zoom, scroll, narrow window |
| First task with no setup | Plan task → Create room → return; Add teammate → return; Save | Creation uses existing Rust commands; request execution still separate | Private native SQLite fixture and restart |
| Locate an older task | Tasks → room → keyword → status → open card | Filtering is local, source task IDs unchanged | Long list, status screen-reader announcements |
| Run a particular task | Task card → explicitly choose provider → review destination → Run | Only the selected task's provider ID passed to native; no implicit network call on model selection | Two tasks/one chat with distinct endpoints; verify zero unintended HTTP |
| Ask a team | Save user message → select agents/model → Ask selected/team | Explicit model dispatch and native permission boundaries preserved | Full native localhost fixture with failure/cancellation |
| Evidence | Save/import source → search → quote/digest → copy | Verbatim local source validation remains in native logic | No new UX acceptance claim; paging open |
| Recovery | Settings → Advanced Studio → inspect/create/restore | Native encrypted backup and separate restore directory | Current source native backup/recovery UIA |

## Known negative states

The palette displays a no-match state. Room/task filters have empty result guidance; task filters can be cleared. Pending tasks have no selected provider by default. Browser preview continues to disable persistent native actions. Invalid or unconfigured per-agent provider selections prevent Run. Native failures still render the existing error paths, and provider calls remain explicitly user-triggered.
