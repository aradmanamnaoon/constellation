# Initial acceptance and evidence ledger

**Snapshot:** 2026-10-30. **Every row starts `UNVERIFIED`.** This is a requirements inventory, not an implementation report. It is based on the user's 1,237-line Constellation master specification (§§1–32), with the exact GUI matrix from §23 and final deliverables from §30. No app build, runtime test, CI run, packaging, signing, hosted release, credential or successful platform validation is asserted by this file.

## Rules for evidence and updates

- Every test row uses the required fields: `requirement_id`, `platform`, `build_sha`, `environment`, `test_type`, `automated/manual`, `observed_result`, `screenshot_or_log`, `verified_by`, `timestamp`, `blocker`. `requirement` is an added human-readable field.
- `UNVERIFIED` means no direct admissible observation is linked here. Use `PASS`, `FAIL`, `PARTIAL` or `BLOCKED` only after observing and recording evidence; never infer `PASS` from code inspection, mocks, a build alone, a worker assertion, or a green unrelated CI check.
- A native installed GUI result needs the actual installer/APK source, exact app build SHA, OS/device or VM version, interaction procedure, screenshot/video/automation log, outcome and the tester. A hosted build that compiles but does not launch stays `UNVERIFIED` for GUI behavior.
- For a row covering multiple platforms, split into one row per real target before marking platform-specific `PASS`. A row can be `PASS` only for the precise platform and modality exercised. Keep a failing regression visible; do not overwrite historical failures.
- Separate `DOC` research and `SPIKE` prototypes from `OBSERVED` installed-app verification. Example links can use `docs/qa/evidence/<build-sha>/<platform>/<test-id>/...` when files exist; do not fabricate them.
- Initial acceptance gates include exactly **200 substantive role templates** and **200 distinct vetted skills/MCP entries**, requiring a machine-readable manifest and real exercise/limitations; counting scaffolds is not proof.
- Refer to [provisional framework ADR](../architecture-decision.md) and [initial threat model](../security/initial-threat-model.md). The specification also calls for `docs/qa/acceptance-ledger.md`; a later integration task should either move this ledger there or make that path a discoverable link, without creating inconsistent duplicate status records.

**Additional 2026-10-31 source-only observations (not upgrades to any
cross-platform or installed-GUI acceptance row):** Prime independently
verified Core 30/30 offline tests for exact SQLite v1 plus budget/Evidence
optional extensions; native Rust 81/81 tests including source-only saved
Evidence, encrypted archive recovery, rights-gated PDF/DOCX preview and
five host-only HMAC checkpoint negative/race tests; Evidence 28/28 offline
tests with actual restricted PDF/DOCX fixtures; 24 protocol and 8 runner
service tests, and 14 palette/plan Node tests. Native/Svelte check and build,
strict native/Core/Evidence Clippy and fmt gates pass at those worktree
checkpoints. Source paths, commands and limitations in
budget QA *(unpublished historical local QA evidence)*,
saved Evidence *(unpublished historical local QA evidence)*,
PDF/DOCX preview *(unpublished historical local QA evidence)*,
host checkpoint *(unpublished historical local QA evidence)*,
runner *(unpublished historical local QA evidence)* and
CSS *(unpublished historical local QA evidence)*.
**All 188 platform rows below remain UNVERIFIED** until direct, source-pinned
installed application, platform and hosted observations meet each row's
required modality. No Git remote, push or public release was verified.

**Subsequent ninth/tenth-wave source evidence (same uncommitted Windows
working tree):** Native Tauri Rust tests **89/89 PASS** including real Core
pending-task revocation, one-time Windows Credential Manager checkpoint key,
durable concurrent independent workflow completion, paired encrypted archive
reference checks, and explicit hashed Evidence source deletion/export.
Tool crate independent offline Windows tests and strict Clippy **PASS** with
approved cwd OS identity pinning; Sync crate **21/21 PASS**, strict Clippy/fmt,
with bounded trusted-checkpoint recovery and session-local revocation;
Workflow **30/30 PASS** and Project Backup **14/14 PASS**. Svelte typecheck
zero errors/warnings, production build PASS; palette/plan **14/14 PASS**.
The latest final native strict Clippy initially found an unused borrow in the
new Evidence export and was corrected; a follow-up lint/fmt run is required
before claiming a clean complete source revision. All observations are limited
to exact test scopes, not real deployed multi-agent OS containment, physical
two-device cloud CAS or accessible installed GUI. See
Prime native integration *(unpublished historical local QA evidence)*,
Tools *(unpublished historical local QA evidence)*, and
Sync *(unpublished historical local QA evidence)*. Platform rows remain
UNVERIFIED; release remains blocked.

