# Roadmap and release blockers

This is a **capability-first backlog**, not a promise of specific deadlines.
Mark features implemented only after tested behavior with actual evidence.

## Proven first vertical slice

- Rust core persistence and default-deny permission *library*.
- Windows native chat and provider contract smoke with a labelled fixture.
- User-selected 1–8 agents for sequential responses, per-agent outcomes,
  duplicate suppression and an explicit network egress decision.
- 200 distinct prompt-only chat-role instructions; no tool permissions conferred.
- Persisted planning/cancellation of tasks, with no execution claim.
- Markdown transcript export as a read-only/copy action.
- Windows local NSIS installer build and installation smoke for the earlier
  milestone; latest source requires a refreshed installed-build acceptance.

## P0 before any public release

1. True file/process/Git **tool runner** with OS isolation, least-privilege
   task grants, handle-safe path enforcement, approval sheet, evidence
   receipts, cancellation and rollback for applicable edits.
2. Real provider credential storage via OS keyring, live compatibility tests
   for supported providers, capability probes, streaming and user-facing
   error/cost/egress controls. External-provider privacy audit.
3. Full no-account onboarding, robust data migrations/integrity, encrypted
   backups and clean uninstall/data retention behavior.
4. Native GUI regression suite across OSes, assistive tech/IME and Android,
   simulated outages/denials, and reproducible installer upgrades.
5. License/SBOM/vulnerability audits, signing, check sums, CI green and
   complete release-specific acceptance ledger.

## Subsequent gates

- True typed workflows, agent-to-agent delegation, concurrency control,
  independent worktree coding, review before publishing, citations/provenance,
  documents and file artifacts with versioned export.
- Authenticated user-owned 24/7 Linux runner, device pairing/revocation,
  always-on scheduler, and tested two-device recovery.
- User-owned E2EE synchronization with hostile-conflict detection and restore.
- Android APK with actually tested mobile-appropriate capabilities and signed
  update continuity; macOS Intel/ARM and Linux packaging with native GUI QA.
- Upgrade the **200 prompt-only templates** into tested operating agents with
  explicit per-role model capabilities, permission contracts, safety tests and
  functional evaluations, plus **exactly 200** installed, licensed, vetted and
  exercised development skills/MCP tools.
  Observed inventory is not the same as qualified installations.
- Repository/public GitHub Release only after every release-blocking gate
  is met or a transparently limited pre-release is independently approved.

The authoritative initial 188-row inventory lives at
[the acceptance ledger](quality/acceptance-ledger.md). Actual narrow Windows
evidence appears in [QA](qa/windows-native-smoke.md).
