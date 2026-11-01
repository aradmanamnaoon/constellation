#!/usr/bin/env node
// Regenerate exhaustive, conservative task traceability from the repository's
// master specification and inventories. This does not mark tasks complete.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('../../', import.meta.url)));
const read = (path) => readFileSync(resolve(root, path), 'utf8');

// Public, reviewed-source snapshot distilled ONLY from the already committed
// COMPLETE_EXECUTION_PLAN.md summary at Git revision ef7018e. These are
// requirement titles, public task IDs, prior section labels and provenance
// line numbers, not the withheld original master prompt or private board.
// This embedded input makes regeneration independent from private files.
const PUBLIC_SPEC_ROWS = [
 [
  "SPEC-28-001",
  "Inspect environment, OS, available Rust/Node/Android SDK, installed apps, GitHub auth, signing credentials and any existing project. Never delete unrelated data or overwrite an existing repository.",
  "Phase 0 — workspace audit, research and dependencies",
  1059
 ],
 [
  "SPEC-28-002",
  "Source-linked architecture/UX/license/security research, framework comparison, quick runnable spikes, choose framework and document decision.",
  "Phase 0 — workspace audit, research and dependencies",
  1060
 ],
 [
  "SPEC-28-003",
  "Choose placeholder codename, check final naming candidates; set repo structure, task ledger, issue tracker and first CI gates.",
  "Phase 0 — workspace audit, research and dependencies",
  1061
 ],
 [
  "SPEC-28-004",
  "Source and audit 200 development skills/MCP integrations under §14; start the inventory early, do not postpone all 200 to final week.",
  "Phase 0 — workspace audit, research and dependencies",
  1062
 ],
 [
  "SPEC-28-005",
  "**Gate:** reproducible Rust workspace; real empty-state app opens; UI direction validated; permission architecture agreed; no unlicensed brand assets.",
  "Phase 0 — workspace audit, research and dependencies",
  1063
 ],
 [
  "SPEC-28-006",
  "Original brand/icon/token system, onboarding, local accountless workspace, navigation, responsive desktop/mobile shells, SQLite/migrations.",
  "Phase 1 — exceptional first-run app and chat foundation",
  1067
 ],
 [
  "SPEC-28-007",
  "Room/DM, individual messages, threads, message persistence, attachments, text selection, export, real empty/error states, searchable role library scaffold.",
  "Phase 1 — exceptional first-run app and chat foundation",
  1068
 ],
 [
  "SPEC-28-008",
  "Real Tauri command boundary and test fixtures; no faked backend.",
  "Phase 1 — exceptional first-run app and chat foundation",
  1069
 ],
 [
  "SPEC-28-009",
  "**Gate:** can install and launch a coherent branded app, create a room, message a deterministic demo, restart and see persisted content; literal GUI test at multiple sizes.",
  "Phase 1 — exceptional first-run app and chat foundation",
  1070
 ],
 [
  "SPEC-28-010",
  "Provider adapters, new provider wizard, local model support, streaming, capability probe, model routing, budget counters.",
  "Phase 2 — real providers, roles and single-agent tools",
  1074
 ],
 [
  "SPEC-28-011",
  "Implement functional default roles with real local tools; progressively populate/test the complete 200-role catalogue.",
  "Phase 2 — real providers, roles and single-agent tools",
  1075
 ],
 [
  "SPEC-28-012",
  "Permission engine, filesystem-scoped tools, structured process runner, audit, code diff/review/approval, cancellation and restart.",
  "Phase 2 — real providers, roles and single-agent tools",
  1076
 ],
 [
  "SPEC-28-013",
  "**Gate:** user can add a real supported provider and run a safe filesystem/code task end to end with receipts; denied action stays denied; installed UI tested.",
  "Phase 2 — real providers, roles and single-agent tools",
  1077
 ],
 [
  "SPEC-28-014",
  "Group chat, `@mentions`, role-to-agent mapping, delegation/parallelism, plan visibility, context boundaries, cross-agent reviews, workload and spend limits.",
  "Phase 3 — true multi-agent collaboration and workflow engine",
  1081
 ],
 [
  "SPEC-28-015",
  "Durable DAG orchestration, schedules (local while app/runner active), reusable templates, artifact versions, review/approve and conflict-safe state.",
  "Phase 3 — true multi-agent collaboration and workflow engine",
  1082
 ],
 [
  "SPEC-28-016",
  "**Gate:** test real group task researcher → writer → reviewer and independent parallel coders with change isolation; complete output backed by logs.",
  "Phase 3 — true multi-agent collaboration and workflow engine",
  1083
 ],
 [
  "SPEC-28-017",
  "Full authorized coding toolbox, browser automation where available, repository index, search, diff/test/build/Git workflows, MCP stdio/HTTP, ACP bridge if tested, skill lifecycle, document retrieval with evidence.",
  "Phase 4 — expert coding, knowledge and integrations",
  1087
 ],
 [
  "SPEC-28-018",
  "Integrate advanced settings without cluttering Rooms. User can inspect and revoke all broad privileges.",
  "Phase 4 — expert coding, knowledge and integrations",
  1088
 ],
 [
  "SPEC-28-019",
  "**Gate:** a coding agent modifies a real fixture repository, runs failing then passing tests, offers diff, and recovers from denial/interruption.",
  "Phase 4 — expert coding, knowledge and integrations",
  1089
 ],
 [
  "SPEC-28-020",
  "E2EE local backups/restore, user-owned S3/R2 capability check/CAS, conflict resolution, device key management; optional WebDAV if safe.",
  "Phase 5 — secure sync and always-on runner",
  1093
 ],
 [
  "SPEC-28-021",
  "Linux headless runner, pair/unpair, scheduler, authenticated transport, resumability, service installer and optional container example.",
  "Phase 5 — secure sync and always-on runner",
  1094
 ],
 [
  "SPEC-28-022",
  "Android monitor and approve/stop remote jobs. Clear data egress indicators.",
  "Phase 5 — secure sync and always-on runner",
  1095
 ],
 [
  "SPEC-28-023",
  "**Gate:** a 2-device conflict/restore and a VM-run task after the desktop closes, each actually tested; no operator-hosted backend required.",
  "Phase 5 — secure sync and always-on runner",
  1096
 ],
 [
  "SPEC-28-024",
  "Native Android packaging, release signing, phone layouts and document picker; mobile-safe tasks and seamless remote control; deep accessibility testing.",
  "Phase 6 — Android parity, polishing and advanced productivity",
  1100
 ],
 [
  "SPEC-28-025",
  "Enhance evidence/provenance, workflows, artifact previews, remaining useful catalog features, large-data performance, all failed UI journeys.",
  "Phase 6 — Android parity, polishing and advanced productivity",
  1101
 ],
 [
  "SPEC-28-026",
  "Re-run the original UX usability scenarios and substantially redesign weak screens, not just tweak corner radii.",
  "Phase 6 — Android parity, polishing and advanced productivity",
  1102
 ],
 [
  "SPEC-28-027",
  "**Gate:** tested APK installed/emulator and any available physical device; user can run a team, connect storage, pair machine and handle approvals.",
  "Phase 6 — Android parity, polishing and advanced productivity",
  1103
 ],
 [
  "SPEC-28-028",
  "Full matrix; red-team tests; audits; performance; accessible interaction; fresh installs/upgrades, literal GUI; documentation and screenshot accuracy.",
  "Phase 7 — hostile QA, packaging, repository and public release",
  1107
 ],
 [
  "SPEC-28-029",
  "Validate all 200 audited development skills/MCP entries and all 200 role definitions, separating evidence level.",
  "Phase 7 — hostile QA, packaging, repository and public release",
  1108
 ],
 [
  "SPEC-28-030",
  "Create actual Windows NSIS, macOS arm64/x86_64 DMGs, Linux AppImage/DEB, signed Android APK and runner binary using CI and platform toolchains, where available.",
  "Phase 7 — hostile QA, packaging, repository and public release",
  1109
 ],
 [
  "SPEC-28-031",
  "Security scan, Git repo size check, release provenance, version consistency, public GitHub push, real GitHub Release publication and post-publish install verification.",
  "Phase 7 — hostile QA, packaging, repository and public release",
  1110
 ],
 [
  "SPEC-28-032",
  "**Gate:** all must-pass tests with verifiable evidence or explicit blocker; no fictional release status. Produce final deliverables index.",
  "Phase 7 — hostile QA, packaging, repository and public release",
  1111
 ],
 [
  "SPEC-29-001",
  "invent security assumptions, release success, or credentials;",
  "Delegation and worktree safety for multi-agent Codex execution",
  1123
 ],
 [
  "SPEC-29-002",
  "delete or reset unrelated local files, force-push or clean unknown worktrees;",
  "Delegation and worktree safety for multi-agent Codex execution",
  1124
 ],
 [
  "SPEC-29-003",
  "import whole third-party projects without permission/licenses;",
  "Delegation and worktree safety for multi-agent Codex execution",
  1125
 ],
 [
  "SPEC-29-004",
  "self-approve their own destructive tool use;",
  "Delegation and worktree safety for multi-agent Codex execution",
  1126
 ],
 [
  "SPEC-29-005",
  "publish before real verification;",
  "Delegation and worktree safety for multi-agent Codex execution",
  1127
 ],
 [
  "SPEC-29-006",
  "pass unreviewed third-party instructions into higher-priority project policy;",
  "Delegation and worktree safety for multi-agent Codex execution",
  1128
 ],
 [
  "SPEC-29-007",
  "introduce a mandatory operator-funded cloud dependency;",
  "Delegation and worktree safety for multi-agent Codex execution",
  1129
 ],
 [
  "SPEC-29-008",
  "commit binaries/large model weights to satisfy repo deliverables;",
  "Delegation and worktree safety for multi-agent Codex execution",
  1130
 ],
 [
  "SPEC-29-009",
  "silently replace failing real tests with mocks or skip them.",
  "Delegation and worktree safety for multi-agent Codex execution",
  1131
 ],
 [
  "SPEC-01-001",
  "Build the **actual application**, not a demo-only site or an Electron mockup.",
  "Non-negotiable acceptance contract",
  25
 ],
 [
  "SPEC-01-002",
  "**Rust** is the authoritative language for domain model, agent scheduler, provider routing, permission engine, tool execution, persistence, synchronization, cryptography, runner, and cross-platform business logic. Provide a shared Rust workspace for desktop, Android, CLI/runner, and tests.",
  "Non-negotiable acceptance contract",
  26
 ],
 [
  "SPEC-01-003",
  "Support **Windows, macOS, Linux**, plus **Android** with a real installable, release-signed **APK**, not merely a responsive webpage. Android can have platform-specific limitations, but must maximize legitimate capabilities.",
  "Non-negotiable acceptance contract",
  27
 ],
 [
  "SPEC-01-004",
  "**Default tasks run locally** within the user's machine's security permissions. **Opt-in remote** execution must run on the user's own or explicitly configured VM/server, never a hidden developer-operated worker.",
  "Non-negotiable acceptance contract",
  28
 ],
 [
  "SPEC-01-005",
  "Support **remote AI APIs, local models, custom OpenAI-compatible endpoints**, and extensible vendor adapters. Do not hard-code a single provider or vendor-specific prompt format.",
  "Non-negotiable acceptance contract",
  29
 ],
 [
  "SPEC-01-006",
  "Support **individual DMs, group rooms, mentions, thread replies, multi-agent teamwork, task delegation, and parallel jobs** with clear attribution and control.",
  "Non-negotiable acceptance contract",
  30
 ],
 [
  "SPEC-01-007",
  "Include a **large, real, searchable catalogue of useful preconfigured roles** and a role creator/editor. Every role must have working tools/limits, not merely a different avatar and system prompt.",
  "Non-negotiable acceptance contract",
  31
 ],
 [
  "SPEC-01-008",
  "Provide **serious coding-agent capabilities** to any sufficiently capable model when permitted: inspect/edit files, use terminal, plan, apply diffs, run tests, debug, navigate repos, use Git, review diffs, browse, take screenshots when supported, and produce checkable artifacts. Do not claim that every model has the same tool-calling abilities.",
  "Non-negotiable acceptance contract",
  32
 ],
 [
  "SPEC-01-009",
  "Build a **custom brand, original icon family, beautiful minimal UI, superb motion/interaction design**, fully adaptive layouts and accessible controls. The product should look premium without looking busy.",
  "Non-negotiable acceptance contract",
  33
 ],
 [
  "SPEC-01-010",
  "Put **ordinary chat and tasks first**. Put expert settings behind an intentionally discoverable **Advanced / Studio** area with searchable controls, presets, and contextual explanations.",
  "Non-negotiable acceptance contract",
  34
 ],
 [
  "SPEC-01-011",
  "Include **zero-operator-cost bring-your-own synchronization** with at least one real user-owned storage provider integration, local encrypted backups, and multi-device conflict handling. The end user pays any third-party service fees directly; do not operate a paid centralized sync backend.",
  "Non-negotiable acceptance contract",
  35
 ],
 [
  "SPEC-01-012",
  "Include a **pairable, self-hostable, 24/7 runner** for Linux server/VM as the initial strongly verified target; design and test other OS/architectures where feasible. A server/VM still consumes somebody's resources; never promise universally free compute.",
  "Non-negotiable acceptance contract",
  36
 ],
 [
  "SPEC-01-013",
  "**Install and use exactly 200 distinct, vetted development skills and MCP integrations** by the evidence protocol in §14, without padding the count or running untrusted code blindly. Keep the installation inventory outside the shipped runtime unless a specific component is intentionally distributed. A license/security blocker must be reported transparently rather than bypassed.",
  "Non-negotiable acceptance contract",
  37
 ],
 [
  "SPEC-01-014",
  "**Literally start the application and interact with real UI** during development and release verification on all executable test platforms available. Use screenshots, accessibility tree, keyboard/mouse interactions, and Android emulator/ADB where possible; capture reproducible evidence.",
  "Non-negotiable acceptance contract",
  38
 ],
 [
  "SPEC-01-015",
  "**Public GitHub repository** only after source/security/license checks and successful release gates. Commit comprehensible, reviewed source, workflow definitions, and documentation. Do not push credentials, huge model weights, caches, dependencies, or unlicensed assets.",
  "Non-negotiable acceptance contract",
  39
 ],
 [
  "SPEC-01-016",
  "Publish **real GitHub Releases** containing installable **Windows, macOS Intel/Apple Silicon, Linux, Android** binaries where successfully built and verified, checksums, changelog, install guide, and known limitations. Never publish a misleading green release with a broken primary platform.",
  "Non-negotiable acceptance contract",
  40
 ],
 [
  "SPEC-01-017",
  "Target **less than 500 MB of total pushed Git-tracked repository content/history** for initial delivery; avoid individual tracked blobs above 50 MiB, and absolutely respect GitHub's 100 MiB tracked-file block. Build artifacts belong in release assets, not repository commits. The 500 MB limit is **not** a limit on local build cache, installed dependencies, optional downloaded models, or runtime user data.",
  "Non-negotiable acceptance contract",
  41
 ],
 [
  "SPEC-01-018",
  "Publish an exceptional, **highly visual README.md** with original screenshots, an animated demo (optimized), interface walkthrough, use cases, downloadable release links near the top, architecture, security model, limitations, and instructions for beginners.",
  "Non-negotiable acceptance contract",
  42
 ],
 [
  "SPEC-01-019",
  "Run real unit/integration/property/chaos/end-to-end/security/performance/accessibility tests; never confuse mock-only success with tested production behavior.",
  "Non-negotiable acceptance contract",
  43
 ],
 [
  "SPEC-01-020",
  "Continue through implementation, correction, packaging, verification, GitHub push, and release to the extent actual access and credentials permit. When blocked by unavailable credentials/platforms, complete every other part, leave a precise manual finish step, and **do not misrepresent the blocked step as done**.",
  "Non-negotiable acceptance contract",
  44
 ],
 [
  "SPEC-02-001",
  "**Local-first, not local-only:** the workspace and execution engine work without an account; network only when explicitly required by selected model, source, connector, sync, or paired runner.",
  "Product principles and explicit constraints",
  50
 ],
 [
  "SPEC-02-002",
  "**User owns execution:** models can suggest actions; the local/paired runtime actually executes them under verified permission grants.",
  "Product principles and explicit constraints",
  51
 ],
 [
  "SPEC-02-003",
  "**Everything visible, nothing mysterious:** explain what the agents are doing, which machine is executing, which model is paying, which files are touched, and what will be sent away.",
  "Product principles and explicit constraints",
  52
 ],
 [
  "SPEC-02-004",
  "**One obvious path per ordinary task:** natural language, a role picker, or a starter workflow. Advanced orchestration remains optional.",
  "Product principles and explicit constraints",
  53
 ],
 [
  "SPEC-02-005",
  "**Simple until powerful:** no walls of knobs, status LEDs, and dense dashboards on the default canvas.",
  "Product principles and explicit constraints",
  54
 ],
 [
  "SPEC-02-006",
  "**No false agency:** distinguish queued, planning, waiting approval, running, succeeded, partially completed, canceled, and failed. Never mark \"done\" merely because a model claimed success.",
  "Product principles and explicit constraints",
  55
 ],
 [
  "SPEC-02-007",
  "**Human authority:** high-risk actions are explicitly scoped and approved. Support undo/recovery where realistically possible.",
  "Product principles and explicit constraints",
  56
 ],
 [
  "SPEC-02-008",
  "**Multiple providers, no lock-in:** implement robust capability negotiation, not a promise that any provider/model supports tools or vision or JSON equally.",
  "Product principles and explicit constraints",
  57
 ],
 [
  "SPEC-02-009",
  "**Portable by default:** open standard exports, Markdown/JSON project descriptions, no proprietary cloud requirements.",
  "Product principles and explicit constraints",
  58
 ],
 [
  "SPEC-02-010",
  "**Privacy is architectural:** strict egress controls, provider notices, encrypt secrets, local audit logs, and optional E2EE sync.",
  "Product principles and explicit constraints",
  59
 ],
 [
  "SPEC-02-011",
  "**No hidden costs:** no mandatory subscriptions or vendor API keys to open the app; disclose provider/server/storage prices and enforce configurable spend limits.",
  "Product principles and explicit constraints",
  60
 ],
 [
  "SPEC-02-012",
  "**Low resource overhead:** no giant model downloads or heavy browser runtime bundled by default; let the user opt in to local inference and browser automation.",
  "Product principles and explicit constraints",
  61
 ],
 [
  "SPEC-02-013",
  "**Respect real platform boundaries:** Android cannot be used as an unrestricted desktop shell; desktop features may need approval and a companion runner.",
  "Product principles and explicit constraints",
  62
 ],
 [
  "SPEC-03-001",
  "**Tauri 2 with a Rust core and a custom Svelte/TypeScript/WebView frontend** on Windows/macOS/Linux/Android.",
  "Research and architecture decision gate — do this first, then implement",
  68
 ],
 [
  "SPEC-03-002",
  "**Slint with Rust core and its native/custom UI DSL** across desktop and Android.",
  "Research and architecture decision gate — do this first, then implement",
  69
 ],
 [
  "SPEC-03-003",
  "A defensible alternative only if one of the above fails a concrete requirement.",
  "Research and architecture decision gate — do this first, then implement",
  70
 ],
 [
  "SPEC-03-004",
  "Tauri platform build/distribution: https://v2.tauri.app/distribute/",
  "Research and architecture decision gate — do this first, then implement",
  80
 ],
 [
  "SPEC-03-005",
  "Tauri security capabilities: https://tauri.app/security/capabilities/",
  "Research and architecture decision gate — do this first, then implement",
  81
 ],
 [
  "SPEC-03-006",
  "Tauri shell plugin/mobile limitations: https://v2.tauri.app/plugin/shell/",
  "Research and architecture decision gate — do this first, then implement",
  82
 ],
 [
  "SPEC-03-007",
  "Tauri Android APK: https://v2.tauri.app/distribute/google-play/",
  "Research and architecture decision gate — do this first, then implement",
  83
 ],
 [
  "SPEC-03-008",
  "Slint Android: https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/android/",
  "Research and architecture decision gate — do this first, then implement",
  84
 ],
 [
  "SPEC-03-009",
  "Slint licenses: https://slint.dev/faqs",
  "Research and architecture decision gate — do this first, then implement",
  85
 ],
 [
  "SPEC-03-010",
  "Android background restrictions: https://developer.android.com/develop/background-work/background-tasks/bg-work-restrictions",
  "Research and architecture decision gate — do this first, then implement",
  86
 ],
 [
  "SPEC-03-011",
  "Android foreground-service changes: https://developer.android.com/develop/background-work/services/fgs/changes",
  "Research and architecture decision gate — do this first, then implement",
  87
 ],
 [
  "SPEC-04-001",
  "**Newcomer:** no AI vocabulary, wants \"make this document clearer,\" \"organize these files,\" \"plan an event.\" Needs presets and clear confirmations.",
  "Personas",
  95
 ],
 [
  "SPEC-04-002",
  "**Student and researcher:** asks a team to search, summarize, cite, analyze documents/data, and create reproducible outputs.",
  "Personas",
  96
 ],
 [
  "SPEC-04-003",
  "**Developer:** wants coding agents with filesystem/terminal/Git privileges in a selected project, efficient diffs, tests, review, and branch integration.",
  "Personas",
  97
 ],
 [
  "SPEC-04-004",
  "**Creator and marketer:** needs brainstorming, writing, script/storyboard, assets, calendars, accessible review and export.",
  "Personas",
  98
 ],
 [
  "SPEC-04-005",
  "**Operator/analyst:** wants automation, spreadsheets, reports, workflows, connector sync, schedules, audit trails.",
  "Personas",
  99
 ],
 [
  "SPEC-04-006",
  "**Power user:** mixes providers, agents, tools, role permissions, self-hosted runners, and collaboration graph.",
  "Personas",
  100
 ],
 [
  "SPEC-04-007",
  "**Android-first user:** monitors tasks, chats, reviews approvals, uses mobile-appropriate tools, delegates heavy jobs to paired desktop/server.",
  "Personas",
  101
 ],
 [
  "SPEC-04-008",
  "Fresh install → no-login introduction → choose one objective → add/test one provider or local inference endpoint → send first message → inspect reply. No forced cloud sign-up.",
  "Must-pass user journeys",
  105
 ],
 [
  "SPEC-04-009",
  "Pick \"Researcher\" and \"Writer\" → create a room → @mention a researcher → route factual findings to writer → review draft → export Markdown/PDF where actually supported.",
  "Must-pass user journeys",
  106
 ],
 [
  "SPEC-04-010",
  "Add arbitrary compatible API endpoint → test auth/capabilities/tool-calling → choose fallback → see cost warnings → chat in 1:1 and group.",
  "Must-pass user journeys",
  107
 ],
 [
  "SPEC-04-011",
  "Add local Ollama/LM Studio endpoint → airplane-mode task using local files and supported on-device tools → no unexpected network egress.",
  "Must-pass user journeys",
  108
 ],
 [
  "SPEC-04-012",
  "Choose coding agent → grant one directory → inspect repo → propose plan/diff → approve apply → run tests → see pass/fail → revert or commit.",
  "Must-pass user journeys",
  109
 ],
 [
  "SPEC-04-013",
  "Initiate task with multiple workers → see queue, parallel steps, progress, tool permissions, final artifacts, provenance and failure recovery.",
  "Must-pass user journeys",
  110
 ],
 [
  "SPEC-04-014",
  "Attach a long document → ask a question → see citation spans and evidence versus unsupported assertions → export findings.",
  "Must-pass user journeys",
  111
 ],
 [
  "SPEC-04-015",
  "Configure sync with a storage account owned by the user → pair second device → reconcile a deliberate conflicting edit without data loss.",
  "Must-pass user journeys",
  112
 ],
 [
  "SPEC-04-016",
  "Pair a user-owned Linux VM → schedule a task to run while laptop is off → monitor from Android → revoke remote execution from desktop.",
  "Must-pass user journeys",
  113
 ],
 [
  "SPEC-04-017",
  "Deny shell permission → agent fails closed; it must not acquire another tool to bypass the denial.",
  "Must-pass user journeys",
  114
 ],
 [
  "SPEC-04-018",
  "Recover from crash mid-task → durable task state reflects incomplete work; no duplicate emails/files or phantom completions.",
  "Must-pass user journeys",
  115
 ],
 [
  "SPEC-04-019",
  "Android install → onboarding → group chat → approve a remote task → send attachment from document picker → notifications only when allowed → reconnect after background suspend.",
  "Must-pass user journeys",
  116
 ],
 [
  "SPEC-04-020",
  "Screen-reader/keyboard-only beginner completes simple onboarding and task; reduced motion and 200% zoom remain usable.",
  "Must-pass user journeys",
  117
 ],
 [
  "SPEC-04-021",
  "A user disconnects one provider, uninstalls an MCP integration, or deletes a room; deletion and secret retention are explicit and testable.",
  "Must-pass user journeys",
  118
 ],
 [
  "SPEC-05-001",
  "**Home / Today** — friendly activity summary, recent rooms, resume task, three suggested starting actions, compact notifications, currently paired machines.",
  "Five primary destinations, not forty",
  124
 ],
 [
  "SPEC-05-002",
  "**Rooms** — DMs, group rooms, threads, messages, files, mentions, reactions where useful. This is the **main product**.",
  "Five primary destinations, not forty",
  125
 ],
 [
  "SPEC-05-003",
  "**Tasks** — unified live queue and task history with progress, approval requests, deliverables, and recoveries.",
  "Five primary destinations, not forty",
  126
 ],
 [
  "SPEC-05-004",
  "**Library** — searchable role gallery, saved workflows, templates, artifacts, optional tools; role discovery must not clutter Rooms.",
  "Five primary destinations, not forty",
  127
 ],
 [
  "SPEC-05-005",
  "**Settings** — simple essentials first, with **Advanced Studio** behind an obvious secondary action.",
  "Five primary destinations, not forty",
  128
 ],
 [
  "SPEC-05-006",
  "Messages feel like clean, readable chat, not terminal logs.",
  "Primary room design",
  134
 ],
 [
  "SPEC-05-007",
  "A group message clearly identifies human vs agent, role, provider/model, timestamp, and optional cost without displaying seven badges on every message.",
  "Primary room design",
  135
 ],
 [
  "SPEC-05-008",
  "Every running agent has a short, intelligible state (e.g., \"Reading 3 files\", \"Waiting for your approval\") with expandable evidence.",
  "Primary room design",
  136
 ],
 [
  "SPEC-05-009",
  "Composer supports plain language, `@agent`, `/workflow`, attachment, voice dictation if platform/provider permits, and selection of run target (This Device / Paired Machine) when relevant.",
  "Primary room design",
  137
 ],
 [
  "SPEC-05-010",
  "Allow **Send to one**, **Ask team**, **Debate**, **Delegate**, **Parallel research**, and **Quick answer** as unobtrusive composer actions/presets.",
  "Primary room design",
  138
 ],
 [
  "SPEC-05-011",
  "Threaded subtask conversations should be nested or drawer-based; avoid making the default room unreadable.",
  "Primary room design",
  139
 ],
 [
  "SPEC-05-012",
  "Show reasoning **summaries and tool logs only**; do not promise raw private chain-of-thought from proprietary APIs.",
  "Primary room design",
  140
 ],
 [
  "SPEC-05-013",
  "Each output artifact opens in a beautiful preview with provenance, versions and an explicit Save/Export action.",
  "Primary room design",
  141
 ],
 [
  "SPEC-05-014",
  "Keep drag-and-drop attachments, paste images, code blocks with copy, mathematical notation, document links, and markdown readability.",
  "Primary room design",
  142
 ],
 [
  "SPEC-06-001",
  "A beginner can start one task in **no more than three primary decisions** after a valid provider is available.",
  "Quantitative UX gates",
  170
 ],
 [
  "SPEC-06-002",
  "The main Chat/Rooms screen renders meaningfully with **zero provider configured**; tutorial/demo content is explicitly labeled as simulation and never fake a real agent execution.",
  "Quantitative UX gates",
  171
 ],
 [
  "SPEC-06-003",
  "Any destructive/high-cost action must make its **effect and target** understandable before confirmation.",
  "Quantitative UX gates",
  172
 ],
 [
  "SPEC-06-004",
  "**Keyboard-only** access for every desktop action, obvious focus order, complete screen reader names, accessible dialogs and live updates.",
  "Quantitative UX gates",
  173
 ],
 [
  "SPEC-06-005",
  "Aim for WCAG **2.2 AA** where applicable (test contrast, zoom, focus, touch targets, non-drag alternative, color independence, reduced motion).",
  "Quantitative UX gates",
  174
 ],
 [
  "SPEC-06-006",
  "Preserve selections and input drafts through pane switches, scrolling, resizes and reconnects.",
  "Quantitative UX gates",
  175
 ],
 [
  "SPEC-06-007",
  "Long chats must virtualize without scroll jumps; auto-scroll only when user is at bottom; new-message indicator otherwise.",
  "Quantitative UX gates",
  176
 ],
 [
  "SPEC-06-008",
  "Test windows at 1280×800, 1024×768, 900×600, large scaling, 200% zoom; Android phones in compact and large sizes, portrait and landscape.",
  "Quantitative UX gates",
  177
 ],
 [
  "SPEC-06-009",
  "Default screen must remain calm with 3–5 strong visual focal elements, not 25 competing controls. Advanced features use search, hover/long-press, command palette, drawers or intentional Studio navigation.",
  "Quantitative UX gates",
  178
 ],
 [
  "SPEC-06-010",
  "Real context menus, command palette (`Ctrl/Cmd+K`), inline shortcuts discoverability, and well-tested undo where safe.",
  "Quantitative UX gates",
  179
 ],
 [
  "SPEC-06-011",
  "Accessibility and error states receive the **same design effort** as polished happy paths.",
  "Quantitative UX gates",
  180
 ],
 [
  "SPEC-07-001",
  "Async: Tokio, structured concurrency, cancellation tokens, bounded channels.",
  "Suggested technologies (verify licenses and latest compatibility)",
  236
 ],
 [
  "SPEC-07-002",
  "Domain serialization: serde with strict JSON schema/versioning; typed enums for approval/risk/task states.",
  "Suggested technologies (verify licenses and latest compatibility)",
  237
 ],
 [
  "SPEC-07-003",
  "Storage: SQLite in WAL mode, proper migrations, foreign keys, transactional outbox, FTS5; optional vector index separate and validated.",
  "Suggested technologies (verify licenses and latest compatibility)",
  238
 ],
 [
  "SPEC-07-004",
  "HTTP: robust timeouts, TLS validation, retries with jitter, rate limits, streaming SSE/WebSocket as supported.",
  "Suggested technologies (verify licenses and latest compatibility)",
  239
 ],
 [
  "SPEC-07-005",
  "Remote: SSH setup/pairing with authenticated tunnel or mutually authenticated encrypted transport; optional Tailscale/WireGuard where user-configured; no unauthenticated public admin port.",
  "Suggested technologies (verify licenses and latest compatibility)",
  240
 ],
 [
  "SPEC-07-006",
  "Crypto: audited standard libraries for Argon2id + XChaCha20-Poly1305 or AES-256-GCM with versioned envelopes; OS keychain/Android Keystore for secrets, with secure fallback and clear caveat.",
  "Suggested technologies (verify licenses and latest compatibility)",
  241
 ],
 [
  "SPEC-07-007",
  "Filesystem: OS-native paths, canonicalization, symlink/hardlink race defenses, volume boundaries, no escape from granted workspace, limits on file reads and archives.",
  "Suggested technologies (verify licenses and latest compatibility)",
  242
 ],
 [
  "SPEC-07-008",
  "Tracing: structured spans/event IDs, secrets/PII redaction, local diagnostics opt-in.",
  "Suggested technologies (verify licenses and latest compatibility)",
  243
 ],
 [
  "SPEC-07-009",
  "Testing: cargo test, Clippy strict, rustfmt, property-based tests, mock service adapters, UI automation, Android UI Automator/ADB, filesystem and network fault injection.",
  "Suggested technologies (verify licenses and latest compatibility)",
  244
 ],
 [
  "SPEC-08-001",
  "Workspace: id, name, settings, encryption profile, role catalog, linked repositories, devices.",
  "Domain entities and required state machines",
  252
 ],
 [
  "SPEC-08-002",
  "User profile: local identity, device keys, optional display name, app preferences; no remote account required.",
  "Domain entities and required state machines",
  253
 ],
 [
  "SPEC-08-003",
  "Provider connection: adapter family, URL, TLS/auth policy, secret reference, proxy, limits, availability.",
  "Domain entities and required state machines",
  254
 ],
 [
  "SPEC-08-004",
  "Model profile: exact identifier, runtime capability observations, text/vision/audio/tool/JSON context, pricing (user input/verified), rate limit, health, fallbacks.",
  "Domain entities and required state machines",
  255
 ],
 [
  "SPEC-08-005",
  "Agent: id, role version, custom name, model strategy, allowed tools, execution target, personality, context policies, budget, memory, evaluation policy.",
  "Domain entities and required state machines",
  256
 ],
 [
  "SPEC-08-006",
  "Room/Thread/Message: ordered durable events, roles, attachments, replies, references, mentions, delivery states.",
  "Domain entities and required state machines",
  257
 ],
 [
  "SPEC-08-007",
  "Task: request, owner, agent(s), source room, actual executor device, graph state, timeout, cost, authorization, artifact provenance, retries.",
  "Domain entities and required state machines",
  258
 ],
 [
  "SPEC-08-008",
  "TaskStep: typed input/output, tool call, immutable request hash, idempotency key, risk, approval, effects, receipts, checkpoints.",
  "Domain entities and required state machines",
  259
 ],
 [
  "SPEC-08-009",
  "PermissionGrant: principal, tool, path/domain/action, expiration, one-shot/room/session persistent mode, revocation and audit.",
  "Domain entities and required state machines",
  260
 ],
 [
  "SPEC-08-010",
  "Skill: id, version, publisher, digest, capabilities, inputs, outputs, tests, dependencies, trust/signature, policy requirements.",
  "Domain entities and required state machines",
  261
 ],
 [
  "SPEC-08-011",
  "Tool/MCP server: transport, version, discovered schemas, trust tier, credentials, permission scopes, actual availability.",
  "Domain entities and required state machines",
  262
 ],
 [
  "SPEC-08-012",
  "Artifact: MIME, SHA-256, immutable content ID, creation input/source provenance, preview and export format.",
  "Domain entities and required state machines",
  263
 ],
 [
  "SPEC-08-013",
  "Execution target: local, paired desktop, paired VM/server, temporary container; capability matrix and attestation.",
  "Domain entities and required state machines",
  264
 ],
 [
  "SPEC-08-014",
  "Schedule: calendar/timezone/recurrence, misfire and catch-up behavior, expiration, pause state.",
  "Domain entities and required state machines",
  265
 ],
 [
  "SPEC-08-015",
  "Sync device/revision: device public key, encrypted ops/revisions, vector/causal clocks or version tokens, conflicts, retries and reconciliation.",
  "Domain entities and required state machines",
  266
 ],
 [
  "SPEC-08-016",
  "Audit event: immutable trace of grants, changed files, external egress, runner actions, spend, decisions, undo/recovery.",
  "Domain entities and required state machines",
  267
 ],
 [
  "SPEC-11-001",
  "Inspect repository, browse a project tree, search names and content (gitignore-aware), grep/ripgrep, symbolic references where available, inspect file metadata.",
  "Coding toolbox",
  356
 ],
 [
  "SPEC-11-002",
  "Read and edit UTF-8/source documents with diffs, line context, encoding detection, binary-file detection and path-scoped policies.",
  "Coding toolbox",
  357
 ],
 [
  "SPEC-11-003",
  "Propose patch, validate clean application, apply atomically, revert/undo selected patches, compare before/after, and detect concurrent edits.",
  "Coding toolbox",
  358
 ],
 [
  "SPEC-11-004",
  "Run formatter, compiler, tests, linter, static analyzer, benchmarks; capture exit code/stdout/stderr; mark timeouts.",
  "Coding toolbox",
  359
 ],
 [
  "SPEC-11-005",
  "Launch supported development servers or test applications; discover local port using explicit process ownership; shut down child process trees reliably.",
  "Coding toolbox",
  360
 ],
 [
  "SPEC-11-006",
  "Git status/diff/log/branch/worktree, generate commit messages, create branches and commits after confirmation, stage selected files, create PRs only through explicit authenticated connector permission; never force-push or rewrite unknown history.",
  "Coding toolbox",
  361
 ],
 [
  "SPEC-11-007",
  "Inspect issues/review feedback with authorized connectors; propose release notes and changelog.",
  "Coding toolbox",
  362
 ],
 [
  "SPEC-11-008",
  "Browser-based webpage testing, screenshots, console/network inspection and user-guided accessibility inspection where legitimate and permitted.",
  "Coding toolbox",
  363
 ],
 [
  "SPEC-11-009",
  "Test plan generation, unit/integration/E2E test writing, bug isolation, dependency/license/SBOM checks, coverage change analysis.",
  "Coding toolbox",
  364
 ],
 [
  "SPEC-11-010",
  "Documentation and API reference generation, refactor, dependency upgrades with regression checks, reproducible experiment runners.",
  "Coding toolbox",
  365
 ],
 [
  "SPEC-11-011",
  "Run containerized/project-scoped toolchains when the user explicitly enables container access. No implicit host Docker socket privilege.",
  "Coding toolbox",
  366
 ],
 [
  "SPEC-11-012",
  "Optional code language servers and debugger integration; accurate \"not available\" states for unavailable tools.",
  "Coding toolbox",
  367
 ],
 [
  "SPEC-11-013",
  "Repository indexing, retrieval over approved project files, code knowledge maps, generated code review and risk analysis.",
  "Coding toolbox",
  368
 ],
 [
  "SPEC-11-014",
  "Concurrent agent coding must use separate worktrees or scoped locks; offer diff merge/approval in GUI.",
  "Coding toolbox",
  369
 ],
 [
  "SPEC-11-015",
  "Search history and outputs and resume from interrupted operation with verifiable task state.",
  "Coding toolbox",
  370
 ],
 [
  "SPEC-15-001",
  "**This computer:** default; Rust agent engine/tool supervisor operates on current desktop OS under the current user's privileges plus app policy.",
  "Execution target selector",
  527
 ],
 [
  "SPEC-15-002",
  "**This Android phone:** only Android-safe operations (app-private files, document-provider granted URIs, network API calls, local inference if hardware/model available, media/document creation, approved intents). No generic unattended host shell, arbitrary desktop process spawning or privileged filesystem access.",
  "Execution target selector",
  528
 ],
 [
  "SPEC-15-003",
  "**Paired desktop:** a separately installed Rust runner on a second user-controlled computer, over authenticated connection.",
  "Execution target selector",
  529
 ],
 [
  "SPEC-15-004",
  "**Paired server/VM:** user-installed headless Rust runner with explicit scopes; can execute scheduled jobs without the UI being open.",
  "Execution target selector",
  530
 ],
 [
  "SPEC-15-005",
  "**Optional isolated container:** user-approved container runtime on a supported host; not a mandatory dependency and never unrestricted privileged Docker access.",
  "Execution target selector",
  531
 ],
 [
  "SPEC-15-006",
  "**Ask me** (default for writes, shell, network egress to new endpoints, accounts and destructive actions).",
  "Permissions UX",
  545
 ],
 [
  "SPEC-15-007",
  "**Allow for this workspace** (explicit tool + path/domain/action + duration + budget; user can inspect/revoke).",
  "Permissions UX",
  546
 ],
 [
  "SPEC-15-008",
  "**Restricted / view only** (reads within selected scope, no writes or shell).",
  "Permissions UX",
  547
 ],
 [
  "SPEC-09-001",
  "**OpenAI-style:** supported OpenAI APIs, custom OpenAI-compatible Chat Completions/Responses endpoints, Azure variants where available; include required API/endpoint-version differences.",
  "Provider families",
  279
 ],
 [
  "SPEC-09-002",
  "**Anthropic-style:** first-party Anthropic adapter, or explicitly enabled compatibility endpoint where tested. Account for message/tool protocol differences.",
  "Provider families",
  280
 ],
 [
  "SPEC-09-003",
  "**Google Gemini:** supported direct API adapter and applicable streaming/tool features.",
  "Provider families",
  281
 ],
 [
  "SPEC-09-004",
  "**General managed provider routing:** configurable gateways such as OpenRouter or self-hosted routing proxies, only where lawful and documented.",
  "Provider families",
  282
 ],
 [
  "SPEC-09-005",
  "**Cloud platforms:** optional AWS Bedrock, Azure, or compatible enterprise endpoints with user-supplied credentials; never claim every service is identical.",
  "Provider families",
  283
 ],
 [
  "SPEC-09-006",
  "**Local inference:** Ollama, LM Studio, llama.cpp server, vLLM, and other custom endpoints through their actual supported protocols; do not download any large model without explicit opt-in.",
  "Provider families",
  284
 ],
 [
  "SPEC-09-007",
  "**Coding agents as managed peers:** optional ACP-compatible agent subprocesses where supported and trusted; keep explicit that a coding-agent integration is not just a raw chat model API.",
  "Provider families",
  285
 ],
 [
  "SPEC-09-008",
  "**Future adapter SDK:** give maintainers a small stable Rust interface, fixture transport, contract tests and conformance manifest. Include import/export of non-secret connection templates.",
  "Provider families",
  286
 ],
 [
  "SPEC-10-001",
  "**Solo:** one agent and a human.",
  "Collaboration models to implement",
  318
 ],
 [
  "SPEC-10-002",
  "**Roundtable:** user asks all selected members; replies independently, then optionally synthesize.",
  "Collaboration models to implement",
  319
 ],
 [
  "SPEC-10-003",
  "**Sequential handoff:** researcher → analyst → writer → reviewer, with evidence attached.",
  "Collaboration models to implement",
  320
 ],
 [
  "SPEC-10-004",
  "**Parallel specialists:** non-overlapping subtasks, bounded concurrency and resources, merge/review stage.",
  "Collaboration models to implement",
  321
 ],
 [
  "SPEC-10-005",
  "**Planner–executor:** planning model proposes DAG; execution agents perform typed steps, validators check completion.",
  "Collaboration models to implement",
  322
 ],
 [
  "SPEC-10-006",
  "**Debate + adjudication:** disagreements surfaced with cited evidence and human-visible uncertainties, not invented consensus.",
  "Collaboration models to implement",
  323
 ],
 [
  "SPEC-10-007",
  "**Reviewer/critic:** peer agent reviews an artifact or code change without silently editing it.",
  "Collaboration models to implement",
  324
 ],
 [
  "SPEC-10-008",
  "**Manager/delegates:** leader breaks work into tasks; workers have independent budgets, deadlines and tool permissions.",
  "Collaboration models to implement",
  325
 ],
 [
  "SPEC-10-009",
  "**Human-in-the-loop:** interrupt, comment, ask for alternatives, approve narrow actions, or reassign before continuation.",
  "Collaboration models to implement",
  326
 ],
 [
  "SPEC-10-010",
  "**Scheduled autonomous run:** durable named workflow runs on local always-on computer or paired server within configured permissions.",
  "Collaboration models to implement",
  327
 ],
 [
  "SPEC-10-011",
  "Distinguish public room messages from internal machine-readable task events; don't leak private agent scratchpads or chain-of-thought.",
  "Coordination semantics",
  331
 ],
 [
  "SPEC-10-012",
  "Support `@all`, `@role`, `@agent`, scoped reply to a thread, assign-to, ask-for-critique, pin relevant outputs and compare replies.",
  "Coordination semantics",
  332
 ],
 [
  "SPEC-10-013",
  "A single user instruction must not cause every agent to answer unless the selected dispatch strategy says so.",
  "Coordination semantics",
  333
 ],
 [
  "SPEC-10-014",
  "Avoid infinite agent ping-pong by a message budget, step budget, recursion limit, idle timeout and explicit completion conditions.",
  "Coordination semantics",
  334
 ],
 [
  "SPEC-10-015",
  "Do not allow untrusted external content to spawn unlimited agents or override room policy.",
  "Coordination semantics",
  335
 ],
 [
  "SPEC-10-016",
  "Shared context includes task brief, authorized artifacts, selected summaries and citations; each agent receives only data necessary for its step.",
  "Coordination semantics",
  336
 ],
 [
  "SPEC-10-017",
  "Manage file contention through worktree/file locks, merge review, conflict detection and a serialized write stage.",
  "Coordination semantics",
  337
 ],
 [
  "SPEC-10-018",
  "Explain provenance: which model did the work, what tools ran, which files changed, what was assumed, what was verified.",
  "Coordination semantics",
  338
 ],
 [
  "SPEC-10-019",
  "When agents disagree, present a brief **\"Where they differ\"** section, not two overlapping long walls of text.",
  "Coordination semantics",
  339
 ],
 [
  "SPEC-10-020",
  "Expose \"Regenerate with another model,\" \"Ask a reviewer,\" \"Continue from checkpoint,\" \"Undo changes\" (when possible), and \"Make this a reusable workflow.\"",
  "Coordination semantics",
  340
 ],
 [
  "SPEC-19-001",
  "Rich Markdown editor/preview, PDF text extraction and page citations, DOCX import/export if reliable, plaintext, HTML, CSV/TSV/JSON, spreadsheet formats when supported, images with metadata and opt-in vision, common code and archive formats.",
  "Files and documents",
  683
 ],
 [
  "SPEC-19-002",
  "Structured artifact store with immutable versions, diffs, comments, exports and task provenance. Protect against ZIP-slip, decompression bombs, malicious office macro payloads and unsafe HTML preview.",
  "Files and documents",
  684
 ],
 [
  "SPEC-19-003",
  "File organization with preview/dry-run before renames/moves; duplicate detection via hashes; batch actions with recoverable trash and transaction log.",
  "Files and documents",
  685
 ],
 [
  "SPEC-19-004",
  "OCR only when a suitable installed/local service or authorized API is available; label which engine performed it and its privacy implications.",
  "Files and documents",
  686
 ],
 [
  "SPEC-19-005",
  "Document comparison, term extraction, summarize-with-citations, classification, structured extraction with schema validation and review, table extraction and source-grounded QA.",
  "Files and documents",
  687
 ],
 [
  "SPEC-19-006",
  "Ingest selected local folder/documents and explicitly authorized cloud sources.",
  "Knowledge/RAG",
  691
 ],
 [
  "SPEC-19-007",
  "Chunk provenance with original source URI/path, page/line/span anchors, content hash, extraction version and timestamp.",
  "Knowledge/RAG",
  692
 ],
 [
  "SPEC-19-008",
  "Full-text retrieval as a working baseline; optional embedding/semantic/hybrid search with local or BYO embeddings; hybrid ranking and deduplication; measured relevance tests.",
  "Knowledge/RAG",
  693
 ],
 [
  "SPEC-19-009",
  "Context packing, prompt-injection defense on retrieved content, citation-grounded answers with \"unsupported\" when no evidence.",
  "Knowledge/RAG",
  694
 ],
 [
  "SPEC-19-010",
  "Reindex incrementally and detect file changed/deleted; enforce permissions on both indexing and retrieval; never index outside authorized roots.",
  "Knowledge/RAG",
  695
 ],
 [
  "SPEC-19-011",
  "Corpus and ingestion settings with explicit disk/network costs; no silent uploading entire local drives for embeddings.",
  "Knowledge/RAG",
  696
 ],
 [
  "SPEC-19-012",
  "Ability to create a dedicated knowledge room linked to a local corpus, with test queries and evidence inspector.",
  "Knowledge/RAG",
  697
 ],
 [
  "SPEC-19-013",
  "Clean CSV/Excel data with human-visible preview, schema inference, type validation, deduplication and change log.",
  "Data and research",
  700
 ],
 [
  "SPEC-19-014",
  "Optional sandboxed Python/R runtimes if user installs and authorizes; Rust-native data processing for common tasks; avoid bundling a huge Python distribution in all installers.",
  "Data and research",
  701
 ],
 [
  "SPEC-19-015",
  "Descriptive statistics, reproducible notebooks/scripts, charts, summary tables, exportable methods and assumptions; flag possible leakage and missing values.",
  "Data and research",
  702
 ],
 [
  "SPEC-19-016",
  "Research planning, public literature search through connected sources, citation metadata, methods critiques, evidence gap detection and generated bibliography with source verification. No fabricated citations.",
  "Data and research",
  703
 ],
 [
  "SPEC-19-017",
  "Project templates for reproducible experiments, Git-friendly reports and structured research logs.",
  "Data and research",
  704
 ],
 [
  "SPEC-19-018",
  "Images attach, preview, annotate; AI vision where selected model supports; image generation via a properly configured provider, with visible costs and permissions.",
  "Images, audio and creative tools",
  708
 ],
 [
  "SPEC-19-019",
  "Transcription/audio notes via local/provider capabilities with opt-in privacy disclosure; voice notes are not recorded silently.",
  "Images, audio and creative tools",
  709
 ],
 [
  "SPEC-19-020",
  "Video/source media planning, subtitles, storyboard layouts, and video transcript summarization when tools are explicitly installed.",
  "Images, audio and creative tools",
  710
 ],
 [
  "SPEC-19-021",
  "Diagram/flowchart generation through safe renderers, export PNG/SVG and schema-based chart generation.",
  "Images, audio and creative tools",
  711
 ],
 [
  "SPEC-19-022",
  "User-controlled content workspaces with editable intermediate artifacts instead of opaque generated files.",
  "Images, audio and creative tools",
  712
 ],
 [
  "SPEC-19-023",
  "Workflow triggers, approved web APIs, external connector calls, desktop file watchers, notifications, safe email draft generation and **explicit send confirmation**.",
  "Automation, applications and external systems",
  716
 ],
 [
  "SPEC-19-024",
  "Calendar/task integration only with connected user authorization, guarded destructive operations and accurate timezone handling.",
  "Automation, applications and external systems",
  717
 ],
 [
  "SPEC-19-025",
  "GitHub/repo/issue/PR workflows with explicit authenticated user approvals.",
  "Automation, applications and external systems",
  718
 ],
 [
  "SPEC-19-026",
  "Web research with URLs, sources and source timestamps; avoid claims of real-time data when only cached results exist.",
  "Automation, applications and external systems",
  719
 ],
 [
  "SPEC-19-027",
  "Custom webhooks with authentication, request validation and rate limits; no public listener enabled by default.",
  "Automation, applications and external systems",
  720
 ],
 [
  "SPEC-19-028",
  "Open protocol for additional community plugins but a strong permission and signing/review model.",
  "Automation, applications and external systems",
  721
 ],
 [
  "SPEC-12-001",
  "**Role template:** what the agent is supposed to accomplish, how it communicates and what default capabilities it requests.",
  "Distinguish these four things",
  390
 ],
 [
  "SPEC-12-002",
  "**Skill:** versioned instructions/workflows/resources with clear scope and testable input/output contracts; can be part of a role.",
  "Distinguish these four things",
  391
 ],
 [
  "SPEC-12-003",
  "**Tool/MCP server:** executable or remote protocol endpoint providing real external data/actions, with its own permissions/trust surface.",
  "Distinguish these four things",
  392
 ],
 [
  "SPEC-12-004",
  "**Coding-agent ACP adapter:** interop with an external coding agent speaking Agent Client Protocol; not a substitute for Model Context Protocol.",
  "Distinguish these four things",
  393
 ],
 [
  "SPEC-12-005",
  "https://modelcontextprotocol.io/specification",
  "Distinguish these four things",
  399
 ],
 [
  "SPEC-12-006",
  "https://blog.modelcontextprotocol.io/posts/2026-07-28/",
  "Distinguish these four things",
  400
 ],
 [
  "SPEC-12-007",
  "https://registry.modelcontextprotocol.io/",
  "Distinguish these four things",
  401
 ],
 [
  "SPEC-12-008",
  "https://github.com/agentclientprotocol",
  "Distinguish these four things",
  402
 ],
 [
  "SPEC-12-009",
  "Model output, webpage text, repository READMEs, MCP tool descriptions/responses, skill descriptions, images and documents are **untrusted data**. None can silently change the hierarchy of instructions, install tools or widen permissions.",
  "Security hard boundaries",
  412
 ],
 [
  "SPEC-12-010",
  "Per-tool and per-resource consent, domain/path allowlists, SSRF protection (metadata addresses, localhost/loopback/internal IP policies), response-size limits, sanitized logs, schema validation, deterministic policy checks independent of LLMs.",
  "Security hard boundaries",
  413
 ],
 [
  "SPEC-12-011",
  "Resolve DNS rebinding and redirect destinations under consistent network policy. Local endpoint exception only for trusted configured providers; no arbitrary remote MCP access to localhost.",
  "Security hard boundaries",
  414
 ],
 [
  "SPEC-12-012",
  "Reject dangerous role/skill installations without review. Ensure process execution runs with low privileges, OS permissions, and sandbox where possible. Disallow reading secrets via environment/process inspection.",
  "Security hard boundaries",
  415
 ],
 [
  "SPEC-12-013",
  "Store keys/tokens exclusively in OS keyring or encrypted user store; never put them in Markdown exports, screenshots, logs, sync snapshots by default.",
  "Security hard boundaries",
  416
 ],
 [
  "SPEC-12-014",
  "OAuth where available with state/PKCE, correct redirect allowlisting, token rotation/revocation. Auth secrets belong to the user and service.",
  "Security hard boundaries",
  417
 ],
 [
  "SPEC-12-015",
  "Every tool execution produces an audit event, redacted arguments summary and evidence of effect. A cancelled approval token must not be reusable.",
  "Security hard boundaries",
  418
 ],
 [
  "SPEC-14-001",
  "Rust architecture and APIs; 2. async/concurrency; 3. Rust FFI/mobile; 4. Tauri/Slint UI; 5. Svelte/TypeScript frontend; 6. design systems; 7. accessibility; 8. desktop usability; 9. Android engineering; 10. motion/interaction design; 11. visual branding; 12. multi-agent coordination; 13. LLM provider adapters; 14. prompt and role design; 15. MCP interoperability; 16. ACP integration; 17. RAG/search/provenance; 18. privacy/cryptography; 19. threat modelling and app security; 20. sandboxing and OS permissions; 21. databases/migrations; 22. synchronization/distributed systems; 23. networking/SSH/TLS; 24. test automation/E2E; 25. fault injection/performance; 26. release packaging/signing; 27. GitHub CI and maintenance; 28. documentation/technical writing; 29. UX content/localization; 30. licensing/supply chain audits.",
  "Skill coverage to search, select and validate",
  486
 ],
 [
  "SPEC-13-001",
  "Make the **top 12 recommended roles** visible in an engaging gallery by common objective, not occupational jargon; the remaining catalogue is behind search, tags and smart recommendations.",
  "Role experience requirements",
  466
 ],
 [
  "SPEC-13-002",
  "One role can work with different providers, models, budgets, locales and tools; avoid coupling \"role\" to one vendor.",
  "Role experience requirements",
  467
 ],
 [
  "SPEC-13-003",
  "Natural-language role creation: user describes the desired assistant; app generates a draft, previews requested tools/data and asks for confirmation. A role created by AI must not acquire privileges without explicit grants.",
  "Role experience requirements",
  468
 ],
 [
  "SPEC-13-004",
  "Templates are versioned, human-editable, exportable, clonable and uninstallable; support user favorites, collections, task examples, capability badges and locally installed skill bundles.",
  "Role experience requirements",
  469
 ],
 [
  "SPEC-13-005",
  "Test role configuration, prompt schema, tool scopes, example intents and output format with deterministic fixtures; don't claim the model is certified for every job.",
  "Role experience requirements",
  470
 ],
 [
  "SPEC-13-006",
  "An agent can hold multiple compatible roles, but display one active task role to avoid identity confusion.",
  "Role experience requirements",
  471
 ],
 [
  "SPEC-13-007",
  "Add domain packs later without adding clutter to top-level navigation.",
  "Role experience requirements",
  472
 ],
 [
  "SPEC-16-001",
  "`runner install` interactive CLI with safe defaults; `runner doctor`, `runner status`, `runner update`, `runner logs`, `runner revoke`, `runner uninstall`, `runner backup`, `runner self-test`.",
  "Installer and setup",
  563
 ],
 [
  "SPEC-16-002",
  "**Linux:** signed/pinned release artifact for x86_64 and aarch64 where built; systemd user service or system service with dedicated low-privilege account; optional Docker/Podman image/Compose with strict non-privileged example; documented ARM small VM path.",
  "Installer and setup",
  564
 ],
 [
  "SPEC-16-003",
  "**Windows:** per-user scheduled task/service only if justified, tested; proper restart and shutdown behavior.",
  "Installer and setup",
  565
 ],
 [
  "SPEC-16-004",
  "**macOS:** launchd user agent and notarization/signing caveats.",
  "Installer and setup",
  566
 ],
 [
  "SPEC-16-005",
  "Local LAN paired runner, remote SSH bootstrapping, and user-managed Tailscale/WireGuard are desirable; do not make any commercial VPN mandatory.",
  "Installer and setup",
  567
 ],
 [
  "SPEC-16-006",
  "Preserve a complete manual installation option: download release, verify checksum, install, pair using one-time code/QR, grant workspace/data scopes, test simple dry-run, configure scheduler and confirm health.",
  "Installer and setup",
  568
 ],
 [
  "SPEC-16-007",
  "Avoid public inbound ports when practical. If exposing a service is explicitly chosen, require mutual authentication, strict TLS, firewall guidance, rotating pairing credentials and narrow network allowlists.",
  "Installer and setup",
  569
 ],
 [
  "SPEC-16-008",
  "Versioned protobuf/JSON command protocol, request IDs, capability negotiation, signed/authenticated device identity, replay protection, mutual authentication, certificate/key rotation and revocation.",
  "Remote task protocol",
  573
 ],
 [
  "SPEC-16-009",
  "Pairing via short-lived, one-time challenge with explicit fingerprint verification on both devices; QR optional for Android.",
  "Remote task protocol",
  574
 ],
 [
  "SPEC-16-010",
  "A task dispatch includes a signed immutable scope/budget/task graph; remote runner cannot request broader filesystem/network privileges itself.",
  "Remote task protocol",
  575
 ],
 [
  "SPEC-16-011",
  "Stream progress/receipts with reconnect and backpressure; retain bounded durable logs; never stream secrets in progress events.",
  "Remote task protocol",
  576
 ],
 [
  "SPEC-16-012",
  "Runner heartbeats and leases, cancellation and job timeout. Do not re-execute a destructive side-effect on reconnect.",
  "Remote task protocol",
  577
 ],
 [
  "SPEC-16-013",
  "Runner receives **only specifically authorized task inputs**, not the entire workspace by default. Secrets and project files remain local unless necessary and acknowledged.",
  "Remote task protocol",
  578
 ],
 [
  "SPEC-16-014",
  "Optional uploaded bundles are encrypted, size-limited, scanned for path traversal and stored in per-task isolated directories.",
  "Remote task protocol",
  579
 ],
 [
  "SPEC-16-015",
  "Maintain compatibility negotiation between desktop/mobile apps and runner; support safe updates/rollbacks and minimum secure protocol versions.",
  "Remote task protocol",
  580
 ],
 [
  "SPEC-16-016",
  "Offline local queue should survive device restart and display \"waiting for runner\" accurately.",
  "Remote task protocol",
  581
 ],
 [
  "SPEC-16-017",
  "Direct LAN discovery where allowed.",
  "Connectivity modes",
  589
 ],
 [
  "SPEC-16-018",
  "SSH or user-configured VPN/mesh connection, preferred for personal server reliability.",
  "Connectivity modes",
  590
 ],
 [
  "SPEC-16-019",
  "Optional BYO relay service only if user explicitly configures a provider; do not imply free global inbound NAT traversal.",
  "Connectivity modes",
  591
 ],
 [
  "SPEC-16-020",
  "Manual address/URL for environments with static network configuration.",
  "Connectivity modes",
  592
 ],
 [
  "SPEC-17-001",
  "**No sync:** fully functional default; local device-only mode.",
  "Sync choices",
  606
 ],
 [
  "SPEC-17-002",
  "**Local encrypted export/import** to file or external drive; simple transfer and safe restore.",
  "Sync choices",
  607
 ],
 [
  "SPEC-17-003",
  "**Local folder:** compatible with user-controlled synced folders and tools such as Syncthing (do not promise automatically conflict-free raw SQLite replication).",
  "Sync choices",
  608
 ],
 [
  "SPEC-17-004",
  "Bring-your-own S3-compatible object storage: initial documented recipe for **Cloudflare R2** with user-controlled bucket/account; provider-neutral endpoint/credentials so users can choose other *verified* S3 services.",
  "Sync choices",
  609
 ],
 [
  "SPEC-17-005",
  "**WebDAV/Nextcloud** optional second transport if reliable conditional writes/versioning are verified.",
  "Sync choices",
  610
 ],
 [
  "SPEC-17-006",
  "**Peer-to-peer sync** optional later: LAN/manual pairing or Syncthing-managed encrypted objects. Avoid premature complex distributed networking.",
  "Sync choices",
  611
 ],
 [
  "SPEC-17-007",
  "Encrypt messages, configuration metadata that is appropriate to sync, agent role definitions, room histories, attachment manifests and artifact contents **before upload** with a versioned E2EE envelope.",
  "E2EE design",
  617
 ],
 [
  "SPEC-17-008",
  "Use an independently generated high-entropy workspace key, wrapped for authorized devices / passphrase via strong KDF. Support per-device keypair, pairing, key rotation, revocation, recovery phrase/file and clear warning about losing recovery key.",
  "E2EE design",
  618
 ],
 [
  "SPEC-17-009",
  "Use authenticated encryption with distinct nonces, salts and associated data binding workspace/device/revision/content type. Guard against replay, downgrade, rollback and partial downloads. Avoid leaking human-readable titles and model API credentials in object keys.",
  "E2EE design",
  619
 ],
 [
  "SPEC-17-010",
  "Integrate OS keychain/Android Keystore and app passcode/biometric unlock as appropriate; do not hard-code encryption secrets. No unencrypted key material in logs, source, crash dumps or Git.",
  "E2EE design",
  620
 ],
 [
  "SPEC-17-011",
  "**Do not sync remote model API keys by default.** Provide deliberately separate, opt-in secret transfer with device-bound encryption only if strong design and recovery are demonstrated; safest default is configuring provider keys on each device.",
  "E2EE design",
  621
 ],
 [
  "SPEC-17-012",
  "Encryption at rest on local device should be opt-in or automatic depending on secure key storage; document filesystem caches, OS backups, screenshots and platform limits honestly.",
  "E2EE design",
  622
 ],
 [
  "SPEC-17-013",
  "Never synchronize a live SQLite database file by direct folder replication. Sync **immutable encrypted events/artifact chunks** and a **compare-and-swap version pointer** or equivalent safe protocol, with per-device clocks/causal IDs.",
  "Correctness under multiple writers",
  626
 ],
 [
  "SPEC-17-014",
  "Model append-only operations with stable content IDs, schema versions, deterministic reconciliation for idempotent events, and user-visible conflict UI for messages/settings/role edits that cannot be safely merged.",
  "Correctness under multiple writers",
  627
 ],
 [
  "SPEC-17-015",
  "On 409/412 conditional write conflict, re-read authoritative revision, merge operations if safe, or surface conflict. No last-writer-wins for destructive state.",
  "Correctness under multiple writers",
  628
 ],
 [
  "SPEC-17-016",
  "Verify integrity before applying; store a transactionally consistent local snapshot plus immutable backup/recovery history.",
  "Correctness under multiple writers",
  629
 ],
 [
  "SPEC-17-017",
  "Persist a sync queue and last-known state across app crash/restart; distinguish upload pending, pulled, verified, applied and conflict.",
  "Correctness under multiple writers",
  630
 ],
 [
  "SPEC-17-018",
  "Explicitly allow choose-which-data-to-sync: rooms, roles, workflows, attachments, model profiles without secrets, task results; runner/system permissions remain device-local.",
  "Correctness under multiple writers",
  631
 ],
 [
  "SPEC-17-019",
  "Treat paired remote execution secrets/paths as **not portable** across machines unless reapproved.",
  "Correctness under multiple writers",
  632
 ],
 [
  "SPEC-17-020",
  "Enforce quota, storage cost alerts, bandwidth limits, metered-network options, retention and optional attachment exclusion.",
  "Correctness under multiple writers",
  633
 ],
 [
  "SPEC-18-001",
  "Complete local rooms/DMs/group chat; role catalog and profiles; multiple AI providers; custom HTTP/OpenAI-compatible endpoints where network policies permit; local chat/search/history.",
  "Android must support",
  653
 ],
 [
  "SPEC-18-002",
  "Add/edit agents, rooms and workflows; trigger or delegate existing tasks; inspect task logs, receipts, artifacts and model costs.",
  "Android must support",
  654
 ],
 [
  "SPEC-18-003",
  "Native document picker and share intents; images/photos/files/audio input with explicit permission; attach from device-selected URIs.",
  "Android must support",
  655
 ],
 [
  "SPEC-18-004",
  "Android-safe document/text transformations; on-device app-private persistence, safe export, voice-to-text if a user-selected provider or permitted OS API is available.",
  "Android must support",
  656
 ],
 [
  "SPEC-18-005",
  "Connect to user's local/remote AI endpoint; test reachability and make LAN access assumptions clear.",
  "Android must support",
  657
 ],
 [
  "SPEC-18-006",
  "Scan QR/pairing code and manage trusted desktop/VM runners; approve, pause, cancel, schedule and monitor tasks running remotely.",
  "Android must support",
  658
 ],
 [
  "SPEC-18-007",
  "App-level biometric/passcode lock if supported, privacy screen for sensitive content, share sheets, notification channels with opt-in notification permission.",
  "Android must support",
  659
 ],
 [
  "SPEC-18-008",
  "Offline read/drafts/queued actions, local search, optional encrypted sync, resilient reconnect; clear stale task indicators.",
  "Android must support",
  660
 ],
 [
  "SPEC-18-009",
  "Smooth responsive portrait/landscape layout, adaptive tablet view, gesture and screen reader accessibility, large text/dynamic fonts, reduced motion.",
  "Android must support",
  661
 ],
 [
  "SPEC-18-010",
  "Android Keystore for secrets; TLS pinning only where safely configured, with sane rotation and diagnostics.",
  "Android must support",
  662
 ],
 [
  "SPEC-18-011",
  "Real mobile battery, memory and network budget handling, proper back navigation, deep links and predictable task cancellation.",
  "Android must support",
  663
 ],
 [
  "SPEC-18-012",
  "Tauri's shell plugin **does not offer arbitrary spawn/execute on Android**; do not attempt to bypass this with unrelated hidden code. A connected desktop/server runner offers full coding/terminal tools under its own permission boundary.",
  "Android differences to explain",
  667
 ],
 [
  "SPEC-18-013",
  "Mobile background scheduling and network access are subject to Android OS rules (including Android 15/16 foreground service quotas and WorkManager restrictions); **no claim of guaranteed 24/7 phone-resident tasks**.",
  "Android differences to explain",
  668
 ],
 [
  "SPEC-18-014",
  "No unrestricted cross-app UI control without OS-granted special accessibility/automation permissions and a legitimate clearly visible purpose; avoid invasive surveillance functionality.",
  "Android differences to explain",
  669
 ],
 [
  "SPEC-18-015",
  "On-device AI inference is optional and hardware-dependent; the default APK contains **no prebundled multi-gigabyte model weights**.",
  "Android differences to explain",
  670
 ],
 [
  "SPEC-18-016",
  "Device file access uses Android's storage framework and explicit selected URIs, not desktop path assumptions.",
  "Android differences to explain",
  671
 ],
 [
  "SPEC-20-001",
  "**Ask a Team** composer: human asks once, intelligent routing selects 1–3 relevant roles with a visible option to override.",
  "Extra original product features to differentiate the app",
  727
 ],
 [
  "SPEC-20-002",
  "Agent Cards: 10-second glance at what an agent can do, which model it uses, what it can access, and what it usually costs.",
  "Extra original product features to differentiate the app",
  728
 ],
 [
  "SPEC-20-003",
  "Team Starter Kits: \"Build an app,\" \"Research a topic,\" \"Organize my files,\" \"Plan my week,\" \"Write and review an article,\" \"Study with me.\"",
  "Extra original product features to differentiate the app",
  729
 ],
 [
  "SPEC-20-004",
  "Trust Lens: one click reveals what data each action will send off-device and how it will be used by selected provider.",
  "Extra original product features to differentiate the app",
  730
 ],
 [
  "SPEC-20-005",
  "Proof of Work: completed work includes test/effect receipts, changed files, artifact previews, references and unresolved risks.",
  "Extra original product features to differentiate the app",
  731
 ],
 [
  "SPEC-20-006",
  "Task Playback: compact visual history of model messages, approvals, tool outputs, artifacts, and retries; replay safely without triggering side effects.",
  "Extra original product features to differentiate the app",
  732
 ],
 [
  "SPEC-20-007",
  "Budget Dial: user picks \"quick / balanced / thorough\" and sees likely model cost and expected quality trade-offs without fake price certainty.",
  "Extra original product features to differentiate the app",
  733
 ],
 [
  "SPEC-20-008",
  "Compare Models: send same prompt with same authorized inputs to selected models, compare outputs side-by-side and choose one for follow-up.",
  "Extra original product features to differentiate the app",
  734
 ],
 [
  "SPEC-20-009",
  "Local-Only Switch: prevents remote model and connector egress for the room, with network enforcement and clear unsupported-tool warnings.",
  "Extra original product features to differentiate the app",
  735
 ],
 [
  "SPEC-20-010",
  "Privacy Receipt: audit summary of domains/services contacted, category of data shared and which machine executed work.",
  "Extra original product features to differentiate the app",
  736
 ],
 [
  "SPEC-20-011",
  "Smart Handoff: user can send an incomplete agent draft to a reviewer or a more capable model, with explicit context and artifact permissions.",
  "Extra original product features to differentiate the app",
  737
 ],
 [
  "SPEC-20-012",
  "Recoverable Actions: optional pre-change snapshots for local file operations and a clear undo history with limitations.",
  "Extra original product features to differentiate the app",
  738
 ],
 [
  "SPEC-20-013",
  "Focus Mode: one room, no sidebars, quiet notifications, compact agent status and full keyboard support.",
  "Extra original product features to differentiate the app",
  739
 ],
 [
  "SPEC-20-014",
  "Small-Team Debate: structured question, evidence gathering, conflicting answers, neutral synthesis and uncertainty note.",
  "Extra original product features to differentiate the app",
  740
 ],
 [
  "SPEC-20-015",
  "Role Recipes: save a successful sequence of role assignments and tools as an understandable reusable workflow.",
  "Extra original product features to differentiate the app",
  741
 ],
 [
  "SPEC-20-016",
  "Explain the Plan: toggle between a one-paragraph summary and detailed DAG/Gantt-style expert view.",
  "Extra original product features to differentiate the app",
  742
 ],
 [
  "SPEC-20-017",
  "Machine Passport: each runner shows OS, capabilities, version, last verified activity, trust key and allowed tasks.",
  "Extra original product features to differentiate the app",
  743
 ],
 [
  "SPEC-20-018",
  "Connection Doctor: tests model, MCP, storage and runner configuration and suggests exact fixes without exposing secrets.",
  "Extra original product features to differentiate the app",
  744
 ],
 [
  "SPEC-20-019",
  "Demo without billing: truthful interactive sandbox with deterministic fixture responses and explicit simulation watermark.",
  "Extra original product features to differentiate the app",
  745
 ],
 [
  "SPEC-20-020",
  "Learning by doing: two-minute contextual onboarding with one practical task per skill, no tutorial wall.",
  "Extra original product features to differentiate the app",
  746
 ],
 [
  "SPEC-20-021",
  "Answer Calibration: show \"source-backed,\" \"tool-verified,\" \"model suggestion,\" and \"unverified\" states in unobtrusive detail view.",
  "Extra original product features to differentiate the app",
  747
 ],
 [
  "SPEC-20-022",
  "Adaptive Team Size: suggest only as many agents as necessary, avoiding expensive agent swarms on basic questions.",
  "Extra original product features to differentiate the app",
  748
 ],
 [
  "SPEC-20-023",
  "Artifact Inbox: completed deliverables searchable independently of raw conversations.",
  "Extra original product features to differentiate the app",
  749
 ],
 [
  "SPEC-20-024",
  "Change Impact Preview: files affected, disk delta, network destinations and estimated tool usage before approval.",
  "Extra original product features to differentiate the app",
  750
 ],
 [
  "SPEC-20-025",
  "Environment Checker: detect missing build tools/browser/local model server and offer instructions; no automatic system installs.",
  "Extra original product features to differentiate the app",
  751
 ],
 [
  "SPEC-20-026",
  "Workspace Templates: persist role configurations and room layouts locally, export encrypted portable pack.",
  "Extra original product features to differentiate the app",
  752
 ],
 [
  "SPEC-20-027",
  "Cross-device Handoff: start chat on Android, continue task on paired desktop, inspect result back on Android with correct provenance.",
  "Extra original product features to differentiate the app",
  753
 ],
 [
  "SPEC-20-028",
  "Quick Capture: desktop clipboard/drag drop/selected file → one-click summarize/organize with approval; Android share sheet equivalent.",
  "Extra original product features to differentiate the app",
  754
 ],
 [
  "SPEC-20-029",
  "Knowledge Freshness: source-change indicators and manual reindex to avoid stale citations.",
  "Extra original product features to differentiate the app",
  755
 ],
 [
  "SPEC-20-030",
  "Sensible Notifications: approval needed, task done, task failed, cost ceiling, machine disconnected, sync conflict — not chatty self-promotion.",
  "Extra original product features to differentiate the app",
  756
 ],
 [
  "SPEC-20-031",
  "Incident Recovery Mode: after unexpected shutdown show task states, incomplete operations, backups and safe next actions.",
  "Extra original product features to differentiate the app",
  757
 ],
 [
  "SPEC-20-032",
  "Model Capability Lab: user runs harmless structured tests to see how well a model handles tools, structured output, multilingual text and retrieval.",
  "Extra original product features to differentiate the app",
  758
 ],
 [
  "SPEC-20-033",
  "Agent Evaluation Bench: test selected roles/workflows on fixture tasks with quality, latency, cost, error and unsafe-tool metrics.",
  "Extra original product features to differentiate the app",
  759
 ],
 [
  "SPEC-20-034",
  "Workflow Dry Run: inspect steps and permissions without contacting external services, then explicitly activate.",
  "Extra original product features to differentiate the app",
  760
 ],
 [
  "SPEC-20-035",
  "Energy / Resource Awareness: on local inference, show memory requirements and battery impact warning; no fake GPU estimations.",
  "Extra original product features to differentiate the app",
  761
 ],
 [
  "SPEC-20-036",
  "Offline First-Run Data: ship a few tiny permitted example documents so new users can explore safely without real private data.",
  "Extra original product features to differentiate the app",
  762
 ],
 [
  "SPEC-20-037",
  "Import from Other Agents: parse documented role/skill/workflow formats, with review of prompts and tools before enabling.",
  "Extra original product features to differentiate the app",
  763
 ],
 [
  "SPEC-20-038",
  "Localized Assistance: support English first and prepare full localization including Persian RTL, Arabic and other languages; no mixed-direction UI bugs.",
  "Extra original product features to differentiate the app",
  764
 ],
 [
  "SPEC-20-039",
  "Role Safety Profiles: \"Advisory only\" for regulated domains; verified user consent before any external publication/action.",
  "Extra original product features to differentiate the app",
  765
 ],
 [
  "SPEC-20-040",
  "An Inspectable Product: readable architecture and exported data so users know exactly what is local and what is remote.",
  "Extra original product features to differentiate the app",
  766
 ],
 [
  "SPEC-24-001",
  "`rust-format-lint`: `cargo fmt --all --check`, clippy workspace/all relevant features with `-D warnings`, feature-gate matrix.",
  "Minimum CI jobs",
  906
 ],
 [
  "SPEC-24-002",
  "`unit-and-integration`: `cargo test --workspace` across supported targets and appropriate conditional integration fixtures.",
  "Minimum CI jobs",
  907
 ],
 [
  "SPEC-24-003",
  "`frontend-check`: typecheck, lint, component tests, bundle and accessibility smoke if a webview presentation layer is selected.",
  "Minimum CI jobs",
  908
 ],
 [
  "SPEC-24-004",
  "`security-and-supply-chain`: secret scan, vulnerability audit, license report, dependency manifest/SBOM, forbidden binary/asset rules.",
  "Minimum CI jobs",
  909
 ],
 [
  "SPEC-24-005",
  "`schema-role-validation`: all 200 role templates and skill/tool manifest schema/capability compatibility.",
  "Minimum CI jobs",
  910
 ],
 [
  "SPEC-24-006",
  "`skills-200-audit`: exact-count inventory, real evidence links, duplicate detection, recorded blockers.",
  "Minimum CI jobs",
  911
 ],
 [
  "SPEC-24-007",
  "`provider-contract-tests`: deterministic model protocol test server with streaming, tool calls, errors, fallback and cancellation.",
  "Minimum CI jobs",
  912
 ],
 [
  "SPEC-24-008",
  "`mcp-acp-contract-tests`: supported protocol semantics, revocation, malicious response and transport failure.",
  "Minimum CI jobs",
  913
 ],
 [
  "SPEC-24-009",
  "`sync-tests`: encryption, conflict/transactional recovery, S3 CAS fixture and provider capability detection.",
  "Minimum CI jobs",
  914
 ],
 [
  "SPEC-24-010",
  "`runner-tests`: Linux install/start/pair/dispatch/reconnect/revoke/restart and version negotiation.",
  "Minimum CI jobs",
  915
 ],
 [
  "SPEC-24-011",
  "`desktop-windows`: compile/bundle NSIS `*-setup.exe` and MSI if viable; install to clean test user; launch, UIA smoke, files/permissions.",
  "Minimum CI jobs",
  916
 ],
 [
  "SPEC-24-012",
  "`desktop-macos-arm`: macOS Apple Silicon build/test/DMG; separately macOS x86_64 build/test, including native GUI smoke where runners available.",
  "Minimum CI jobs",
  917
 ],
 [
  "SPEC-24-013",
  "`desktop-linux`: appimage and `.deb` for supported x86_64; launch in appropriate virtual display, GUI smoke; report system dependency set. Consider aarch64 only where reliably built.",
  "Minimum CI jobs",
  918
 ],
 [
  "SPEC-24-014",
  "`android-build`: reproducible APK/AAB build, ABI inspection, signing verification, emulator install/launch/chat/task smoke, ADB logs.",
  "Minimum CI jobs",
  919
 ],
 [
  "SPEC-24-015",
  "`release-assets`: hash/size, filenames/version, executable inspection, archive content manifest, signature verification, `sha256sum` file, dry-run release notes.",
  "Minimum CI jobs",
  920
 ],
 [
  "SPEC-24-016",
  "`docs-links`: README assets present, no dead internal links, screenshots match current UI, install instructions match actual outputs.",
  "Minimum CI jobs",
  921
 ],
 [
  "SPEC-24-017",
  "`repo-size`: Git pack size, single tracked blob size, vendor/model download exclusion and LFS policy check.",
  "Minimum CI jobs",
  922
 ],
 [
  "SPEC-24-018",
  "Develop on a feature branch, maintain small inspectable commits, use PRs and protected checks where available. Never force-push unrelated work or clobber existing branches/worktrees.",
  "Build matrix and branch policies",
  926
 ],
 [
  "SPEC-24-019",
  "Use OS-native runners for packaging whenever possible. A successful cross-compile is not equivalent to native GUI verification.",
  "Build matrix and branch policies",
  927
 ],
 [
  "SPEC-24-020",
  "Distinguish `compiled`, `unit tested`, `integration tested`, `installed and launched`, `UI flow tested`, `signed`, `notarized`, `released` in every report.",
  "Build matrix and branch policies",
  928
 ],
 [
  "SPEC-24-021",
  "Do not leave release workflows broken or disabled just to obtain a green status.",
  "Build matrix and branch policies",
  929
 ],
 [
  "SPEC-24-022",
  "Integrate dependency update policy, vulnerability handling, release rollback, SBOM generation and source-to-asset traceability.",
  "Build matrix and branch policies",
  930
 ],
 [
  "SPEC-24-023",
  "Store signing credentials, API test keys and Android keystore outside Git and inject through appropriate protected CI secrets.",
  "Build matrix and branch policies",
  931
 ],
 [
  "SPEC-24-024",
  "Do not require the end user to install Rust/Node/Android Studio to run a packaged release.",
  "Build matrix and branch policies",
  932
 ],
 [
  "SPEC-22-001",
  "Cold launch to interactive chat: target **under 3 seconds** on a midrange recent desktop, excluding network/provider setup.",
  "Product performance, reliability and observability budgets",
  820
 ],
 [
  "SPEC-22-002",
  "Opening an existing room with 10,000 messages should not freeze the UI or materialize the entire chat DOM. Virtualized scrolling, incremental search and stable scroll anchoring required.",
  "Product performance, reliability and observability budgets",
  821
 ],
 [
  "SPEC-22-003",
  "Task state, cancel button and approval UI must remain responsive while multiple streaming agents and local subprocesses run.",
  "Product performance, reliability and observability budgets",
  822
 ],
 [
  "SPEC-22-004",
  "Typical typing/input interaction should be smooth with no synchronous disk/network work on the UI thread; aim for 60 fps where hardware allows.",
  "Product performance, reliability and observability budgets",
  823
 ],
 [
  "SPEC-22-005",
  "Idle RAM and CPU must be benchmarked and tracked by OS; derive realistic thresholds after real installed measurements; investigate regressions greater than **20%** on matched hardware.",
  "Product performance, reliability and observability budgets",
  824
 ],
 [
  "SPEC-22-006",
  "No unbounded log buffers, task queues, subprocess output capture, embedding buffers or attachment previews.",
  "Product performance, reliability and observability budgets",
  825
 ],
 [
  "SPEC-22-007",
  "UI/agent engine separation must isolate model timeouts/crashes from renderer responsiveness.",
  "Product performance, reliability and observability budgets",
  826
 ],
 [
  "SPEC-22-008",
  "SQLite uses migrations with integrity checks, pragmas, transactions and bounded DB reads; include real disk-full and interrupted-migration tests.",
  "Product performance, reliability and observability budgets",
  827
 ],
 [
  "SPEC-22-009",
  "Resilient streaming with reconnect and backpressure; emit at most a sensible display update rate to avoid rerender storms.",
  "Product performance, reliability and observability budgets",
  828
 ],
 [
  "SPEC-22-010",
  "Cancellable work: provider request, browser action, shell process, retrieval, model warmup, sync upload and remote-runner command; quantify non-cancellable cases.",
  "Product performance, reliability and observability budgets",
  829
 ],
 [
  "SPEC-22-011",
  "Persist drafts, open room selection and task checkpoints across crashes; never falsely replay a destructive external action.",
  "Product performance, reliability and observability budgets",
  830
 ],
 [
  "SPEC-22-012",
  "Instruments: local structured tracing with redaction, task ID correlation, provider latency/tokens/cost when available, error categories and exportable diagnostic bundle.",
  "Product performance, reliability and observability budgets",
  831
 ],
 [
  "SPEC-21-001",
  "Web content/document/repository prompt injection into tool calls and role policies.",
  "Required threat model artifacts",
  778
 ],
 [
  "SPEC-21-002",
  "Direct tool-output spoofing, `README.md` instructions that request secrets, malicious MCP tool descriptions, model/provider impersonation.",
  "Required threat model artifacts",
  779
 ],
 [
  "SPEC-21-003",
  "An agent asking a second agent for a prohibited operation; permissions must not be laundered through delegation.",
  "Required threat model artifacts",
  780
 ],
 [
  "SPEC-21-004",
  "Wrong execution-target selection or stale approval when a file, workspace, task plan or runner changed.",
  "Required threat model artifacts",
  781
 ],
 [
  "SPEC-21-005",
  "Shell command injection, glob/regex abuse, path traversal, symlink escapes, `..` and Windows reparse points.",
  "Required threat model artifacts",
  782
 ],
 [
  "SPEC-21-006",
  "Sensitive file/credential disclosure by tool logs, error traces, chat transcripts, screenshots, provider requests or sync metadata.",
  "Required threat model artifacts",
  783
 ],
 [
  "SPEC-21-007",
  "SSRF and redirection to cloud instance metadata, localhost/private networks or arbitrary file URLs.",
  "Required threat model artifacts",
  784
 ],
 [
  "SPEC-21-008",
  "Third-party MCP package supply chain, unauthorized process spawning, tool impersonation, dependency compromise.",
  "Required threat model artifacts",
  785
 ],
 [
  "SPEC-21-009",
  "Remote-runner pairing replay, compromised pairing code, stolen token, stale trust key, MITM, revoked device and downgrade.",
  "Required threat model artifacts",
  786
 ],
 [
  "SPEC-21-010",
  "Sync overwrites, corruption, transaction races, rollback, compromised cloud storage, lost recovery key and malicious object content.",
  "Required threat model artifacts",
  787
 ],
 [
  "SPEC-21-011",
  "Android exported activity/content provider mistakes, cleartext network configurations and insecure deep links.",
  "Required threat model artifacts",
  788
 ],
 [
  "SPEC-21-012",
  "Malicious imported role/skill/workflow packages with instructions that attempt broad tool grants.",
  "Required threat model artifacts",
  789
 ],
 [
  "SPEC-21-013",
  "UI confused deputy: an \"Approve\" button referring to a different changed command or project.",
  "Required threat model artifacts",
  790
 ],
 [
  "SPEC-21-014",
  "Denial-of-service from recursive agent cycles, huge file chunks, enormous logs, giant tool results, repeated schedules or cost explosion.",
  "Required threat model artifacts",
  791
 ],
 [
  "SPEC-21-015",
  "A **single central policy engine** is the authoritative evaluator for all tools, across UI, CLI, MCP, workflow, direct APIs and paired runner. Avoid duplicated permission logic in frontends.",
  "Controls",
  795
 ],
 [
  "SPEC-21-016",
  "Treat all model-generated text as untrusted suggestions, validate tool call names/JSON args against schemas and enforce exact scope at execution.",
  "Controls",
  796
 ],
 [
  "SPEC-21-017",
  "Scope approvals to hashed action arguments and task/version/executor. Old approval tokens become invalid when context changes.",
  "Controls",
  797
 ],
 [
  "SPEC-21-018",
  "Distinguish read-only claims from actual tool operations; an \"informational\" action cannot cause an external write.",
  "Controls",
  798
 ],
 [
  "SPEC-21-019",
  "Maintain a consent receipt for remote egress, high-risk tooling and remote execution.",
  "Controls",
  799
 ],
 [
  "SPEC-21-020",
  "Restrict webview capabilities, IPC, content-security policy, external URLs and loading remote scripts. Do not give UI generic shell access; Tauri's fine-grained capabilities must be configured per platform.",
  "Controls",
  800
 ],
 [
  "SPEC-21-021",
  "Handle filenames/paths through structured APIs, not shell interpolation; canonicalize paths carefully without TOCTOU assumptions.",
  "Controls",
  801
 ],
 [
  "SPEC-21-022",
  "Provide a secure update/release verification model with hashes/signatures and explicit rollback.",
  "Controls",
  802
 ],
 [
  "SPEC-21-023",
  "Rate-limit model spending and agent recursion; per-agent and global cost/time/token budget, fail closed at limits.",
  "Controls",
  803
 ],
 [
  "SPEC-21-024",
  "Fail closed when encryption, trust verification or version negotiation fails. No secret-bearing fallback from HTTPS to HTTP (localhost test exceptions must be user-configured and visually labeled).",
  "Controls",
  804
 ],
 [
  "SPEC-21-025",
  "Software supply-chain controls: dependency lockfiles, `cargo audit` or equivalent vetted advisories, SBOM, license inventory, provenance, secret scan, Git history scan and reproducible source review.",
  "Controls",
  805
 ],
 [
  "SPEC-21-026",
  "Default local telemetry OFF; diagnostic export opt-in, redacted, with clear preview. No hidden central analytics or crash upload.",
  "Controls",
  806
 ],
 [
  "SPEC-23-001",
  "Build a runnable desktop application or Android installable development artifact.",
  "Literal interaction protocol",
  843
 ],
 [
  "SPEC-23-002",
  "**Launch the actual app** (prefer an isolated test profile and never overwrite real user data).",
  "Literal interaction protocol",
  844
 ],
 [
  "SPEC-23-003",
  "Navigate by mouse/touch/keyboard or reliable UI automation; verify actual labels, focus, interaction and persisted state.",
  "Literal interaction protocol",
  845
 ],
 [
  "SPEC-23-004",
  "Perform the user's full workflow, including bad inputs, network failure, cancellation and restart.",
  "Literal interaction protocol",
  846
 ],
 [
  "SPEC-23-005",
  "Inspect screenshots and logs for clipping, overlaps, tiny click targets, visual contrast, blank states, misleading statuses, unresponsive buttons or stale data.",
  "Literal interaction protocol",
  847
 ],
 [
  "SPEC-23-006",
  "File a reproducible issue with OS/build/steps/expected/observed, fix code, rerun the literal GUI scenario, and record evidence.",
  "Literal interaction protocol",
  848
 ],
 [
  "SPEC-23-007",
  "Preserve truthful screenshot/video/automation logs in `docs/qa/evidence/` where privacy and repo size permit; compress assets or attach full videos to CI artifacts/release notes instead of huge Git blobs.",
  "Literal interaction protocol",
  849
 ],
 [
  "SPEC-23-008",
  "Repeat on installed **release bundles**, not just `cargo run` or a dev server. Installer, installation directory, update/uninstall and restart may behave differently.",
  "Literal interaction protocol",
  850
 ],
 [
  "SPEC-23-009",
  "Unit: role schema, model capabilities, permissions, task transitions, budgets, idempotence, cryptography primitives, path validation, provider parser.",
  "Test categories",
  877
 ],
 [
  "SPEC-23-010",
  "Integration: local SQLite/restart, mock HTTP model, MCP stdio and HTTP, credential store abstraction, job scheduler, live filesystem sandbox, remote runner handshake, S3 fixture sync.",
  "Test categories",
  878
 ],
 [
  "SPEC-23-011",
  "End-to-end: fresh install, model setup, chat, group delegation, real task, review/approve, artifact export, crash recovery.",
  "Test categories",
  879
 ],
 [
  "SPEC-23-012",
  "Contract: all provider adapters and MCP capability schemas, ACP interop if claimed.",
  "Test categories",
  880
 ],
 [
  "SPEC-23-013",
  "Security: prompt injection, wrong approvals, egress, filesystem boundaries, malicious skill, runner revocation, sync tampering.",
  "Test categories",
  881
 ],
 [
  "SPEC-23-014",
  "Accessibility: axe or appropriate automated scanner plus manual/keyboard and screen reader checks; WCAG 2.2 AA evidence.",
  "Test categories",
  882
 ],
 [
  "SPEC-23-015",
  "Property/fuzz: path parsing, stream handling, JSON schema, sync conflict operations, archive import and cryptographic envelopes.",
  "Test categories",
  883
 ],
 [
  "SPEC-23-016",
  "Chaos: intermittent internet, interrupted file write, DB lock, out of disk, process death, missing key, revoked API key, task cancellation and duplicate event.",
  "Test categories",
  884
 ],
 [
  "SPEC-23-017",
  "Load: 100, 1,000 and 10,000 message rooms; 10/50 concurrent queued agents; bounded concurrent active tasks; huge file attachment; archive bombs rejected.",
  "Test categories",
  885
 ],
 [
  "SPEC-23-018",
  "Performance: cold/warm start, idle, long scroll, update storms, memory/resource profile, runner latency and battery/network use on mobile.",
  "Test categories",
  886
 ],
 [
  "SPEC-23-019",
  "Packaging: installed binaries, missing system deps, long paths/Unicode, OS permissions, upgrade/uninstall, app data preservation, clean install.",
  "Test categories",
  887
 ],
 [
  "SPEC-23-020",
  "Offline: no user account, no provider, local model available, local-only egress-denial test, queued offline synchronization.",
  "Test categories",
  888
 ],
 [
  "SPEC-25-001",
  "**Android:** release APK must use a stable, owner-controlled signing key, securely backed up. Never publish a debug-signed APK as a release or commit `.jks`/passwords. Verify `apksigner` certificate fingerprint and install/upgrade from preceding release where applicable.",
  "Platform signing realities",
  985
 ],
 [
  "SPEC-25-002",
  "**macOS:** fully frictionless public distribution ordinarily requires Apple Developer signing and notarization; a free development account is insufficient for notarization. Build/test a valid DMG regardless, and if credentials are unavailable **label unsigned/ad-hoc/not-notarized status and user-facing Gatekeeper instructions honestly**. Do not claim notarized without Apple's result.",
  "Platform signing realities",
  986
 ],
 [
  "SPEC-25-003",
  "**Windows:** compile a valid NSIS installer and test installation. Authenticode signing is separate and may require certificates/fees; unsigned installer may trigger SmartScreen. Be clear in release notes.",
  "Platform signing realities",
  987
 ],
 [
  "SPEC-25-004",
  "**Linux:** verify desktop file, icon, correct permissions, update/uninstall mechanics and runtime dependencies across clean supported distro images.",
  "Platform signing realities",
  988
 ],
 [
  "SPEC-25-005",
  "**Supply chain:** checksums, source commit/tag, license manifests, release asset inspection, no malware alerts ignored or \"false positive\" assumed.",
  "Platform signing realities",
  989
 ],
 [
  "SPEC-25-006",
  "Choose publicly distinct verified brand and repository name; check existing repositories, copyright, licenses and trademark risks.",
  "GitHub repository workflow",
  995
 ],
 [
  "SPEC-25-007",
  "Initialize Git in appropriate project folder, `.gitignore`, license (choose compatible with all dependencies), security policy and contributor files. Do not reuse existing repo destructively.",
  "GitHub repository workflow",
  996
 ],
 [
  "SPEC-25-008",
  "Build in controlled branches with commit messages and changelog. Set up matrix workflows, code owners/reviews where possible.",
  "GitHub repository workflow",
  997
 ],
 [
  "SPEC-25-009",
  "Before any public push: run secret scan on working tree **and full Git history**; inspect licenses/fonts/icons/screenshots and test fixtures for privacy; remove local `.env`, test credentials, sample patient data and giant files.",
  "GitHub repository workflow",
  998
 ],
 [
  "SPEC-25-010",
  "Verify `git count-objects -vH`, `git rev-list --objects --all` + pack size and maximum blob sizes; no tracked archives/large downloads/model weights.",
  "GitHub repository workflow",
  999
 ],
 [
  "SPEC-25-011",
  "Push after actual QA evidence exists; create reviewable PRs when appropriate; merge only after required checks are green and relevant installed UI smoke has passed.",
  "GitHub repository workflow",
  1000
 ],
 [
  "SPEC-25-012",
  "Tag a semantic version matching installers, changelog and app About page; run native CI release workflows and verify every expected asset is present with correct architecture and hashes.",
  "GitHub repository workflow",
  1001
 ],
 [
  "SPEC-25-013",
  "Publish a real public Release with overview, visual screenshots, one-click download links, install/first-run steps, security caveats, known limitations and source/hash details.",
  "GitHub repository workflow",
  1002
 ],
 [
  "SPEC-25-014",
  "Download/install release artifacts **from the GitHub Release itself** on clean test environment where possible and confirm start, provider config, group chat, task approval and stop, and exported data.",
  "GitHub repository workflow",
  1003
 ],
 [
  "SPEC-25-015",
  "Verify the public repo/Release URLs actually resolve and link back to them in README; collect published tag, commit SHA, workflow run URLs and checksums.",
  "GitHub repository workflow",
  1004
 ],
 [
  "SPEC-30-001",
  "Custom original brand and app icons across all installers/platforms; brand search documented.",
  "Product and UX",
  1141
 ],
 [
  "SPEC-30-002",
  "Beautiful minimal first-run, group chat, task UI, role catalogue and expert settings.",
  "Product and UX",
  1142
 ],
 [
  "SPEC-30-003",
  "Beginner journey completes without jargon walls and no mandatory account.",
  "Product and UX",
  1143
 ],
 [
  "SPEC-30-004",
  "Light/dark, responsive phone/tablet, keyboard/screen reader, reduced motion, localization architecture.",
  "Product and UX",
  1144
 ],
 [
  "SPEC-30-005",
  "Literal installed UI walkthrough and screenshot review on tested target OSes.",
  "Product and UX",
  1145
 ],
 [
  "SPEC-30-006",
  "All visible primary controls have real backing logic and meaningful empty/error states.",
  "Product and UX",
  1146
 ],
 [
  "SPEC-30-007",
  "DMs, rooms, mentions, threads, roles, parallel/team delegation and real task engine.",
  "Multi-agent and providers",
  1150
 ],
 [
  "SPEC-30-008",
  "Arbitrary compatible custom model endpoint configuration and honest capability probes.",
  "Multi-agent and providers",
  1151
 ],
 [
  "SPEC-30-009",
  "Local model endpoint, supported external providers, streaming, fallback, costs and privacy notices.",
  "Multi-agent and providers",
  1152
 ],
 [
  "SPEC-30-010",
  "200 substantive, validated role templates; easy search and custom roles.",
  "Multi-agent and providers",
  1153
 ],
 [
  "SPEC-30-011",
  "Full safe coding toolbox with real tests, diffs, authorized process tool, Git and recoverability.",
  "Multi-agent and providers",
  1154
 ],
 [
  "SPEC-30-012",
  "MCP skill lifecycle and ACP interop **only where tested**; strict permissions and process sandbox limits.",
  "Multi-agent and providers",
  1155
 ],
 [
  "SPEC-30-013",
  "200 distinct installed/exercised vetted dev skills/MCPs with inventory, provenance and audit evidence.",
  "Multi-agent and providers",
  1156
 ],
 [
  "SPEC-30-014",
  "Local execution truly local by default; no project-owned cloud backend.",
  "Infrastructure and security",
  1160
 ],
 [
  "SPEC-30-015",
  "User-owned Linux server/VM runner tested including laptop-off continuation, monitor, revoke and cancel.",
  "Infrastructure and security",
  1161
 ],
 [
  "SPEC-30-016",
  "User-owned zero-operator-cost sync path with E2EE, CAS, safe conflicts and recovery.",
  "Infrastructure and security",
  1162
 ],
 [
  "SPEC-30-017",
  "Android local-safe operations, remote control, restrictions and reliable notifications/background behavior.",
  "Infrastructure and security",
  1163
 ],
 [
  "SPEC-30-018",
  "Prompt injection, file scope, SSRF, egress, secrets, pairing, sync corruption and trust tests.",
  "Infrastructure and security",
  1164
 ],
 [
  "SPEC-30-019",
  "Valid backups, migration rollback, audit receipts, cancellation, crash recovery and budget controls.",
  "Infrastructure and security",
  1165
 ],
 [
  "SPEC-30-020",
  "Git-tracked push/history under 500 MB with no illegal/secret/huge binary assets.",
  "Delivery",
  1169
 ],
 [
  "SPEC-30-021",
  "CI native Windows/macOS/Linux/Android builds with green relevant checks and known caveats recorded.",
  "Delivery",
  1170
 ],
 [
  "SPEC-30-022",
  "Actual installable Windows EXE, macOS ARM/Intel DMGs, Linux DEB/AppImage, Android release APK, runner download.",
  "Delivery",
  1171
 ],
 [
  "SPEC-30-023",
  "Stable Android signing key protected; macOS/Windows signing state represented honestly.",
  "Delivery",
  1172
 ],
 [
  "SPEC-30-024",
  "SHA256SUMS, source tag/commit, dependency license inventory and SBOM.",
  "Delivery",
  1173
 ],
 [
  "SPEC-30-025",
  "Real installed GUI smoke/negative flows on release artifacts where test hardware available.",
  "Delivery",
  1174
 ],
 [
  "SPEC-30-026",
  "Public GitHub source pushed, public Release verified, download links resolve.",
  "Delivery",
  1175
 ],
 [
  "SPEC-30-027",
  "Engaging visual README with **real** screenshots/demos, links, honest costs and privacy architecture.",
  "Delivery",
  1176
 ],
 [
  "SPEC-30-028",
  "No false `PASS`, no invented links, no hidden unfinished \"coming soon\" behind apparently enabled buttons.",
  "Delivery",
  1177
 ],
 [
  "SPEC-31-001",
  "Public GitHub repository URL and default branch/commit SHA, **only if created and verified**.",
  "Exact deliverables and final reporting format",
  1185
 ],
 [
  "SPEC-31-002",
  "Actual GitHub Release URL and tag, **only if published and verified**.",
  "Exact deliverables and final reporting format",
  1186
 ],
 [
  "SPEC-31-003",
  "Windows, macOS (both architectures), Linux, Android APK, runner artifact links and checksums, with signing status.",
  "Exact deliverables and final reporting format",
  1187
 ],
 [
  "SPEC-31-004",
  "Visual README/brand/design system sources and authentic app screenshot gallery.",
  "Exact deliverables and final reporting format",
  1188
 ],
 [
  "SPEC-31-005",
  "`docs/qa/acceptance-ledger.md` and platform-specific GUI test logs; list failures/unverified platforms.",
  "Exact deliverables and final reporting format",
  1189
 ],
 [
  "SPEC-31-006",
  "`docs/security/threat-model.md`, supply-chain audit/SBOM, sync/runner security verification.",
  "Exact deliverables and final reporting format",
  1190
 ],
 [
  "SPEC-31-007",
  "`skills-manifest/dev-tool-inventory.*` proving the 200-skill/MCP requirement (and distinguishing mocks from live exercise).",
  "Exact deliverables and final reporting format",
  1191
 ],
 [
  "SPEC-31-008",
  "200 role template generated index and validation results.",
  "Exact deliverables and final reporting format",
  1192
 ],
 [
  "SPEC-31-009",
  "Beginner install and quickstart instructions; provider setup, remote VM setup and sync instructions.",
  "Exact deliverables and final reporting format",
  1193
 ],
 [
  "SPEC-31-010",
  "Plain-language list of **what is genuinely completed**, **what is incomplete**, **what requires payment/credentials**, and **what was not practically tested**.",
  "Exact deliverables and final reporting format",
  1194
 ],
 [
  "SPEC-26-001",
  "**Hero:** centered custom logo, memorable name and concise one-line value proposition such as *\"Your AI team. Your machine. Your rules.\"* (create original final copy).",
  "Visual, engaging and beginner-friendly README.md contract",
  1012
 ],
 [
  "SPEC-26-002",
  "**Top-of-page download strip:** clear install links for Windows/macOS/Linux/Android; current release, release notes, verified platform matrix and a \"Start in 2 minutes\" guide. Use actual URLs after publication.",
  "Visual, engaging and beginner-friendly README.md contract",
  1013
 ],
 [
  "SPEC-26-003",
  "**Large authentic product screenshot** showing a calm group room and agents collaborating; light/dark variants or carousel-like side-by-side images in Markdown.",
  "Visual, engaging and beginner-friendly README.md contract",
  1014
 ],
 [
  "SPEC-26-004",
  "**Short real demo clip/GIF** optimized to less than a sensible size (e.g. 10–20 MB), showing create team → assign task → tool approval → verified artifact. No fake backend simulation presented as live.",
  "Visual, engaging and beginner-friendly README.md contract",
  1015
 ],
 [
  "SPEC-26-005",
  "**Three big ideas:** (a) chat with a team (b) work happens on your device (c) bring your own models and optional server.",
  "Visual, engaging and beginner-friendly README.md contract",
  1016
 ],
 [
  "SPEC-26-006",
  "**Use cases and examples:** beginner and power user, with real screenshots/GIFs of agent roles, workflow view, role marketplace, approvals and runner pairing.",
  "Visual, engaging and beginner-friendly README.md contract",
  1017
 ],
 [
  "SPEC-26-007",
  "**How it works:** clean architecture diagram, local/remote dataflow, provider vs execution-target distinction, what gets sent to external models.",
  "Visual, engaging and beginner-friendly README.md contract",
  1018
 ],
 [
  "SPEC-26-008",
  "**Platform comparison:** desktop vs Android feature matrix with real limitations.",
  "Visual, engaging and beginner-friendly README.md contract",
  1019
 ],
 [
  "SPEC-26-009",
  "**Quickstart:** no account, add provider, optionally connect Ollama/LM Studio, create room, run first task; screenshots and troubleshooting.",
  "Visual, engaging and beginner-friendly README.md contract",
  1020
 ],
 [
  "SPEC-26-010",
  "**Safety & privacy:** permission model, remote E2EE sync, sensitive data egress, runner trust, backup/recovery in plain language.",
  "Visual, engaging and beginner-friendly README.md contract",
  1021
 ],
 [
  "SPEC-26-011",
  "**Free/paid disclosure:** product and owner-hosted backend policy; user-paid API/storage/VM costs; no permanently free remote compute promise.",
  "Visual, engaging and beginner-friendly README.md contract",
  1022
 ],
 [
  "SPEC-26-012",
  "**Advanced features:** roles, custom models, MCP, ACP, workflow orchestration, scheduling, sync, runner and CLI links.",
  "Visual, engaging and beginner-friendly README.md contract",
  1023
 ],
 [
  "SPEC-26-013",
  "**Developer section:** source structure, build prerequisites, tests, CI status, contributor instructions.",
  "Visual, engaging and beginner-friendly README.md contract",
  1024
 ],
 [
  "SPEC-26-014",
  "**FAQ:** Is AI local? Do I need an API key? Does it control my computer? Can it run 24/7? How does Android differ? Do I pay you? Can I restore my data? How do I stop an agent?",
  "Visual, engaging and beginner-friendly README.md contract",
  1025
 ],
 [
  "SPEC-26-015",
  "**Roadmap and known limitations** with honest status, not inflated feature badges.",
  "Visual, engaging and beginner-friendly README.md contract",
  1026
 ],
 [
  "SPEC-26-016",
  "**Credits, license, security/contact and release verification**.",
  "Visual, engaging and beginner-friendly README.md contract",
  1027
 ],
 [
  "SPEC-27-001",
  "`docs/getting-started.md`: install, first provider, first room and first task, illustrated.",
  "Documentation and operator/user guides",
  1035
 ],
 [
  "SPEC-27-002",
  "`docs/using-roles.md`: roles/agents, model choice, room permissions, workflows.",
  "Documentation and operator/user guides",
  1036
 ],
 [
  "SPEC-27-003",
  "`docs/agents-and-tools.md`: tool execution model, examples, safe approvals and stop/undo.",
  "Documentation and operator/user guides",
  1037
 ],
 [
  "SPEC-27-004",
  "`docs/providers.md`: API keys/local model/custom endpoint, capabilities, troubleshooting, cost and egress.",
  "Documentation and operator/user guides",
  1038
 ],
 [
  "SPEC-27-005",
  "`docs/server-runner.md`: pair user-owned VPS/VM, SSH/VPN, systemd/Docker, updates, revocation, actual cost responsibilities.",
  "Documentation and operator/user guides",
  1039
 ],
 [
  "SPEC-27-006",
  "`docs/sync-and-backups.md`: user-owned R2/S3, device pairing, E2EE, recovery key, conflicts, storage quota and disaster recovery.",
  "Documentation and operator/user guides",
  1040
 ],
 [
  "SPEC-27-007",
  "`docs/android.md`: APK install, verified ABI/version, feature differences, background restrictions, share sheet and permissions.",
  "Documentation and operator/user guides",
  1041
 ],
 [
  "SPEC-27-008",
  "`docs/security/threat-model.md`, `SECURITY.md`, privacy policy statement for application behavior and external model privacy dependencies.",
  "Documentation and operator/user guides",
  1042
 ],
 [
  "SPEC-27-009",
  "`docs/architecture.md`, `docs/adr/` for framework/data/cryptography/runner/sync decisions.",
  "Documentation and operator/user guides",
  1043
 ],
 [
  "SPEC-27-010",
  "`docs/roles-catalog.md`: generated index of role templates and capabilities.",
  "Documentation and operator/user guides",
  1044
 ],
 [
  "SPEC-27-011",
  "`docs/design/`: screenshots, tokens, component catalogue, accessibility/UX test records.",
  "Documentation and operator/user guides",
  1045
 ],
 [
  "SPEC-27-012",
  "`docs/qa/`: acceptance ledger, failure reproductions and actual evidence.",
  "Documentation and operator/user guides",
  1046
 ],
 [
  "SPEC-27-013",
  "`docs/release/`: build matrix, signing limitations, provenance, public release checklist.",
  "Documentation and operator/user guides",
  1047
 ],
 [
  "SPEC-27-014",
  "`docs/roadmap.md`: implementable future work with priorities and no false \"implemented\" statuses.",
  "Documentation and operator/user guides",
  1048
 ],
 [
  "SPEC-27-015",
  "`AGENTS.md`: how any future coding agent should operate safely, how to run tests, ownership boundaries and architecture constraints.",
  "Documentation and operator/user guides",
  1049
 ]
];
const PUBLIC_TRACK_ROWS = [
 [
  "TRACK-P01",
  "Recover original master specification, architecture and scope",
  "IN PROGRESS",
  "**DONE (scope-qualified)**"
 ],
 [
  "TRACK-P02",
  "Build persistent local rooms, agents, messages and task ledger",
  "IN PROGRESS",
  "**DONE (scope-qualified, Windows source tests)**"
 ],
 [
  "TRACK-P11",
  "Real model capability probing, adapters, egress and spend budgets",
  "IN PROGRESS",
  "**IN PROGRESS**"
 ],
 [
  "TRACK-P12",
  "OS keyring, lifecycle, session credential hygiene",
  "IN PROGRESS",
  "**IN PROGRESS**"
 ],
 [
  "TRACK-P13",
  "Streaming, long-running jobs, retries and provider failover",
  "IN PROGRESS",
  "**IN PROGRESS (terminal framing hardened; native GUI pending)**"
 ],
 [
  "TRACK-P03",
  "Explicit provider selection and 1–8 agent room replies",
  "IN PROGRESS",
  "**IN PROGRESS**"
 ],
 [
  "TRACK-P04",
  "Plan, run, cancel and inspect text-only AI tasks",
  "IN PROGRESS",
  "**DONE (fixture only)**"
 ],
 [
  "TRACK-P05",
  "Bind task result and success atomically, fail late replies",
  "IN PROGRESS",
  "**DONE (core test, Windows)**"
 ],
 [
  "TRACK-P06",
  "Rust local read/write/process/Git tools with scoped approvals",
  "IN PROGRESS",
  "**IN PROGRESS (security regressions added)**"
 ],
 [
  "TRACK-P07",
  "Native approval dialog, persistent permission/audit receipts",
  "IN PROGRESS",
  "**IN PROGRESS (Windows source-matched two-stage consent GUI PASS)**"
 ],
 [
  "TRACK-P08",
  "Agent iteration loop and automated coding fixture (fail→fix→pass)",
  "IN PROGRESS",
  "**IN PROGRESS (host checkpoint persistence primitive; no actual native agent loop)**"
 ],
 [
  "TRACK-P14",
  "UX: clean rooms, Tasks plan, searchable roles, long history",
  "IN PROGRESS",
  "**IN PROGRESS (Tidal Atelier semantic CSS migration, no installed GUI)**"
 ],
 [
  "TRACK-P15",
  "Documents, RAG, search, citations, export, workflow DAG",
  "IN PROGRESS",
  "**IN PROGRESS (bounded PDF/DOCX parsing, opt-in saved text, no generated RAG answers)**"
 ],
 [
  "TRACK-P09",
  "Role catalog: exactly 200 distinct substantive instructions",
  "IN PROGRESS",
  "**DONE (prompt templates only)**"
 ],
 [
  "TRACK-P10",
  "Exactly 200 vetted, installed and exercised skill/MCP integrations",
  "IN PROGRESS",
  "**IN PROGRESS / LIVE QUALIFICATION FAILING (10 PASS)**"
 ],
 [
  "TRACK-P16",
  "Encrypted portable backup/restore with tested recovery",
  "IN PROGRESS",
  "**IN PROGRESS (Windows local encrypted recovery and saved Evidence fixture)**"
 ],
 [
  "TRACK-P17",
  "E2EE opt-in sync with conditional writes/conflict/rollback",
  "IN PROGRESS",
  "**IN PROGRESS (opt-in S3 HTTPS adapter, no live provider)**"
 ],
 [
  "TRACK-P18",
  "Authenticated, paired user-owned Linux runner and scheduling",
  "IN PROGRESS",
  "**IN PROGRESS (real opt-in authenticated loopback TCP, not remote executor)**"
 ],
 [
  "TRACK-P19",
  "Android-safe GUI and real signed APK acceptance",
  "BLOCKED",
  "**BLOCKED**"
 ],
 [
  "TRACK-P20",
  "Windows, macOS Intel/ARM and Linux installers plus QA",
  "IN PROGRESS",
  "**IN PROGRESS (frozen Windows/Linux packages GUI-tested; latest checkout drift)**"
 ],
 [
  "TRACK-P21",
  "Adversarial security tests, SBOM/license and privacy audit",
  "IN PROGRESS",
  "**IN PROGRESS (write parent-path security narrowed, sandbox open)**"
 ],
 [
  "TRACK-P22",
  "Fixed-revision cross-platform CI, signing, release validation",
  "BLOCKED",
  "**BLOCKED (latest preflight 8/11 gates fail)**"
 ],
 [
  "TRACK-P23",
  "User docs, sample projects, setup and portable exports",
  "IN PROGRESS",
  "**IN PROGRESS**"
 ]
];
const ledger = read('docs/quality/acceptance-ledger.md').split(/\r?\n/);
const features = read('docs/mission/master-feature-acceptance.md').split(/\r?\n/);
const roles = JSON.parse(read('catalog/roles.json')).roles;
const inventory = JSON.parse(read('skills-manifest/inventory.json')).entries;
const clean = (x) => String(x ?? '').replace(/\|/g, '\\|').replace(/\s+/g, ' ').replace(/^\[ \] /, '').trim();
const entries = [];
const sections = new Map();
const scope = {
  '00': ['Principal architect', 'AGENTS.md; docs/mission/; README.md', '00'],
  '01': ['Principal architect', 'docs/mission/; crates/; apps/desktop/', '00'],
  '02': ['Principal architect', 'AGENTS.md; docs/security/; README.md', '00'],
  '03': ['Research specialist (queued)', 'docs/architecture-decision.md; docs/security/', '00'],
  '04': ['Prime (UX wave; worker-3 startup failed)', 'apps/desktop/src/routes/; docs/design/', '01'],
  '05': ['Prime (UX wave; worker-3 startup failed)', 'apps/desktop/src/routes/; docs/design/', '01'],
  '06': ['Prime (UX wave; worker-3 startup failed)', 'apps/desktop/src/routes/; assets/; docs/design/', '01'],
  '07': ['Core worker-1 (second and fifth waves independently verified)', 'crates/; apps/desktop/src-tauri/', '02'],
  '08': ['Core worker-1 (second and fifth waves independently verified)', 'crates/constellation-core/', '02'],
  '09': ['Provider specialist (queued)', 'crates/constellation-provider-*/; apps/desktop/src-tauri/', '03'],
  '10': ['Agent loop worker-1 (third wave verified; native integration queued)', 'crates/constellation-agent-loop/; crates/constellation-workflows/', '03'],
  '11': ['Tool-security worker-1 (focused fix verified; residual queued)', 'crates/constellation-tools/; apps/desktop/src-tauri/', '02'],
  '12': ['Integrations specialist (queued)', 'skills-manifest/; catalog/', '04'],
  '13': ['Role evaluation specialist (queued)', 'catalog/; tests/', '04'],
  '14': ['Integrations specialist (queued)', 'skills-manifest/; /skills', '04'],
  '15': ['Tool-security worker-1 (focused fix verified; residual queued)', 'crates/constellation-tools/; apps/desktop/src-tauri/', '02'],
  '16': ['Runner specialist (queued)', 'crates/constellation-runner-protocol/; docs/server-runner.md', '05'],
  '17': ['Sync specialist (queued)', 'crates/constellation-sync/; crates/constellation-project-backup/', '05'],
  '18': ['Android specialist (queued)', 'apps/desktop/src-tauri/; docs/android.md', '06'],
  '19': ['Evidence/productivity specialist (queued)', 'crates/constellation-evidence/; apps/desktop/src-tauri/', '04'],
  '20': ['Feature specialist (queued)', 'crates/; apps/desktop/', '06'],
  '21': ['Security specialist (queued)', 'crates/constellation-tools/; docs/security/; tests/', '07'],
  '22': ['Perf specialist (queued)', 'crates/; apps/desktop/; docs/qa/', '07'],
  '23': ['GUI QA specialist (queued)', 'tests/windows-*; docs/qa/; apps/desktop/', '07'],
  '24': ['CI specialist (queued)', '.github/workflows/validate.yml; tests/', '07'],
  '25': ['Release specialist (queued)', 'dist/; docs/release/; tests/release-readiness.ps1', '08'],
  '26': ['Docs specialist (queued)', 'README.md; assets/; docs/', '08'],
  '27': ['Docs specialist (queued)', 'docs/; README.md', '08'],
  '28': ['Prime', 'docs/mission/; MASTER_CONTINUATION.md', '00'],
  '29': ['Prime', 'docs/mission/; Git worktrees', '00'],
  '30': ['Prime', 'docs/quality/acceptance-ledger.md; docs/qa/', '08'],
  '31': ['Prime', 'docs/mission/; docs/release/', '08'],
  '32': ['Research specialist (queued)', 'docs/; external source citations', '00'],
  '33': ['Prime', 'docs/mission/; tests/release-readiness.ps1', '08'],
};