## Core functionality, security, performance, CI and release

| requirement_id | requirement | platform | build_sha | environment | test_type | automated/manual | observed_result | screenshot_or_log | verified_by | timestamp | blocker |
|---|---|---|---|---|---|---|---|---|---|---|---|
| CORE-01 | Accountless local workspace | Cross-platform | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| CORE-02 | Rust authoritative domain and permission engine | Cross-platform | — | — | code/integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| CORE-03 | Typed versioned command/event contracts | Cross-platform | — | — | contract | — | **UNVERIFIED** | — | — | — | No linked observation |
| CORE-04 | Persistent SQLite migrations/integrity | Cross-platform | — | — | integration/chaos | — | **UNVERIFIED** | — | — | — | No linked observation |
| CORE-05 | Durable task state machine and event IDs | Cross-platform | — | — | unit/integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| CORE-06 | Task failure/partial/success status grounded in receipts | Cross-platform | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| CORE-07 | Crash-safe draft and selection restore | Cross-platform | — | — | chaos/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-01 | Custom brand/icons with asset licenses | Desktop+Android | — | — | review | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-02 | Calm meaningful no-provider empty state | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-03 | First task within three primary decisions | Desktop+Android | — | — | usability | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-04 | Five main destinations and Advanced area | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-05 | Accessible command palette and shortcuts | Desktop | — | — | keyboard/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-06 | Light/dark/high-contrast and reduced motion | Desktop+Android | — | — | accessibility | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-07 | Readable 900x600 and 200% scaling | Desktop | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-08 | Persian RTL/CJK IME composition and Unicode paths | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-09 | Virtualized 500 messages, stable scroll | Desktop+Android | — | — | performance/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-10 | 10k-message room responsive, bounded DOM | Desktop+Android | — | — | load/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-11 | Text selection/copy/markdown links | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-12 | No unintended autoscroll while reading older items | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-13 | Draft survives pane switch/restart/reconnect | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-14 | Negative UX states explain changes and recovery | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-15 | Original UX research and two UI directions | Desktop+Android | — | — | research/prototype | — | **UNVERIFIED** | — | — | — | No linked observation |
| UX-16 | Actual accessibility: keyboard and screen readers | Desktop+Android | — | — | accessibility | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-01 | Remote API provider and live request | Desktop+Android | — | — | integration/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-02 | Local model endpoint and local network option | Desktop+Android | — | — | integration/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-03 | Custom OpenAI-compatible endpoint with validation | Desktop+Android | — | — | contract/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-04 | Provider adapter capability probe truthful | Cross-platform | — | — | contract | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-05 | Streaming text, errors, timeout, reconnect | Cross-platform | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-06 | Tool calling only for capable models | Cross-platform | — | — | contract | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-07 | Provider routing fallback/limits | Cross-platform | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-08 | No credentials in UI logs or exports | Cross-platform | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-09 | Egress consent, provider disclosures and budget | Desktop+Android | — | — | gui/security | — | **UNVERIFIED** | — | — | — | No linked observation |
| PROV-10 | Invalid key, revoked key and rate limit UX | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-01 | DM chat and persistent messages | Desktop+Android | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-02 | Multi-agent rooms/mentions/replies | Desktop+Android | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-03 | Agent attribution and activity timeline | Desktop+Android | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-04 | Delegation and independent parallel steps | Cross-platform | — | — | integration/e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-05 | Researcher→writer→reviewer shared task | Cross-platform | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-06 | Multi-coder isolated diffs/conflicts | Desktop | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-07 | Durable DAG workflow, retry/idempotence | Cross-platform | — | — | integration/chaos | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-08 | Interrupt, cancellation and resume checkpoints | Cross-platform | — | — | chaos/e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROOM-09 | No delegation permission laundering | Cross-platform | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROLES-01 | Exactly 200 substantive role definitions | Cross-platform | — | — | schema/inventory | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROLES-02 | Search, filter, customize role | Desktop+Android | — | — | gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| ROLES-03 | Role limits/tools/capability actually effective | Cross-platform | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| INTEG-01 | Exactly 200 distinct vetted dev skills/MCP integrations | Cross-platform | — | — | inventory/evidence | — | **UNVERIFIED** | — | — | — | No linked observation |
| INTEG-02 | Manifest provenance/sha/license/duplicate audit | Cross-platform | — | — | security/review | — | **UNVERIFIED** | — | — | — | No linked observation |
| INTEG-03 | MCP stdio and HTTP conformance | Cross-platform | — | — | contract | — | **UNVERIFIED** | — | — | — | No linked observation |
| INTEG-04 | MCP permission and process isolation | Cross-platform | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| INTEG-05 | Skill install/update/disable/revoke lifecycle | Cross-platform | — | — | integration/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| INTEG-06 | ACP interoperability where claimed | Cross-platform | — | — | contract | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-01 | Scoped read/write filesystem tools | Desktop/Runner | — | — | integration/security | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-02 | Safe process execution and timeouts | Desktop/Runner | — | — | integration/security | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-03 | Git diff/review/apply reversible edits | Desktop/Runner | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-04 | Fixture repo fail→fix→passing tests | Desktop/Runner | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-05 | Explicit file/diff/command approval UX | Desktop/Runner | — | — | gui/security | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-06 | Approval bound to immutable action/context | Cross-platform | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-07 | Browser automation only when supported/approved | Desktop/Runner | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-08 | Artifact SHA256 and source provenance | Cross-platform | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-09 | Document ingestion/RAG source evidence | Cross-platform | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| TOOL-10 | Export Markdown/JSON artifacts | Desktop+Android | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| REMOTE-01 | Explicit local/phone/paired target capability | Cross-platform | — | — | contract/gui | — | **UNVERIFIED** | — | — | — | No linked observation |
| REMOTE-02 | User-owned Linux runner service install | Linux VM | — | — | installation | — | **UNVERIFIED** | — | — | — | No linked observation |
| REMOTE-03 | Authenticated pair/approve/revoke runner | Desktop+Android+Runner | — | — | security/e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| REMOTE-04 | Runner survives laptop UI shutdown | Linux VM | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| REMOTE-05 | Offline/reconnect replay-safe remote tasks | Cross-platform | — | — | chaos | — | **UNVERIFIED** | — | — | — | No linked observation |
| REMOTE-06 | Runner scheduled jobs with quotas | Linux VM | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| SYNC-01 | User-owned S3/R2 provider integration | Cross-platform | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| SYNC-02 | Encrypted local backup and tested restore | Cross-platform | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| SYNC-03 | E2EE authenticated encryption and key rotation | Cross-platform | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| SYNC-04 | Provider write preconditions/CAS verified | Cross-platform | — | — | integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| SYNC-05 | Two writer conflict with non-destructive recovery | Cross-platform | — | — | chaos/e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| SYNC-06 | Tamper, rollback and corrupted object rejection | Cross-platform | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| SYNC-07 | Missing/revoked recovery key fail closed | Cross-platform | — | — | chaos | — | **UNVERIFIED** | — | — | — | No linked observation |
| ANDROID-01 | Phone-safe file/document picker actions | Android | — | — | gui/integration | — | **UNVERIFIED** | — | — | — | No linked observation |
| ANDROID-02 | No unrestricted local shell/process | Android | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| ANDROID-03 | Background restrictions honored | Android | — | — | lifecycle | — | **UNVERIFIED** | — | — | — | No linked observation |
| ANDROID-04 | Pair, monitor, approve, cancel VM jobs | Android | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| ANDROID-05 | Signed APK upgrade/install | Android | — | — | packaging | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-01 | Prompt injection in PDF/web/repo ignored as authority | Cross-platform | — | — | adversarial | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-02 | MCP tool description and package spoofing denied | Cross-platform | — | — | adversarial | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-03 | Cross-agent delegation privilege escalation denied | Cross-platform | — | — | adversarial | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-04 | Changed workspace/target stale approval denied | Cross-platform | — | — | adversarial | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-05 | Path traversal/symlink/reparse escape denied | Desktop/Runner | — | — | fuzz/security | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-06 | Structured shell args and output bound | Desktop/Runner | — | — | fuzz/security | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-07 | SSRF to metadata/loopback/private endpoints denied | Cross-platform | — | — | adversarial | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-08 | Network egress scope and redirect validation | Cross-platform | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-09 | Secrets redacted in logs/chat/screenshots | Cross-platform | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-10 | Runner MITM/replay/revocation tests | Cross-platform | — | — | adversarial | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-11 | Malicious skill/role/workflow import quarantine | Cross-platform | — | — | adversarial | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-12 | Agent loop/task/time/cost bounds enforced | Cross-platform | — | — | load/security | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-13 | Tauri CSP/capability scopes minimized | Desktop+Android | — | — | audit | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-14 | Android deep link/exported component hardening | Android | — | — | security | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-15 | Supply-chain audit, lockfiles, SBOM, license scan | Cross-platform | — | — | security/review | — | **UNVERIFIED** | — | — | — | No linked observation |
| SEC-16 | Opt-in diagnostics and local telemetry off | Cross-platform | — | — | privacy | — | **UNVERIFIED** | — | — | — | No linked observation |
| PERF-01 | Cold launch to interactive under 3 seconds target | Desktop | — | — | benchmark | — | **UNVERIFIED** | — | — | — | No linked observation |
| PERF-02 | Typing responsive under parallel streaming | Desktop+Android | — | — | benchmark | — | **UNVERIFIED** | — | — | — | No linked observation |
| PERF-03 | Measured idle CPU/RAM/battery reference baseline | Desktop+Android | — | — | benchmark | — | **UNVERIFIED** | — | — | — | No linked observation |
| PERF-04 | Buffers, queues, outputs, previews bounded | Cross-platform | — | — | load | — | **UNVERIFIED** | — | — | — | No linked observation |
| PERF-05 | Disk full, process crash, migration rollback | Cross-platform | — | — | chaos | — | **UNVERIFIED** | — | — | — | No linked observation |
| CI-01 | cargo fmt/clippy/workspace test all relevant platforms | Windows+macOS+Linux | — | — | CI | — | **UNVERIFIED** | — | — | — | No linked observation |
| CI-02 | Frontend typecheck/lint/unit/a11y checks | Desktop+Android | — | — | CI | — | **UNVERIFIED** | — | — | — | No linked observation |
| CI-03 | Provider/MCP/ACP/schema contracts fixture matrix | Cross-platform | — | — | CI | — | **UNVERIFIED** | — | — | — | No linked observation |
| CI-04 | Supply chain and secret/history scan | Cross-platform | — | — | CI | — | **UNVERIFIED** | — | — | — | No linked observation |
| CI-05 | Android SDK/NDK build runner | Android | — | — | CI | — | **UNVERIFIED** | — | — | — | No linked observation |
| CI-06 | Native installed GUI negative-path tests | Desktop+Android | — | — | CI/native | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-01 | Windows NSIS installer | Windows | — | — | package/install | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-02 | macOS arm64 DMG | macOS-arm64 | — | — | package/install | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-03 | macOS x64 DMG | macOS-x64 | — | — | package/install | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-04 | Linux AppImage | Linux | — | — | package/install | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-05 | Linux DEB | Linux | — | — | package/install | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-06 | Signed Android release APK | Android | — | — | package/install | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-07 | Linux runner downloadable binary | Linux VM | — | — | package/install | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-08 | Checksums, tag, provenance/SBOM and license list | Cross-platform | — | — | review | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-09 | Release artifact re-download, install and launch | All targets | — | — | e2e | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-10 | Public GitHub tag/release URL and functional links | All targets | — | — | review | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-11 | README real screenshots and truthful install links | All targets | — | — | review | — | **UNVERIFIED** | — | — | — | No linked observation |
| SHIP-12 | Repo history under 500 MB with no secrets | Cross-platform | — | — | review | — | **UNVERIFIED** | — | — | — | No linked observation |

