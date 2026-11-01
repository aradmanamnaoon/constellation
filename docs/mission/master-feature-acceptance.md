# Constellation — master-specification feature acceptance map

**Source:** `Codex_Master_Prompt_Local_Agent_Collaboration_Platform.md` (30 October 2026, 1,237 lines), particularly §20, §23–25, and §30. This is an **implementation and evidence checklist**, not a claim that these features exist. The core [23-workstream board](active-task-plan.md) and [188-row platform acceptance ledger](../quality/acceptance-ledger.md) remain authoritative. Source or isolated fixture evidence cannot substitute for installed-app validation.

**Status convention:** `[ ]` means full end-to-end user acceptance remains open, even when a component exists or a narrow fixture has passed. An item closes only when its relevant feature, error/denial cases, persistent behavior and installed GUI are directly tested on a source-pinned build. The 40 differentiating product requirements below are separate from the 200 role templates and 200 verified skills/MCPs.

## 40 original product features — explicit completion tasks

| # | Master requirement and concrete acceptance exercise | Linked workstream |
| --- | --- | --- |
| 01 | [ ] **Ask a Team** — compose once, show 1–3 suggested roles, override routing, verify selected agents and actual replies | P03, P14 |
| 02 | [ ] **Agent Cards** — inspect real model, advertised and probed capabilities, allowed resources and cost without guessed claims | P03, P11, P14 |
| 03 | [ ] **Team Starter Kits** — instantiate six usable starter flows (app, research, files, weekly plan, writing, study), persist and export them | P09, P14, P15, P23 |
| 04 | [ ] **Trust Lens** — before dispatch identify every off-device recipient, data categories, exact provider and execution target; deny unreviewed egress | P07, P11, P21 |
| 05 | [ ] **Proof of Work** — task results show tested artifact hashes, changed files, execution receipts and unresolved limitations, including failures | P05, P06, P08, P15 |
| 06 | [ ] **Task Playback** — browse each durable model/tool/approval event with accurate chronology and replay without repeating side effects | P04, P05, P08 |
| 07 | [ ] **Budget Dial** — select quick/balanced/thorough, enforce per-model ceiling and represent costs as estimates when only estimated | P11, P13, P14 |
| 08 | [ ] **Compare Models** — same explicitly shared input to two providers; show side-by-side results, latency, failures and expense attribution | P03, P11, P13 |
| 09 | [ ] **Local-Only Switch** — prevent all provider/MCP/connector/runner network egress per room, including retry and indirect delegation | P03, P11, P17, P21 |
| 10 | [ ] **Privacy Receipt** — show actual contacted destinations, disclosed data category and execution machine; verify request/receipt parity | P05, P11, P21 |
| 11 | [ ] **Smart Handoff** — transfer bounded source/artifact to another agent with explicit privilege and egress review, attribution and provenance | P03, P08, P15 |
| 12 | [ ] **Recoverable Actions** — pre-change snapshot where supported, show exact undo capabilities; test overwrite, failed restore and cancellation | P06, P08, P16 |
| 13 | [ ] **Focus Mode** — keyboard-toggle focused room without losing drafts, scroll, task status or essential consent prompts | P14 |
| 14 | [ ] **Small-Team Debate** — evidence-backed opposing claims and neutral synthesis; preserve disagreements and do not invent consensus | P03, P11, P15 |
| 15 | [ ] **Role Recipes** — save and rerun a versioned sequence of agents/providers/approvals, with missing-capability notices | P09, P15 |
| 16 | [ ] **Explain the Plan** — readable summary and truthful expert DAG with errors, blocked dependencies, progress and cancellation | P04, P15, P14 |
| 17 | [ ] **Machine Passport** — inspect actual paired runner OS/version/key epoch/allowed scopes/health and revoke immediately | P18, P21 |
| 18 | [ ] **Connection Doctor** — test provider, MCP, storage and runner independently; display specific fail-closed diagnostics without credential leakage | P10, P11, P17, P18 |
| 19 | [ ] **Demo Without Billing** — deterministic offline exercise explicitly marked simulated in UI and records; cannot masquerade as real AI | P03, P14, P23 |
| 20 | [ ] **Learning By Doing** — complete an actual 2-minute first task with bounded setup and contextual steps; accessibility and clean install verified | P14, P23 |
| 21 | [ ] **Answer Calibration** — clearly distinguish source-backed statements, tool-verified outputs, model guesses and unverified material | P05, P11, P15 |
| 22 | [ ] **Adaptive Team Size** — recommend bounded relevant teammates; inspect and override assignments and cost ceiling | P03, P09, P11 |
| 23 | [ ] **Artifact Inbox** — persist/find/download completed files separately from chats with SHA, provenance and room permissions | P05, P15, P23 |
| 24 | [ ] **Change Impact Preview** — full bounded file path/size/diff/action, touched resources, network destinations and tool limits before approval | P06, P07, P08, P21 |
| 25 | [ ] **Environment Checker** — safely detect missing local model/build tooling, fail without installing or running unapproved binaries | P11, P14, P23 |
| 26 | [ ] **Workspace Templates** — persist room/role/layout configuration, export and restore an encrypted portable pack without secrets | P09, P14, P16 |
| 27 | [ ] **Cross-device Handoff** — Android to paired desktop task with intact approvals, encrypted sync and faithful statuses | P17, P18, P19 |
| 28 | [ ] **Quick Capture** — consented desktop clipboard/drag/file ingest and Android share sheet; sensitive content remains local unless approved | P14, P15, P19, P21 |
| 29 | [ ] **Knowledge Freshness** — compare indexed source fingerprint, signal stale citations and reindex exactly changed documents | P15 |
| 30 | [ ] **Sensible Notifications** — opt-in approval/task failure/completion/cost/sync events with per-device quiet hours and truthful delivery | P13, P14, P18, P19 |
| 31 | [ ] **Incident Recovery Mode** — after forced app termination show orphaned tasks, pending operations, journal state and recovery without replay | P04, P05, P06, P16 |
| 32 | [ ] **Model Capability Lab** — benign probe for tool call/JSON/multilingual/retrieval/streaming with exact provider/model evidence | P11, P13 |
| 33 | [ ] **Agent Evaluation Bench** — fixture task outcomes, baseline, quality, latency, monetary cost and unsafe-tool measurements | P03, P08, P11 |
| 34 | [ ] **Workflow Dry Run** — preview DAG/resource/approval/provider egress without sending billable requests; explicit activation | P04, P07, P15 |
| 35 | [ ] **Energy/Resource Awareness** — show trustworthy local inference requirements and battery/memory warnings only where measurable | P11, P14 |
| 36 | [ ] **Offline First-Run Data** — small licensed samples, no private content or account, real local search/chat simulation | P14, P15, P23 |
| 37 | [ ] **Import From Other Agents** — parse versioned role/skill/workflow formats, review permissions and reject untrusted execution by default | P09, P10, P15, P21 |
| 38 | [ ] **Localized Assistance** — English, Persian RTL and Arabic/CJK paths/input; verify bidirectional layout, IME, Unicode and scaling in real GUI | P14, P19 |
| 39 | [ ] **Role Safety Profiles** — advisory restrictions for regulated advice and separate verified human authorization of outbound publication/action | P09, P11, P21 |
| 40 | [ ] **Inspectable Product** — export project/data/receipts and explain source, provider, storage, runner and trust boundaries without misrepresentation | P16, P17, P18, P23 |