function add({id, title, status='TODO', priority='P1', deps='P01; source-reviewed design and native boundaries', owner='Prime (queue)', files='docs/mission/', acceptance='', evidence='', blocker='', wave='02', source='original specification'}) {
  entries.push({id,title:clean(title),status,priority,deps:clean(deps),owner,files:clean(files),acceptance:clean(acceptance || `Implement the complete behavior described by ${id}, including persistence, negative and cancellation tests where applicable; verify installed app and target platform.`),evidence:clean(evidence),blocker:clean(blocker || 'No fully scoped end-to-end platform PASS attached'),wave,source});
}

// Keep all 520 previously published SPEC IDs and their concise requirement
// descriptions without reading or publishing the private original master.
// Derivation from the public summary is a historical provenance statement,
// not independent public verification of the private original source.
if (PUBLIC_SPEC_ROWS.length !== 520 || new Set(PUBLIC_SPEC_ROWS.map(([id]) => id)).size !== 520)
  throw Error('Public specification traceability must contain exactly 520 unique IDs');
const serial = new Map();
for (const [id, title, subsection, originalLine] of PUBLIC_SPEC_ROWS) {
  const match = /^SPEC-(\d{2})-(\d{3})$/.exec(id);
  if (!match || !Number.isInteger(originalLine) || originalLine < 1)
    throw Error('Malformed public specification traceability entry');
  const section = match[1];
  serial.set(section, (serial.get(section) ?? 0) + 1);
  const [owner,files,wave] = scope[section] ?? scope['00'];
  add({id,title,priority:['11','15','17','18','21','23','25','30'].includes(section)?'P0':'P1',
    deps:`SPEC-${section} section review; target app/runner hardware; permission design`,
    owner,files,
    acceptance:`Prior public master synopsis ?${Number(section)}, ${subsection}; implement named behavior including denial, durability and platform-specific manual/native tests as appropriate. Private original referenced line ${originalLine}; not independently publicly verified.`,
    evidence:`Original specification withheld (private original line ${originalLine}; synopsis already public, source not independently verifiable)`,
    blocker:'No fully scoped end-to-end platform PASS attached',wave,
    source:`private-original:line-${originalLine} (not distributed)`});
}
if (serial.size !== 31)
  throw Error('Public specification traceability lost a source section');

