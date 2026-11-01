# Constellation UX and four-platform release execution plan

**Date:** 2026-11-01 · **Worktree:** `D:\coding\codex\constellation-ux-rebuild` · **Branch:** `ux/competitor-rebuild-20261101` · **Baseline:** `d11ca883`

The status below distinguishes source implementation, native testing and public release acceptance. *Completed* applies only to the described slice. This file is a portable account-independent record of the visible Chat On Steroids plan.

| # | Workstream | Status | Acceptance/next gate |
|---|---|---|---|
| 01 | Protect repository/inspect current code | Completed | Isolated worktree; never overwrite canonical untracked files or production account data |
| 02 | Competitor/skills baseline UX audit | Completed | 11 documented products, 30 patterns, 14 defects, actual skill provenance/qualified counts |
| 03 | Core navigation/task UX changes | Completed, native partial | Searchable palette, rooms, task model isolation, filters and setup recovery; matching current-SHA native UIA |
| 04 | Larger UX transformation | In progress | Screen density, roles, home dashboard, evidence/history, full task workflows, forms, responsive behavior and all controls |
| 05 | Final Windows native UX | In progress | Installed launch, actual UIA typed search and strict historic SQLite restart **passed**; physical keyboard, zoom and broader end-to-end tasks still open |
| 06 | Automated/security/recovery/a11y | In progress | Full frontend/Rust checks, source and skill gates, permission negatives, reliable visual/screen-reader and large-data evidence |
| 07 | Windows installer | In progress | Native signed/unsigned status, isolated real NSIS install, installed UIA and exact bytes, upgrade/uninstall |
| 08 | Linux installer and runtime | In progress | Final-source local WSL2 native .deb and AppImage **both built and rendered in isolated Xvfb**; clean distro installation and accessibility gates still open |
| 09 | Android installer and runtime | Blocked | Android x86_64 Rust library compiled; Gradle 9.3.1 resolution/symlink privilege and native consent blockers; x86_64 QA AVD created |
| 10 | macOS ARM64/Intel local acceptance | Blocked | No accessible macOS machine or VM to perform local Xcode DMG builds, installation, signing/notarization and real GUI QA |
| 11 | Integrate code, release metadata and push | Pending | Review branch against current main; all relevant acceptance gates, honest version, signing, checksums and CI results |
| 12 | Complete GitHub release | Blocked | Publish only once all requested OS artifacts are verified, or explicitly revise scope; check GitHub URLs, download hashes and release title |

## Release rule

The user requested locally tested Windows/macOS/Linux/Android deliverables. As of this run, a Windows host cannot itself execute the two macOS architectures, and Android release packaging/native support remains incomplete. Do not create an apparently complete release or relabel a stale/debug artifact to disguise this. Existing `v0.1.2-local-desktop-preview-20261101` remains a separate, clearly marked preview.

## Required exit criteria

Run actual installed app on disposable profiles at least twice; preserve SQLite and tool recovery integrity; use controlled zero-cost model fixtures for network workflows; prove exact source-build hashes; run platform-specific installer, native GUI, accessibility, crash/restart, signing and download checks. Every unsupported or unrun check is recorded explicitly.