## Seventeen required CI and native release jobs

Check these against the **source tracked in Git**, not only local untracked workflows. An offline test pass does not establish a hosted run; a compile does not establish GUI behavior.

| # | Required job | Acceptance and current evidence boundary |
| --- | --- | --- |
| 01 | [ ] `rust-format-lint` | Workspace fmt + strict Clippy `-D warnings` on supported targets/features, pinned dependencies |
| 02 | [ ] `unit-and-integration` | Cross-platform Rust suites including concurrency, negative/replay and authenticated backup fixtures |
| 03 | [ ] `frontend-check` | Frozen dependency install, Svelte typecheck/build/component and accessibility smoke |
| 04 | [ ] `security-and-supply-chain` | Secrets in full Git history, licenses, advisories, SBOM, forbidden files and dependency drift |
| 05 | [ ] `schema-role-validation` | Exactly 200 distinct professional role definitions and policy-bound schema checks |
| 06 | [ ] `skills-200-audit` | Exactly 200 distinct vetted/installed/independently exercised integrations with current-source receipts; fail otherwise |
| 07 | [ ] `provider-contract-tests` | Real protocol envelopes for all claimed providers, streaming/tool/timeout/errors/fallback/cancellation and privacy policy |
| 08 | [ ] `mcp-acp-contract-tests` | Runtime MCP/ACP handshake, capability restrictions, revocation, malicious output and transport failure |
| 09 | [ ] `sync-tests` | Encrypted S3 conditional CAS, conflict, rollback, restore, offline and real-service capability attestation |
| 10 | [ ] `runner-tests` | Linux paired daemon install, task dispatch/cancel/restart, queue, auth/revocation, offline recovery and exact OS scope |
| 11 | [ ] `desktop-windows` | Build NSIS, install to clean user, launch app, native UIA, backup/file consent and uninstall/registry lifecycle |
| 12 | [ ] `desktop-macos-arm` (+ Intel) | Build real arm64 and x64 DMGs, inspect app identity, literal native GUI and signed/notarized status |
| 13 | [ ] `desktop-linux` | Build `.deb`/AppImage, dependency and package-content inspections, rootless and clean VM install, Xvfb GUI |
| 14 | [ ] `android-build` | Reproducible ARM64 signed APK, ABI/signing checks, emulator/ADB install, permission and app-resume smoke |
| 15 | [ ] `release-assets` | Reproducible filenames/versions, `SHA256SUMS.txt`, SBOM, true signing and source-to-binary provenance |
| 16 | [ ] `docs-links` | Real README screenshots, working internal/docs links, install commands and verified public release URLs |
| 17 | [ ] `repo-size` | Full tracked history under 500 MiB, no >50 MiB individual blobs, no credentials/models/installers in Git |

## Master release exit checks

- [ ] A real model with demonstrated tool capability completes an approved **fail → edit → test passing** coding fixture in the desktop GUI; the model is unable to create its own permissions, and a revoked operation cannot be replayed.
- [ ] An **installed**, source-pinned release package passes real GUI interactions for every claimed platform; a direct development EXE is separately labeled.
- [ ] Real opt-in BYO encryption/sync and paired owner-controlled 24/7 runner pass end-to-end, including offline device, revoked runner, conflicted concurrent writers and Android monitoring.
- [ ] The role/skills inventory strictly proves **200 role definitions** and **200 distinct vetted/exercised skills or MCPs**; no count from filenames, mock servers or untested instructions.
- [ ] Independent security, accessibility, network privacy, battery/resource, 10,000-message load and disaster-recovery checks have inspectable evidence.
- [ ] Release is tied to a fixed, clean and reviewed Git commit, signed/marked as unsigned honestly, hosted CI is green, files/hashes match uploaded public GitHub release assets, and genuine download/install links work.

**Implementation rule:** never mark these checkboxes complete merely because a component, mock, local unit test or one platform passed. Record a source/build/platform-specific observation in the acceptance ledger and link it before closing a requirement. Keep the full 188-row inventory and 23-workstream board; this file fills the explicit master-specification feature/CI traceability gap.