// The formerly internal P01-P23 board is not a reproducible CI input. Keep
// the task labels and conservative statuses already present in the published
// plan; no private board rows, owner notes or file contents are uploaded.
if (PUBLIC_TRACK_ROWS.length !== 23 || new Set(PUBLIC_TRACK_ROWS.map(([id])=>id)).size !== 23)
  throw Error('Public tracking snapshot must contain 23 unique tracks');
for (const [id,title,priorStatus,priorBlocker] of PUBLIC_TRACK_ROWS) {
  const match = /^TRACK-P(\d{2})$/.exec(id);
  if (!match || !['TODO','IN PROGRESS','BLOCKED'].includes(priorStatus))
    throw Error('Malformed public track summary');
  const idx=Number(match[1]);
  const sec=idx<=2?'08':idx<=8?'10':idx<=10?'14':idx<=13?'09':idx<=15?'19':idx<=17?'17':idx<=19?'18':idx<=21?'21':'25';
  const [owner,files,wave]=scope[sec];
  add({id,title,status:priorStatus,
    priority:idx===6||idx===7||idx===8||idx===10||idx===21||idx===22?'P0':'P1',
    deps:`All relevant SPEC and ACC rows; earlier P${match[1]} prerequisites`,
    owner,files,acceptance:`Close every platform-qualified subtask, negative test and source-matched GUI/release gate linked to P${match[1]}.`,
    evidence:'Internal coordination board withheld; this public task is not independently verified',
    blocker:priorBlocker || 'No final independently verified platform acceptance',
    wave,source:'public traceability snapshot (internal board withheld)'});
}

