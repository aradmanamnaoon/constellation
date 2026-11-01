# Constellation — engineering status

**Checkpoint:** 2026-10-30. **Classification:** experimental local-first desktop application; incomplete against the original 1,237-line master specification and [188-row acceptance ledger](../quality/acceptance-ledger.md). The authoritative, current [23-workstream task board](active-task-plan.md) tracks what has and has not been verified. This checkout is not a production release.

## Working tree and release identity

The connected Windows checkout is at Git HEAD `db7d78a` with extensive intentional, uncommitted source, crate, test and documentation changes. `git remote -v` returned no remote. Earlier installer checksums in [the historical Windows smoke record](../qa/windows-native-smoke.md) refer to distinct binaries at older source states; they are **not** current release artifacts.

Windows native consent has two completed source-pinned candidates. The older `54EBD9F1...` direct EXE and `77797EDC...` unsigned NSIS demonstrated native No/Yes/read/create/no-clobber before read-content hash binding. The subsequent **SHA-bound** direct Windows x64 EXE `1A1C2E60A3E2632FAAFBFB79204670187BFB78FB1A90239074C586F44F63F607` and unsigned NSIS `3219D334F8D4B972BD3FBCE13824AC105ECFB2E298B8897364B13446B249C9A2` passed a five-dialog private-desktop matrix including same-length read-content mutation rejection and 14 correlated journal envelopes. The 11-file frozen source manifest was `FCBCC4A82CE858BDB035BF687EDE6358A8668F822580D57EFD7BBD9AC98A50E1`. **Another concurrent actor changed the executor and other source files after this successful build and GUI run. The frozen artifact remains independently verified, but the newer live worktree is not covered by that result.** See [Windows SHA-bound read QA](../qa/windows-sha-bound-read-private-2026-10-30.md) and [native V2 GUI QA](../qa/windows-final-private-desktop-2026-10-30.md). Neither direct-EXE result proves an installed NSIS lifecycle, signed Git tag or public release.

## Implemented and exercised slices

- Local-first Tauri 2 / Svelte 5 interface with Rust-owned SQLite room, message, role, task and provider state. The core implements durable task transitions, idempotent operations, result transactions, cancellation and interrupted-request recovery.
- Explicit model selection and six provider protocol families; labelled localhost fixtures exercise bounded HTTP and multi-agent text replies. No externally billed model run or full streaming/tool-calling capability test is established.
- **Exactly 200 prompt-only role templates** (42 originals plus 158 distinct expansions), verified by catalogue tests. These descriptions do not grant tools or prove real model capability.
- Native text-task and workflow DAG operations, saved plans, cancellation, Markdown reports and source-grounded Evidence search. Multiple isolated Windows GUI tests cover real local UI flows. A new standalone offline Rust `constellation-agent-loop` library tests strict model proposal parsing, permission-free previews and trusted dispatch handoff, but it is not connected to live models or the native UI and is not an autonomous coding loop.
- The Rust tool executor implements exact-scope approvals, write-once operations, authenticated HMAC journal, Windows Job Object descendant cleanup and Unix process-group test paths. The desktop currently exposes **human-triggered**, pending-task-bound read/create actions inside its private artifacts directory, with independent Win32 consent and one-use proof. SHA-bound read approvals reject changed content, including different contents of exactly the same length; the frozen SHA-bound Windows candidate **passed five actual native dialogs and 14 correlated journal envelopes**. Subsequent unrelated executor edits require a separately rebuilt candidate and fresh tests before closing current-source acceptance. It does not expose Process/Git tool dispatch to models or supply a hardened kernel sandbox.
- Two-database encrypted project archive and isolated recovery, standalone encrypted backup codec and SQLite online backup tests, plus a real private-desktop Windows GUI recovery check. Cross-database snapshots are not globally atomic, and portability/attachment recovery are incomplete.
- Opt-in encrypted S3-compatible sync protocol with strict conditional writes and adverse offline fixtures, along with an authenticated remote-runner pairing/checkpoint **protocol**. No live provider CAS, two-device restore, remote daemon, scheduling or automatic laptop-off execution has been demonstrated.
- A source-/license-pinned development audit inventory. The strict verifier currently recognizes **19 of 200** individually installed/exercised skills; 181 are unqualified, and no runtime MCP server is verified. Do not equate observed user-level files with vetted integrations.
- Windows unsigned NSIS local build and real private GUI tests. A frozen SHA-bound Ubuntu 26.04 WSL2 Linux x86_64 ELF, `.deb` and AppImage were independently packaged from **180 checked source inputs**. Rootless Debian unpack and all three private Xvfb modes (.deb, AppImage extract, AppImage FUSE) passed; pixel comparison showed identical rendered workspaces. The AppImage's pinned cached runtime avoided a new download, but its publisher provenance, signing and host GTK/WebKit compatibility outside Ubuntu remain unverified. See [the new source-pinned Linux record](../qa/linux-sha-bound-read-build-2026-10-30.md); the subsequent live tool-executor changes require revalidation.

## Current repeatable checks

```sh
# From repository root:
node catalog/build-roles.mjs --check
node --test catalog/roles.test.mjs
node skills-manifest/validate.mjs --verify-local
node --test skills-manifest/reviewed-exercise.test.mjs skills-manifest/validate.test.mjs

# Rust crate checks (use C: Cargo target on the Windows machine if D: is full):
cargo test --manifest-path crates/constellation-core/Cargo.toml --offline
cargo test --manifest-path crates/constellation-tools/Cargo.toml --offline
cargo test --manifest-path crates/constellation-agent-loop/Cargo.toml --offline --locked --all-targets
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --offline --lib

# From apps/desktop:
pnpm run check
pnpm run build
```

On 2026-10-30, the catalogue and **4/4** role tests passed; latest strict skills verifier returned **19/200**, and the expanded local audit/forgery suite passed **52/52** after re-exercising stale source-bound receipts. Frontend `pnpm run check` reported zero errors and zero warnings and static build exited 0. The new offline agent-proposal library passed **8/8** integration tests and strict Clippy. The final Ubuntu 26.04 WSL2 source-specific verification passed **47/47** desktop library tests and **42** tool tests (four intentionally ignored fixture entry points), including real Unix process-grandchild regressions. Non-Windows native consent fail-close was source-inspected only, without a real Linux GUI approval attempt. A concurrent change to the executor and other source files means prior frozen binary snapshots **must be revalidated** against a final fixed source revision before publishing.

## Gates preventing a complete release

Full tool-using agent iteration (including approved fail→fix→test), OS-level handle/path race containment, real-provider privacy and billing validation, streaming/failover, **181 additional exercised integrations**, live secure sync and paired runner, large-history/RTL/IME/accessibility QA, macOS Intel/ARM and Android native packaging, source-fresh Windows/Linux installer acceptance, cross-distribution Linux AppImage portability, independent threat review, complete SBOM/license triage, immutable release checksums, hosted green CI, signing and public distribution remain open.

The original [acceptance inventory](../quality/acceptance-ledger.md) deliberately retains unverified entries until each has test platform, binary identity, evidence and reviewer details. No Git commit, remote push, tag or public release has been performed for this latest worktree.