## Required installed GUI matrix

The following 70 rows record the **14 required workflows × five build targets** from specification §23. On Android, GUI-07/08 mean the Android-safe subset and GUI-12 means TalkBack/touch; an APK build alone does not pass a phone GUI test.

| requirement_id | requirement | platform | build_sha | environment | test_type | automated/manual | observed_result | screenshot_or_log | verified_by | timestamp | blocker |
|---|---|---|---|---|---|---|---|---|---|---|---|
| GUI-01 | Install, launch, uninstall or APK upgrade | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-01 | Install, launch, uninstall or APK upgrade | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-01 | Install, launch, uninstall or APK upgrade | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-01 | Install, launch, uninstall or APK upgrade | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-01 | Install, launch, uninstall or APK upgrade | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-02 | Fresh start without provider | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-02 | Fresh start without provider | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-02 | Fresh start without provider | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-02 | Fresh start without provider | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-02 | Fresh start without provider | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-03 | Add provider and test capability | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-03 | Add provider and test capability | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-03 | Add provider and test capability | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-03 | Add provider and test capability | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-03 | Add provider and test capability | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-04 | DM, room chat and mentions | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-04 | DM, room chat and mentions | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-04 | DM, room chat and mentions | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-04 | DM, room chat and mentions | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-04 | DM, room chat and mentions | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-05 | Roles gallery, filter and create | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-05 | Roles gallery, filter and create | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-05 | Roles gallery, filter and create | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-05 | Roles gallery, filter and create | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-05 | Roles gallery, filter and create | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-06 | Workflow task approval and cancellation | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-06 | Workflow task approval and cancellation | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-06 | Workflow task approval and cancellation | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-06 | Workflow task approval and cancellation | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-06 | Workflow task approval and cancellation | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-07 | File preview, artifact and export | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-07 | File preview, artifact and export | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-07 | File preview, artifact and export | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-07 | File preview, artifact and export | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-07 | File preview, artifact and export | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-08 | Tool execution with platform-safe limits | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-08 | Tool execution with platform-safe limits | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-08 | Tool execution with platform-safe limits | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-08 | Tool execution with platform-safe limits | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-08 | Tool execution with platform-safe limits | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-09 | Restart and resume state | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-09 | Restart and resume state | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-09 | Restart and resume state | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-09 | Restart and resume state | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-09 | Restart and resume state | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-10 | Pair, monitor and revoke runner | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-10 | Pair, monitor and revoke runner | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-10 | Pair, monitor and revoke runner | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-10 | Pair, monitor and revoke runner | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-10 | Pair, monitor and revoke runner | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-11 | Sync/restore/conflict UX | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-11 | Sync/restore/conflict UX | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-11 | Sync/restore/conflict UX | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-11 | Sync/restore/conflict UX | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-11 | Sync/restore/conflict UX | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-12 | Keyboard/screen reader or TalkBack/touch | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-12 | Keyboard/screen reader or TalkBack/touch | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-12 | Keyboard/screen reader or TalkBack/touch | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-12 | Keyboard/screen reader or TalkBack/touch | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-12 | Keyboard/screen reader or TalkBack/touch | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-13 | High DPI, small window and larger font | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-13 | High DPI, small window and larger font | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-13 | High DPI, small window and larger font | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-13 | High DPI, small window and larger font | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-13 | High DPI, small window and larger font | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-14 | Dark/light and loss of network | Windows | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-14 | Dark/light and loss of network | macOS-x64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-14 | Dark/light and loss of network | macOS-arm64 | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-14 | Dark/light and loss of network | Linux | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |
| GUI-14 | Dark/light and loss of network | Android | — | — | installed GUI | — | **UNVERIFIED** | — | — | — | No native installed test |

## Release decision

The **initial ledger rows above** are intentionally the untouched initial gate
inventory. A subsequent real *Windows development-build smoke* did establish
working room creation, agent creation, message persistence across app restart,
and a **localhost deterministic fake-provider HTTP contract reply**. Those
observations, screenshots and their limits are in
[the dated Windows smoke record](../qa/windows-native-smoke.md). This does **not**
turn any multi-platform, full-workflow release-wide gate above into a PASS.

Current evidence still supports **no release-wide PASS assertions**. A public
release, if one is eventually made, must link checksums and commit SHA and
disclose any incomplete targets. A successful compilation is distinct from
`installed and launched`, `GUI flow tested`, `signed`, `notarized` and `released`.
