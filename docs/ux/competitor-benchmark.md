# Competitor UX evidence and Constellation transfer matrix

**Researched 2026-11-01.** The following references are public vendor docs or maintained official documentation, summarized by a read-only UX research worker. These are evidence of documented interaction patterns, **not** a claim of independently using each installed competitor app. No screenshots/assets/code from another product are shipped. Implementation state is tied to this isolated UX branch; `source` means implemented in source and covered by frontend checks, awaiting current native GUI acceptance.

| # | Documented product/pattern and source | Problem it solves / Constellation adaptation | Status |
|---|---|---|---|
| 01 | [Cursor: Ask vs Agent](https://docs.cursor.com/chat/overview) | Distinguish answer-only model requests from permissioned tool actions | Open, explicit backend boundaries already exist |
| 02 | [Cursor: Review diffs](https://docs.cursor.com/chat/overview) | Preview exact proposed changes before accepting native tool action | Open, native approval exists; visual diff missing |
| 03 | [Cursor: Checkpoints](https://docs.cursor.com/en/agent/chat/checkpoints) | Restore scoped work after an interrupted agent run | Open; requires rigorous original recovery design |
| 04 | [VS Code: Quick Open/Palette](https://code.visualstudio.com/docs/editing/getting-started/userinterface) | Find destinations without hunting through a small rail | Source: searchable Ctrl+K, room/page/action navigation |
| 05 | [VS Code: Editor groups](https://code.visualstudio.com/docs/editing/getting-started/userinterface) | Compare conversation, evidence and execution side by side | Open |
| 06 | [VS Code: Zen and preview](https://code.visualstudio.com/docs/editing/getting-started/userinterface) | Reduce distractions while retaining easy exit and work context | Part: existing focus mode; richer preview open |
| 07 | [Raycast: Action panel](https://manual.raycast.com/action-panel) | Contextual action discoverability and keyboard selection | Source: palette arrow/Enter + actions |
| 08 | [Raycast: Aliases/hotkeys](https://manual.raycast.com/command-aliases-and-hotkeys) | Reach frequent destinations directly | Source: Ctrl+1…6/Ctrl+,; customization open |
| 09 | [ChatGPT: Projects](https://help.openai.com/en/articles/10169521-projects-in-chatgpt) | Make room context and project scope understandable | Part: existing room model; explicit scope education open |
| 10 | [ChatGPT: Conversation branching](https://help.openai.com/en/articles/10169521-projects-in-chatgpt) | Explore alternatives without losing source conversation provenance | Open |
| 11 | [ChatGPT: Find chats/projects](https://help.openai.com/en/articles/10056348-finding-your-chats-projects-and-files-in-chatgpt) | Search prior work across sections | Part: room names and destinations only; message full-text open |
| 12 | [Claude: Project knowledge](https://support.anthropic.com/en/articles/9529781-examples-of-projects-you-can-create) | Reuse source context with clearly labeled opt-in inclusion | Open |
| 13 | [Claude: Artifacts](https://support.claude.com/en/articles/14604416-get-started-with-claude-design) | Separate inspectable generated outputs from message text | Open |
| 14 | [Linear: Inbox](https://linear.app/docs/inbox) | Surface approvals and failures needing action | Open |
| 15 | [Linear: Triage](https://linear.app/docs/triage) | Review incoming requests before granting ownership or execution | Existing explicit approval; triage view open |
| 16 | [Linear: My issues](https://linear.app/docs/my-issues) | Group tasks by work state | Source: All/In progress/Completed/Needs attention |
| 17 | [Notion: Views/filters](https://www.notion.com/help/views-filters-and-sorts) | Filter one underlying task data set without duplication | Source: room, instruction/agent keyword, status |
| 18 | [Notion: Side peek](https://www.notion.com/help/views-filters-and-sorts) | Inspect details without losing task list position | Open |
| 19 | [Notion: Saved views](https://www.notion.com/help/views-filters-and-sorts) | Reuse personal complex filters consistently | Open; current filters are session-local |
| 20 | [Slack: Canvas](https://slack.com/help/articles/203950418-Use-a-canvas-in-Slack) | Keep key notes/decisions close to room discussion | Open |
| 21 | [Slack: Activity](https://slack.com/help/articles/115004846068-Slack-updates-and-changes/) | Separate attention items from verbose logs | Open |
| 22 | [Slack: Keyboard shortcuts](https://slack.com/help/articles/201374536-Slack-keyboard-shortcuts-and-commands) | Navigate major regions without mouse | Part: rail shortcuts/palette, region focus checks open |
| 23 | [Discord: Forum tags](https://support.discord.com/hc/en-us/articles/6208479917079-Forum-Channels-FAQ) | Organize topic discussion within long-lived rooms | Open |
| 24 | [Discord: Topic following](https://support.discord.com/hc/en-us/articles/6208479917079-Forum-Channels-FAQ) | Control relevance, attention and participant permission | Open |
| 25 | [n8n: Executions](https://github.com/n8n-io/n8n-docs/blob/main/docs/build/understand-workflows/understand-executions/view-executions-for-a-single-workflow.md) | Find runs by state/time and inspect what happened | Open; existing workflow state UI lacks history filters |
| 26 | [n8n: Debug executions](https://github.com/n8n-io/n8n-docs/blob/main/docs/build/understand-workflows/understand-executions/debug-executions.md) | Diagnose failed jobs from snapshots without hidden retry | Open |
| 27 | [n8n: Pin test data](https://github.com/n8n-io/n8n-docs/blob/main/docs/build/work-with-data/pin-and-mock-data.md) | Rehearse plans offline with clearly synthetic inputs | Open |
| 28 | [LangSmith: Trajectory tracing](https://www.langchain.com/blog/langsmith-trajectories-tracing) | See ordered agent/tool transitions with provenance | Open; native receipts exist but inspector incomplete |
| 29 | [LangSmith: Evaluation](https://www.langchain.com/langsmith/evaluation) | Human annotation for individual results | Open |
| 30 | [LangSmith: Comparison](https://www.langchain.com/langsmith/evaluation) | Compare outputs using actual results and evaluation data | Open |

## Priority rationale

Immediate priority was finding a room or task and preventing confusing cost-bearing model selection. The source improvements implement transferable behaviors from VS Code, Raycast, Linear and Notion, while preserving a distinct Constellation visual identity. Patterns requiring new backend semantics (undo, provider/recovery actions, provenance, search indexes) remain planned until their native contracts can be tested.

**Tradeoffs:** palette search is bounded to at most 30 results and is read-only; it does not index message bodies or make provider requests. Task model selection is intentionally not persisted until a secure native preference contract is designed. Any external service request or destructive restore continues to require deliberate user action and native checks.