// Original platform acceptance rows are each independently actionable.
let acceptanceRow=0;
for(const line of ledger){const cols=line.split('|').map(x=>x.trim()); if(cols.length<13)continue;
 const id=cols[1];if(!/^[A-Z]+-\d{2}$/.test(id))continue;
 acceptanceRow += 1;
 const req=cols[2];const platform=cols[3];const prefix=id.split('-')[0];
 const sec=({CORE:'08',UX:'06',PROV:'09',AGENT:'10',TOOLS:'11',PERM:'15',ROLE:'13',SKILL:'14',RAG:'19',DOC:'19',DATA:'19',FLOW:'10',REMOTE:'16',SYNC:'17',ANDROID:'18',SEC:'21',PERF:'22',CI:'24',SHIP:'25'})[prefix]??'20';
 const [owner,files,wave]=scope[sec];
 add({id:`ACC-${id}-${String(acceptanceRow).padStart(3,'0')}`,title:`${req} (${platform})`,status:'TODO',priority:prefix==='SEC'||prefix==='SHIP'||prefix==='PERM'?'P0':'P1',deps:`Implementation of relevant SPEC §${Number(sec)}; real ${platform} target; source-bound build`,owner,files,acceptance:`Execute ${cols[5]||'required'} acceptance on ${platform}; record requirement_id, build SHA, environment, test, manual/automated, observed result, screenshot/log, verifier, timestamp and blocker. Denial/failure cases included.`,evidence:'[188-row evidence ledger](../quality/acceptance-ledger.md)',blocker:`Ledger currently ${cols[8]}; ${cols[12]??'no platform receipt'}`,wave,source:'docs/quality/acceptance-ledger.md'});
}

