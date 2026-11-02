# Constellation UX and four-platform release execution plan

**Date:** 2026-11-01 · **Worktree:** `D:\coding\codex\constellation-ux-rebuild` · **Branch:** `ux/competitor-rebuild-20261101` · **Baseline:** `d11ca883`

The status below distinguishes source implementation, native testing and public release acceptance. *Completed* applies only to the described slice. This file is a portable account-independent record of the visible Chat On Steroids plan.

| # | Workstream | Status | Acceptance/next gate |
|---|---|---|---|
| 01 | Protect repository/inspect current code | Completed | Isolated worktree; never overwrite canonical untracked files or production account data |
| 02 | Competitor/skills baseline UX audit | Completed | 11 documented products, 30 patterns, 14 defects, actual skill provenance/qualified counts |
| 03 | Core navigation/task UX changes | Completed, native partial | Searchable palette, rooms, task model isolation, filters and setup recovery; matching current-SHA native UIA |
| 04 | Larger UX transformation | In progress | Command palette, tasks, roles/home, local chat search, safe code blocks and provider search/revision verified. Persisted per-room AI team selection native WebView2 UIA tested, with SQLite audit and stale-write protection. **New narrow removal of an unused, credential-free provider** native UIA tested with persisted deletion audit; refuses any historical task/message, saved workflow, agent binding or remembered credential. Full provider retirement, model migration, long-history, IME and physical keyboard journeys remain open |
| 05 | Final Windows native UX | Completed for installed functional journeys | Source-pinned latest Windows executable/NSIS was rebuilt, literally launched on isolated Win32 desktop, task/palette UIA, 800×560 visual screenshot, backup/restore/workflow and historical SQLite reopen **passed**. Full AT/physical keyboard/real browser zoom remains cross-cutting accessibility workstream 06, not falsely verified here |
| 06 | Automated/security/recovery/a11y | In progress | **141/141** JS checks, zero Svelte diagnostics; latest desktop Rust 130/130, core contract/migration suite plus provider negative cases pass, strict Clippy/format PASS. Native private UIA provider-removal, room/team/command task and earlier focus-return verified; physical keys, true 200% reflow, screen reader/RTL/IME and large-data/adversarial evidence still open |
| 07 | Windows installer | In progress, lifecycle blocked | Latest unsigned NSIS built and exact SHA-256 audited; previous install and actual installed UIA passed before new revision. Live *upgrade/uninstall* requires different Windows SID + HKCU hive: current-user installer may execute older uninstaller, rewrite shared registry/shortcuts or delete app data. Static read-only lifecycle gate PASS; no safe live lifecycle QA on current user |
| 08 | Linux installer and runtime | Completed for local Ubuntu package/runtime | Latest provider-removal-source `.deb` SHA `30525E8F…D400` and `.AppImage` SHA `4364962C…C993` built and passed four isolated native checks: extracted Xvfb GUI both, rootless `dpkg --root --unpack`, and kernel-FUSE AppImage GUI. Pristine Debian 12/Ubuntu 24.04 integration, AT-SPI and provider/recovery parity are still extended open gates under 06/12 |
| 09 | Android installer and runtime | Blocked | Exact API37/AGP9.3.1 Android Gradle copied project built older-JNI x86_64 **debug diagnostic** APK only, not releasable. Native Android consent, OS vault and authenticated checkpoint/rollback support absent; <4GiB free RAM at worker audit prevented safe AVD rerun; no tested signed production ARM64 artifact. See `android-local-qa-2026-11-02.md` |
| 10 | macOS ARM64/Intel local acceptance | Blocked | No accessible macOS machine or VM to perform local Xcode DMG builds, installation, signing/notarization and real GUI QA |
| 11 | Integrate code, release metadata and push | In progress | Reviewed UX branch pushed through `afcd1ff66bf30718821ff839ecdd84ec2f8087fb`, draft PR #1 open. GitHub Actions fail **before any steps** due confirmed account payment/spending-limit block, not an executed code test. New source-pinned provider cleanup implementation and QA require another reviewed commit/push; never force push or overwrite canonical checkout |
| 12 | Complete GitHub release | Blocked | Publish only once all requested OS artifacts are verified, or explicitly revise scope; check GitHub URLs, download hashes and release title |

## Release rule

The user requested locally tested Windows/macOS/Linux/Android deliverables. As of this run, a Windows host cannot itself execute the two macOS architectures, and Android production release packaging/native support remains incomplete. GitHub Actions annotations explicitly report account payment or Actions spending-limit problems; the owner must resolve **Settings → Billing & plans**, after which actual CI can be rerun. Do not create an apparently complete release or relabel a stale/debug artifact to disguise this. Existing `v0.1.2-local-desktop-preview-20261101` remains a separate, clearly marked preview.

## Required exit criteria

Run actual installed app on disposable profiles at least twice; preserve SQLite and tool recovery integrity; use controlled zero-cost model fixtures for network workflows; prove exact source-build hashes; run platform-specific installer, native GUI, accessibility, crash/restart, signing and download checks. Every unsupported or unrun check is recorded explicitly.