let inCi=false;
for(const line of features){if(line.startsWith('## Seventeen'))inCi=true;
const m=line.match(/^\|\s*(\d{2})\s*\|\s*\[ \]\s*(.*?)\s*\|\s*(.*?)\s*\|$/);if(!m)continue;
const id=`${inCi?'CI':'FEATURE'}-${m[1]}`;const linked=m[3].match(/P\d{2}/g)?.join(', ')??'';
const sec=inCi?'24':(Number(m[1])===13?'06':Number(m[1])>=27&&Number(m[1])<=30?'19':'20');const [owner,files,wave]=scope[sec];
add({id,title:m[2].replace(/\*\*/g,'').replace(/`/g,''),status:'TODO',priority:inCi||['04','05','09','10','13','17','24'].includes(m[1])?'P0':'P1',deps:linked||'Host review and source-matched acceptance',owner,files,acceptance:m[3],evidence:'[40 features + 17 CI families](master-feature-acceptance.md)',blocker:inCi?'No verified hosted run with complete job family and immutable commit':'No end-to-end source-pinned platform acceptance',wave,source:'docs/mission/master-feature-acceptance.md'});
}

// A definition existing in the catalog is not an empirical evaluation. Each
// of 200 roles needs a separately verified appropriate model + permission
// matrix before claiming a working professional specialization.
for(const role of roles){add({id:`ROLE-${role.id}`,title:`Evaluate ${role.name} role against its stated output contract`,status:'TODO',priority:'P2',deps:'TRACK-P03; TRACK-P08; FEATURE-33; real permitted model',owner:'Role evaluation specialist (queued)',files:'catalog/roles.json; catalog/roles.test.mjs; docs/qa/role-evaluations/',acceptance:`Run baseline, user task and negative/unavailable-capability scenario; verify ${role.deliverable}; record source, selected model, limits, actual output, quality and tool-authorization outcome.`,evidence:'[Role definitions](../../catalog/roles.json)',blocker:'Only 200 structurally valid prompt templates; no independently evaluated model/tool behavior',wave:'04',source:`catalog/roles.json:${role.id}`});}

// Exactly 200 non-CLI integrations are listed but qualification is different
// from file presence. Keep even previously exercised candidates pending final
// replay on the frozen source, and preserve real unverified statuses.
for(const item of inventory.filter(entry=>entry.kind!=='cli')){const qualified=item.status==='installed_vetted';
 add({id:`INTEGRATION-${item.id.replace(':','-')}`,title:`Vet, install and independently exercise ${item.id}`,status:qualified?'IN PROGRESS':'TODO',priority:'P1',deps:'TRACK-P10; verified publisher/license/source; independent scoped exercise',owner:'Integrations specialist (queued)',files:'skills-manifest/inventory.json; skills-manifest/evidence/; /skills',acceptance:`Verify exact pinned source hash, publisher, SPDX/license, trust and permission boundaries, managed installed entrypoint, independent live or appropriately scoped exercise, error/denial behavior, source-bound receipt replay; for MCP, verify actual runtime handshake.`,evidence:qualified?'[Prior skill evidence](../../skills-manifest/evidence/) (must requalify against current source)':'[Inventory](../../skills-manifest/inventory.json)',blocker:qualified?'Current source changed; strict replay may reject receipt and no runtime MCP evidence':'Source reviewed or installed alone does not earn qualification',wave:'04',source:`skills-manifest/inventory.json:${item.id}`});
}

const extra=[
 ['PLAN-001','Verify private original against the published requirement synopsis without disclosing original source','P0','Prime','00','docs/mission/generate-complete-execution-plan.mjs','Independently verify 520 public summaries and original line mappings without publishing private instructions','Private original withheld; public-source comparison pending'],
 ['PLAN-002','Generate and maintain complete task inventory without fabricating completion','P0','Prime','00','docs/mission/COMPLETE_EXECUTION_PLAN.md','Check deterministic counts and IDs, all 188 ACC rows, 40 features, 17 CI, 23 tracks, 200 roles, 200 integrations','Generator at docs/mission/generate-complete-execution-plan.mjs'],
 ['PLAN-003','Perform post-wave independent specification-to-source completeness re-audit','P0','Prime','07','docs/mission/; apps/; crates/; docs/qa/','Reconcile code with every SPEC/ACC/FEATURE/CI row and changed input hashes; record newly found gaps','No final all-requirement audit yet'],
 ['WORKER-001','Verify actual worker sessions and exclusive file ownership','P0','Prime','00','MASTER_CONTINUATION.md; docs/mission/COMPLETE_EXECUTION_PLAN.md','Compare current agents status and diff to mission assignment before credit; capture failures','worker-1 second wave active, worker-2 and worker-3 failed to start'],
 ['WORKER-002','Resolve agent-loop worker startup failure and reassign implementation','P0','Prime','03','crates/constellation-agent-loop/','Verify chat identity and actual worker patch/test; prime can implement independently','worker-2 failed to start; no agent-loop edits credited'],
 ['UX-COLOR-001','Read applicable accessible design system and color theory skills','P1','Prime (worker-3 startup failed)','01','docs/design/; skills://plugins/uiuxdesigner','Report exact skill content, three original color systems, evaluation criteria and provenance','Four relevant installed skills read; independent designer evaluation pending'],
 ['UX-COLOR-002','Create semantic light/dark/high contrast color tokens','P1','Prime (worker-3 startup failed)','01','apps/desktop/src/routes/palette.css; docs/design/','Select distinctive palette; run contrast 4.5:1 normal, 3:1 large/icons; semantic warning/disabled/focus; no uncontrolled hard-coded colors','Semantic foundation and 3 Node contrast/theme tests pass; remaining legacy CSS literal colors require migration'],
 ['UX-COLOR-003','Apply palette across every screen, native consent context and responsive variant','P1','Prime (worker-3 startup failed)','01','apps/desktop/src/routes/; apps/desktop/src-tauri/','Source-matched light/dark states, small/large window, native dialog consistency and no regressions','Major shell and controls tokenized; entire screen/native consent coverage and installed GUI still pending'],
 ['UX-COLOR-004','Verify color/UX against real GUI screenshots and accessibility tooling','P0','Prime; independent UI QA specialist queued','07','docs/qa/; tests/','Installed private Win32 screenshot and keyboard tree, compare all screens, RTL/IME, 900x600 and 200% DPI','No source-matched post-palette GUI test yet'],
 ['RELEASE-001','Choose public brand and check source/marks/licensing','P1','Release specialist (queued)','08','README.md; LICENSE; assets/','Document real distinct brand and third-party asset licenses','Brand uniqueness not verified'],
 ['RELEASE-002','Reconcile untracked crate code, generated files and lockfiles without data loss','P0','Prime','07','.gitignore; crates/; apps/desktop/','Review every untracked/modified file, include needed source/lock and exclude secrets/caches; verify no deletions','94 source paths currently outside Git index'],
 ['RELEASE-003','Run full secret, license, history, vulnerability, SBOM and binary audit','P0','Security/CI specialist (queued)','07','.github/workflows/; docs/security/; tests/','Record exact tools/results, secret/history review and dependency mitigation','Independent full supply-chain review incomplete'],
 ['RELEASE-004','Stabilize clean reviewed release commit and reproducible checkout','P0','Prime','08','.git/; docs/mission/; crates/; apps/','Review diffs, freeze source, run reproducible --locked tests and compare source hashes','Working tree actively changing; never destructively reset'],
 ['RELEASE-005','Select semantic version and update all versioned manifests','P1','Release specialist (queued)','08','CHANGELOG.md; apps/desktop/package.json; Cargo.toml; docs/release/','Same version in app/about/files/tags/release docs; choose v0.x if incomplete','No verified version alignment'],
 ['RELEASE-006','Verify intended Git remote, repository identity, target branch and permissions','P0','Prime','08','.git/config; docs/release/','Owner and permissions confirmed from authenticated remote; recorded commit and branch','git remote -v empty'],
 ['RELEASE-007','Prepare safe reviewable Git commits and push intended branch','P0','Prime','08','.git/','No data loss, secret scan, successful push with remote commit equals intended local commit SHA','Remote unconfigured; no public push'],
 ['RELEASE-008','Run and independently inspect all 17 hosted CI job families','P0','Security/CI specialist (queued)','08','.github/workflows/validate.yml; docs/qa/','Record immutable run URLs, job IDs, expected OS, success/failure logs and artifact hash','Only 9 job declarations, no hosted runs'],
 ['RELEASE-009','Build and test clean-user Windows NSIS installation and uninstall','P0','Release specialist (queued)','08','apps/desktop/src-tauri/; tests/windows-*; dist/','Installed actual setup with registry/lifecycle, consent, GUI, and upgrade tests','Existing registry collision; isolated VM needed'],
 ['RELEASE-010','Build and test Linux deb and AppImage in clean environments','P0','Release specialist (queued)','08','tests/linux-*; dist/','Reproducible source-pinned binaries, dpkg/AppImage install, ELF identity, Xvfb and clean VM GUI','Previous frozen snapshots predate current source'],
 ['RELEASE-011','Build/inspect/test macOS arm64 DMG','P0','Release specialist (queued)','08','apps/desktop/src-tauri/; dist/','macOS arm64 real GUI, app/DMG identity, signing/notarization truth','No accessible macOS target/signing evidence'],
 ['RELEASE-012','Build/inspect/test macOS x64 DMG','P0','Release specialist (queued)','08','apps/desktop/src-tauri/; dist/','macOS x64 real GUI, app/DMG identity, signing/notarization truth','No accessible macOS target/signing evidence'],
 ['RELEASE-013','Build and test release-signed Android arm64 APK','P0','Android specialist (queued)','08','apps/desktop/src-tauri/; docs/android.md; dist/','Verify apksigner, stable key, ADB install/upgrade, lifecycle, Task/runner/permission flow','No authorized signing key, emulator/device proof'],
 ['RELEASE-014','Build and validate paired Linux runner distribution','P0','Runner specialist (queued)','08','crates/constellation-runner-protocol/; dist/','Headless binary/service/scheduler install, pairing/revocation, restart, offline queue, checksums','Real runner service absent'],
 ['RELEASE-015','Generate SHA256SUMS, SBOM, provenance and asset manifests','P0','Release specialist (queued)','08','dist/; docs/release/','Exact byte hash of all source-matched platform artifacts, signed status truthful','No current-source complete asset matrix'],
 ['RELEASE-016','Create and verify public GitHub Release with correct assets','P0','Prime','08','README.md; CHANGELOG.md; docs/release/','Public GitHub release tag/commit equal tested revision; all assets uploaded, links and SHA verified','Remote/publication auth unavailable'],
 ['RELEASE-017','Independently download and install each published asset','P0','Independent QA specialist (queued)','08','docs/qa/; dist/','Fresh downloads, checksum recomputation, installed clean-machine smoke and provider/task flows','No published assets'],
 ['RELEASE-018','Publish real screenshots, demos, README and release notes','P1','Docs/release specialist (queued)','08','README.md; assets/; docs/','Truthful links, native screenshots and GIFs, platform table, privacy/limitations, license and FAQ','Needs tested native builds and release URLs'],
 ['RELEASE-019','Run final independent full-spec acceptance audit','P0','Prime + independent QA (queued)','08','docs/mission/COMPLETE_EXECUTION_PLAN.md; docs/quality/acceptance-ledger.md','Zero unjustified PASS, all mandatory checks green across 188 rows/40 features/17 CI, 200 roles and integrations','Mandatory gates remain open'],
 ['RELEASE-020','Verify public download URLs, remote HEAD, workflow URLs and SHA matches','P0','Prime','08','docs/release/; README.md','Open every public URL, compare remote commit/tag and uploaded bytes to tested local artifacts','No remote or public release configured'],
];
// Source-to-spec gap audit: additional actionable tasks independently
// identified in real checked-in/untracked implementation trees. Keep each
// grounded in an inspected source and avoid interpreting the absence of a
// lexical TODO marker as proof that a feature is implemented.
const discoveries = [
  ['D-001','Turn effect-free runner protocol into authenticated deployable Linux daemon','P0','Runner specialist (queued)','05','crates/constellation-runner-protocol/src/lib.rs; new runner service crate','Real paired daemon accepts scoped jobs with persistent queue; restart, revocation and denied writes demonstrated','Protocol src/lib.rs declares no I/O or tool execution; no runner binary source found'],
  ['D-002','Create Android-native project and safe companion lifecycle, beyond launcher icons','P0','Android specialist (queued)','06','apps/desktop/src-tauri/; docs/android.md','Release-signed APK and ADB emulator smoke, Android background/URI/security behavior','Only Android icon assets found; docs/android.md explicitly says APK unverified'],
  ['D-003','Add real multi-file PDF and office ingestion with citation provenance','P1','Evidence specialist (queued)','04','crates/constellation-evidence/src/lib.rs; apps/desktop/src-tauri/','PDF extraction/page anchors, DOCX/CSV import, bounded provenance and malicious document tests','Current evidence crate accepts caller-provided PlainText/Markdown only'],
  ['D-004','Persist/reload multi-document indexes with incremental freshness controls','P1','Evidence specialist (queued)','04','crates/constellation-evidence/; crates/constellation-core/','Durable source corpus, change/deletion detection, reindex exact changed documents, retrieval permission tests','Evidence crate states no filesystem/network/model APIs; no complete corpus user workflow verified'],
  ['D-005','Implement source-grounded model answers with hard unsupported-claim detection','P1','Research/product specialist (queued)','04','crates/constellation-evidence/; apps/desktop/src-tauri/','Cross-document QA with accurate page/span citation, unsupported state and prompt injection isolation','Lexical hits and quoted spans do not constitute source-grounded AI answers'],
  ['D-006','Add real model-directed Process and Git steps through independent native consent','P0','Agent-engine specialist (queued)','03','crates/constellation-agent-loop/; apps/desktop/src-tauri/','Provider proposes bounded Process/Git, user approves native preview, sandboxed exact effects and replay/denial tests','Model decision schema currently supports only read/create/final'],
  ['D-007','Complete durable multi-turn agent scheduler with true fail-edit-test iteration','P0','Agent-engine specialist (queued)','03','crates/constellation-agent-loop/; crates/constellation-core/; apps/desktop/src-tauri/','Live model creates plan from failed real test and produces fix; repeated bounded cycles persisted across restart and stopped by user','Current offline agent-loop explicitly disclaims autonomous desktop integration'],
  ['D-008','Install effective OS sandbox and verify process filesystem/network isolation','P0','Security specialist (queued)','02','crates/constellation-tools/; apps/desktop/src-tauri/','Adversarial malicious process cannot escape allowed workspace, spawn descendants or send unapproved network traffic','Write parent identity hardening mitigates races but does not provide kernel sandbox'],
  ['D-009','Provide atomic compare-and-swap replacement against malicious concurrent writes','P0','Security specialist (queued)','02','crates/constellation-tools/src/execution.rs; verified_open.rs','Concurrent adversarial writer test proves no stale compare-and-replace at target, including moved directories','Current Windows and Unix implementation still has post-hash replacement window'],
  ['D-010','Close first-stage native read-denial authenticated journaling gap','P0','Security specialist (queued)','02','apps/desktop/src-tauri/src/lib.rs; crates/constellation-tools/','First user denial journal persists across restart and never probes private bytes before approval','P07 evidence still reports first-stage denial journal absent'],
  ['D-011','Implement provider retry/failover and monetary spend accounting','P1','Provider specialist (queued)','03','crates/constellation-provider-adapters/; apps/desktop/src-tauri/','Provider matrix exercises bounded jitter/backoff, provider switch, real usage/cost accounting, cancel, budget denial','P11/P13 explicitly report real billing and failover remain unverified'],
  ['D-012','Make provider streaming preview/Stop real installed GUI acceptance','P0','GUI QA specialist (queued)','07','apps/desktop/src/routes/+page.svelte; tests/','Source-pinned native GUI streams partial text, stops socket, saves no failed final message','Streaming parser/native localhost tests exist but latest installed GUI acceptance absent'],
  ['D-013','Convert remaining legacy colors to semantic theme tokens across all screens','P1','Prime/UX specialist (queued)','01','apps/desktop/src/routes/studio.css; palette.css','Contrast scanner and screenshots detect no leftover light-only surfaces or unreadable dark-mode labels','Principal UI tokenized; many literal legacy greens remain'],
  ['D-014','Test native Windows consent with new theme and focus controls','P0','GUI QA specialist (queued)','07','tests/windows-installed-uia-smoke.ps1; apps/desktop/src/routes/','Private screenshot, keyboard, dialog visibility, 900x600, 200% DPI, RTL and IME with exact matching build','No new installed GUI artifact after palette/focus source edits'],
  ['D-015','Implement actual cross-device sync opt-in UI and trusted checkpoint recovery','P0','Sync specialist (queued)','05','crates/constellation-sync/; apps/desktop/src-tauri/','Live S3 conditional CAS from two devices, conflict recovery, rollback detection, opt-in activation','Sync crate explicitly disables remote CAS until service probe and has no native UI acceptance'],
  ['D-016','Add actual full-file attachment backup with one coherent snapshot boundary','P1','Backup specialist (queued)','05','crates/constellation-project-backup/; crates/constellation-core/','Crash during two-db/attachment backup restores consistent recoverable state on second host','P16 reports non-atomic cross-database snapshots and missing attachment recovery'],
  ['D-017','Implement 17 distinct verifiable CI job families and hosted matrix acceptance','P0','CI specialist (queued)','07','.github/workflows/validate.yml; tests/','17 required families on fixed source, signed artifact jobs, real hosted run IDs; failures block release','validate.yml currently defines 9 jobs / 20 matrix instances, not 17 families'],
  ['D-018','Replace misleading unfinished-settings copy with precise capability statuses','P1','Prime/product specialist (queued)','01','apps/desktop/src/routes/+page.svelte','Every Settings/Tasks screen truthfully distinguishes verified workflow steps and absent tool/runner/sync','Current advanced-limitations copy may differ from implemented partial workflow execution'],
  ['D-019','Install fresh Windows setup in disposable isolated account and validate registry lifecycle','P0','Release specialist (queued)','08','tests/windows-installed-uia-smoke.ps1; docs/qa/; dist/','Clean install, start, upgrade and uninstall, no modification of existing Constellation user registry','Existing account registry has old Constellation identity, previous tests use extracted direct EXE'],
  ['D-020','Prove every intended source and locked dependency exists in eventual Git commit','P0','Prime','08','crates/**/Cargo.lock; .github/workflows/validate.yml; tests/release-readiness.ps1','Release preflight passes committed source completeness and frozen lockfiles on clean checkout','Current `main` tree has 12/14 untracked Rust lockfiles and many sources not in Git index'],
  ['D-021','Implement versioned transactional SQLite migrations and interruption recovery','P0','Core specialist (queued follow-up)','02','crates/constellation-core/src/store.rs; crates/constellation-core/tests/','Persist schema version, upgrade every supported legacy version atomically, refuse unknown future version without partial mutations; interrupt/reopen tests and independent connection contention','Core atomic initial DDL fix has no user_version migration ledger or process-kill crash proof'],
  ['D-022','Integrate authenticated agent checkpoint storage and trusted revision floor into native task ledger','P0','Prime/native integration (queued)','03','apps/desktop/src-tauri/src/lib.rs; crates/constellation-agent-loop/src/host_steps.rs; crates/constellation-core/','Persist host checkpoint and monotonic floor transactionally with task state; HMAC/rollback and crash pending-approval journal reconciliation; stale model results cannot grant or repeat tools','Third-wave agent-loop crate only exports untrusted serialized checkpoint contract; native authoritative storage and recovery absent'],
  ['D-023','Source-matched native UI verification of failed stream provisional-text cleanup','P0','GUI QA specialist (queued)','07','apps/desktop/src/routes/+page.svelte; tests/windows-*; docs/qa/','Private Win32 delayed/invalid SSE test shows failed model delta removed, other agent replies retained, no invalid answer persisted and cancellation works under room switch','Prime implemented failure-scoped preview clearing and Svelte build passed, but no native GUI/source-matched channel test'],
];
const extraInProgress = new Set(['PLAN-002','WORKER-001','UX-COLOR-001','UX-COLOR-002','UX-COLOR-003','RELEASE-002']);
const externallyBlocked = new Set(['WORKER-002','UX-COLOR-004','RELEASE-006','RELEASE-007','RELEASE-008','RELEASE-009','RELEASE-011','RELEASE-012','RELEASE-013','RELEASE-016','RELEASE-017','RELEASE-020']);
for(const [id,title,priority,owner,wave,files,acceptance,blocker] of extra){add({id,title,status:id==='PLAN-001'?'BLOCKED':extraInProgress.has(id)?'IN PROGRESS':externallyBlocked.has(id)?'BLOCKED':'TODO',priority,deps:id==='PLAN-001'?'Private original withheld; public synopsis is not an original-source attestation':id.startsWith('RELEASE-')?'Preceding release gates and clean source':'Relevant SPEC and ACC requirements',owner,files,acceptance,evidence:id==='PLAN-001'?'Private original withheld; no public original-source verification':id==='PLAN-002'?'[Generator](generate-complete-execution-plan.mjs)':'Internal coordination board withheld; public task summary only',blocker:'No final acceptance proof; '+blocker,wave,source:'2026-10-31 user continuation requirements'});}
const sourceSlicesInProgress = new Set(['D-013','D-021','D-023']);
for(const [id,title,priority,owner,wave,files,acceptance,blocker] of discoveries){add({id:`DISCOVERY-${id}`,title,status:sourceSlicesInProgress.has(id)?'IN PROGRESS':'TODO',priority,deps:'Master specification plus source-gap audit; related SPEC/ACC and feature gates',owner,files,acceptance,evidence:'[Independent inspected-source gap audit](../qa/source-gap-audit-2026-10-31.md)',blocker,wave,source:'docs/qa/source-gap-audit-2026-10-31.md'});}

const ordered=[...entries].sort((a,b)=>a.wave.localeCompare(b.wave)||a.id.localeCompare(b.id));
const idSet=new Set();for(const item of ordered){if(idSet.has(item.id))throw Error(`Duplicate task ID ${item.id}`);idSet.add(item.id);}
const counts={};for(const item of ordered)counts[item.status]=(counts[item.status]??0)+1;
const perWave=new Map();for(const item of ordered){let arr=perWave.get(item.wave)??[];arr.push(item);perWave.set(item.wave,arr);}
const esc=x=>clean(x).replace(/\[/g,'\\[').replace(/\]/g,'\\]');
// Do not hyperlink private original prompts, coordination boards or local
// QA/skill receipts in a published generated document. Public links are a
// positive allowlist of source files committed with this generator.
const publicEvidenceLinks = new Set([
  'generate-complete-execution-plan.mjs',
  'master-feature-acceptance.md',
  '../quality/acceptance-ledger.md',
  '../../catalog/roles.json',
  '../../skills-manifest/inventory.json',
]);
function publicEvidence(value) {
  return String(value || 'No source-matched end-to-end evidence').replace(
    /\[([^\]\r\n]+)\]\(([^)\r\n]+)\)/g,
    (whole, label, target) => publicEvidenceLinks.has(target)
      ? whole
      : `${label} (internal source not distributed; evidence not independently publicly verified)`,
  );
}
const out=[];
out.push('# Constellation — COMPLETE EXECUTION PLAN', '',
 'Auto-generated only from **committed public requirement summaries embedded in the generator** and versioned acceptance, role, feature and integration inventories. The 520 specification summaries and 23 coordination labels were distilled from an earlier committed public execution plan; the original master and private board are withheld. **Independent original-source equivalence is not established.** Regenerate from reviewed public inputs; never fabricate completion.',
 '',`**Total independent traceability tasks: ${ordered.length}.** ${Object.entries(counts).map(([k,v])=>`${k}: **${v}**`).join(' · ')}. No release readiness is implied.`,
 '', `**Coverage:** ${serial.size} publicly summarized specification sections containing ${[...serial.values()].reduce((a,b)=>a+b,0)} requirement labels; ${entries.filter(x=>x.id.startsWith('TRACK-')).length}/23 tracks; ${entries.filter(x=>x.id.startsWith('ACC-')).length}/188 platform acceptance rows; ${entries.filter(x=>x.id.startsWith('FEATURE-')).length}/40 features; ${entries.filter(x=>x.id.startsWith('CI-')).length}/17 CI families; ${roles.length}/200 role evaluations; ${inventory.filter(x=>x.kind!=='cli').length}/200 integrations; ${extra.length} extra tasks; ${discoveries.length} source-gap tasks.`,
 '', '**Worker session truth:** This deterministic generator does not connect to currently running workers. Every owner in the task inventory is a planning assignment, **not** verified current chat ownership or completion. Check the actual live worker-status tool before editing overlapping files; preserve the reported source-specific QA receipts, even if a worker has since exited.',
 '', `**Source truth:** Source checks still require a clean, reviewed Git revision, verified intended remote, hosted CI, real model behavior, and platform-specific acceptance. The inventory currently labels ${inventory.filter(x=>x.kind!=='cli'&&x.status==='installed_vetted').length}/200 skills as vetted; these are **not** automatically verified: re-run \`node skills-manifest/validate.mjs --verify-local\` to measure current source-bound strict credit. ${roles.length}/200 roles have templates but empirical live-model qualification remains separately open. The 188-row platform ledger, 40 product features and 17 hosted CI families remain separate release blockers.`,
 '', '**Priority:** P0 security, native authorization, CI/source/release dependency; P1 core functionality and user experience; P2 empirically evaluating catalog role behavior. **Status:** VERIFIED DONE is limited to direct, narrowly scoped source/artifact proof; a source-only/fixture-only PASS never closes a full platform acceptance task.',
 '', '## Worker coverage and ownership audit', '',
 '| Role | Active chat | Owned files | Risk / next reassignment |', '| --- | --- | --- | --- |',
 '| Prime | Query current worker status | Mission plan, integration, CI/release gate coordination | Dirty worktree and incomplete release acceptance |',
 '| Security/path specialist | See actual live worker status and source QA | `crates/constellation-tools/` and native trust bridge | Real OS isolation, rollback and installed-app verification must be evidenced |',
 '| Core persistence specialist | See actual live worker status and source QA | `crates/constellation-core/` | Power-loss, supported legacy formats and native restart acceptance still open |',
 '| UX/color specialist | See actual live worker status and source QA | `apps/desktop/src/routes/`, design palette and QA | Installed-GUI keyboard/RTL/IME, theme and responsiveness still open |',
 '| Agent loop specialist | See actual live worker status and source QA | `crates/constellation-agent-loop/`, native model/tool bridge | Durable production provider-to-tool-to-model continuation and consent still open |',
 '| Provider specialist | See actual live worker status and source QA | `crates/constellation-provider-adapters/`, `constellation-provider-probe/` | Real API, transport policy, cancellation and billing still unverified |',
 '| Other specialisms | See actual live worker status | Queued per rows below; no ownership assumed | Assign disjoint source paths after checking active workers |',
 '', '## Execution waves and task-by-task acceptance', '');
for(const [wave,items] of [...perWave].sort(([a],[b])=>a.localeCompare(b))){out.push(`### Wave ${wave} — ${items.length} tasks`,'');
 const byOwner=new Map();for(const e of items){const arr=byOwner.get(e.owner)??[];arr.push(e);byOwner.set(e.owner,arr);}
 for(const [owner,group] of byOwner){out.push(`#### Workstream — ${esc(owner)} (${group.length} tasks)`,'');
  for(const e of group){const mark=e.status==='VERIFIED DONE'?'x':' ';
  out.push(`- [${mark}] **${e.id}** — **${esc(e.title)}**`, `  - **Status:** ${e.status} · **Priority:** ${e.priority} · **Worker/chat:** ${esc(e.owner)} · **Dependencies:** ${esc(e.deps)}`, `  - **Files/components:** ${esc(e.files)} · **Acceptance/tests:** ${esc(e.acceptance)}`, `  - **Evidence:** ${publicEvidence(e.evidence)} · **Remaining blocker:** ${esc(e.blocker||'None for this scoped task')}`);
  }
  out.push('');
 }
}
out.push('## Reconciliation protocol','',
 '1. Before each wave, confirm current `git status --porcelain=v1`, relevant code/lockfile hashes, `AGENTS.md`, actual `agents status`, free space, source and accepted scope. Prevent duplicate file ownership.',
 '2. Review each worker patch and test logs independently. Re-run targeted negative tests, rustfmt/strict Clippy, Svelte checks, source-bound GUI/IME/accessibility and platform packaging as required. Record observed SHA and environment.',
 '3. Check all SPEC/ACC/FEATURE/CI/ROLE/INTEGRATION and release rows against the source and original paragraphs; create new task IDs for newly discovered subclauses. Preserve original failures and avoid claiming coverage from mere source presence.',
 '4. Freeze a reviewed source revision, rebuild exact artifacts on available platforms, verify credentials/external conditions and public release separately. Never mark a mandatory gate complete until genuine evidence exists.',
 '5. Regenerate this matrix from reviewed public inputs and attach public source hashes and actual acceptance evidence. Internal worker reports and coordination boards remain private and must not become generated public hyperlinks. Add new requirements to the public generator before regeneration.',
 '', '## Explicit release hold', '',
 'The project is **not finished**. A private Git remote or source push does not establish a public release. Real-provider, native OS containment, 200 integrations, 200 role evaluations, 40 features, 188 acceptance rows, 17 hosted job families and Android/macOS platform gates remain open. Release tasks are not release acceptance.', '');
const path=resolve(root,'docs/mission/COMPLETE_EXECUTION_PLAN.md');
const document=out.join('\n');
console.log('Planning document byte count before writing:',Buffer.byteLength(document));
if(Buffer.byteLength(document)>10*1024*1024)throw Error('Unexpectedly large planning document');
if(process.argv.includes('--check')) {
  const current=readFileSync(path,'utf8');
  if(current!==document) {
    console.error('COMPLETE_EXECUTION_PLAN.md is stale. Re-run the plan generator, review diff, then commit both files.');
    process.exitCode=1;
  } else {
    console.log('Verified deterministic plan content and unique IDs; no files changed.');
  }
} else {
  writeFileSync(path,document,'utf8');
  console.log(JSON.stringify({file:path,total:ordered.length,counts,sections:serial.size,sectionsTasks:[...serial.values()].reduce((a,b)=>a+b,0),waves:[...perWave].map(([wave,e])=>[wave,e.length]),bytes:Buffer.byteLength(document)},null,2));
}
