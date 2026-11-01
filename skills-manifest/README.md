# Development skill evidence inventory: 200 distinct skill names, 35 installed/exercised records

**Twenty-fifth-wave incremental audit (31 October 2026):** One additional
original source-reviewed MIT skill, `production-readiness-auditor`, was
exact-byte managed-installed and qualified with a dedicated independent
operational-readiness probe, ten real/negative assertions and a source-bound
receipt. The audit correctly identifies blockers to releasing Constellation;
it **does not** certify app readiness, current installed UI, production
health, remote hosting or MCP execution. The existing PowerShell release
preflight was separately observed failing eight of eleven gates, including
uncommitted source and missing external release evidence. The trusted skill
runner does not invoke that preflight because the preflight calls the strict
skill validator and would recurse. Of the original 20 source-reviewed
candidates, 13 now have installed/vetted records and 7 are still only
source-reviewed; 158 other user-level skills remain unvetted.

**At this twenty-fifth-wave working-tree checkpoint, strict local replay
credited only 24/200**, with eleven previously installed receipts stale
against concurrent source modifications. The portable inventory therefore
has 35 installed/vetted declarations while 176 are **still unverified by the
strict current-source gate**. These are different counts. Never use the
recorded installed count as the verified acceptance score. Once all active
source owners finish, rerun `requalify-stable.mjs --check` on a frozen input
set and refresh only stable passing skills before rechecking the score.
See twenty-fifth-wave QA *(unpublished historical local QA evidence)*.

**Twenty-second wave (31 October 2026), observed source snapshot:**
Strict local independent replay accepts **34/200** development skill
qualifications (**166 remaining**), with **zero MCP runtimes**. The newly
reviewed, SHA-pinned MIT skills `root-cause-debugger` and `loop-breaker`
were exact-byte managed-installed and exercised against genuine Constellation
qualification incident evidence, including 20 dedicated negative and positive
assertions. All 34 old/new exercises independently passed stability checks
before an authorized SHA-bound receipt refresh. The full skills Node suite
passed **89/89**; original 20-name source audit still checks all 20,
with 12 promoted and 8 source-reviewed/uninstalled. The 158 user-level
skill entries are unvetted. Source changes can revoke strict credit.
See twenty-second-wave QA *(unpublished historical local QA evidence)*.

**Nineteenth-wave qualification, source-bound status (31 October 2026):**
Two additional original MIT/source-reviewed candidate skills were installed
byte-for-byte and independently exercised: `project-context-recovery` and
`reproduction-builder`. Their distinct `nineteenth-exercise.mjs` receipts
contain real read-only Git/acceptance-ledger recovery and an executable,
repeatable two-record production manifest-validator failure, with adverse
fixtures. **32 entries are recorded as installed/vetted, but only 31/200
currently earn strict local credit (169 remain).** One *older* qualification,
`completion-claims-auditor`, lost credit because
`docs/mission/active-task-plan.md` changed after its signed source snapshot.
Do not treat recorded status as acceptance. The unchanged
`validate.mjs --verify-local` reports this drift and exits nonzero.
Refresh only after concurrent native/runner source freeze, when independent
replay succeeds; then verify the current count again. Ten original candidates
remain source-reviewed/uninstalled, 158 user-level skills are unvetted and
there are zero verified MCP runtimes. See
nineteenth-wave QA *(unpublished historical local QA evidence)*.

**Eighteenth-wave source-bound result (31 October 2026):** The unchanged
strict `node skills-manifest/validate.mjs --verify-local` accepted **30/200**
independently vetted, managed-installed and replayed development skills
(**170 outstanding**). Four additional pinned MIT skills were source-reviewed
and installed byte-for-byte: `multi-agent-integration-reviewer`,
`environment-parity-auditor`, `authentication-security-auditor`, and
`regression-detector`. Their individual SHA-bound
`eighteenth-exercise.mjs` receipts include separate negative fixtures and
limited, transparent Constellation source-contract tests. The original
20-candidate cohort now consists of 8 promoted and 12 source-reviewed-only
skills. None of the 158 unvetted user-level entries or MCP runtimes receives
credit. Re-run strict validation after any concurrent native/frontend source
change. See
eighteenth-wave QA *(unpublished historical local QA evidence)*.

**Seventeenth-wave observed source snapshot (31 October 2026):** Strict
`node skills-manifest/validate.mjs --verify-local` passed with **26/200**
installed, vetted and independently replayed development skill exercises,
**174 remaining**. Four of the original 20 pinned MIT/source-reviewed
candidates were exactly installed, individually exercised and promoted:
`existing-work-preservation`, `architecture-complexity-auditor`,
`dead-feature-detector`, `missing-test-detector`. Each uses a distinct
SHA-bound `seventeenth-exercise.mjs` receipt with adverse fixtures.
The original 20-source cohort remains explicitly audited: 16 uninstalled,
four promoted, zero qualified MCP runtimes; 158 user-level entries remain
unvetted. Other agents may change receipt inputs, so rerun the strict command
before relying on this count. See
the seventeenth-wave audit *(unpublished historical local QA evidence)*.

**Sixteenth-wave local qualification (31 October 2026):** All 22 historical
managed skill installations were individually replayed twice, then each
successful CLI runner produced its own independently checked, current-source
receipt. Four evaluators were repaired to inspect the actual renamed Rust
DDL constant, current milestone status, and palette-token CSS while retaining
working adverse SQLite, status, responsive, and source-tamper controls.
Strict `node skills-manifest/validate.mjs --verify-local` returned exit 0
with **22/200** vetted, installed and exercised at that checkout, **178
remaining**. This is a local source-bound snapshot, not proof of an installed
GUI, external MCP runtime, production security, hosted CI or public release.
Re-run the strict command after any native, frontend, mission, runner or skill
source change: the score must fall when evidence is stale. See
the sixteenth-wave audit *(unpublished historical local QA evidence)*.

The offline refresh script at skills-manifest/requalify-stable.mjs supports
--check (read-only two-run source exercise stability, pinned Git revision and
MIT license, managed installed-file identity, unique receipt path) and
--write (execute each approved passing runner CLI, independently recheck
current results and update only that skill's individual receipt/hash).
The regression tests are in requalify-stable.test.mjs. Failing entries are
never refreshed, and the verifier's strict conditions remain unchanged.
Re-run the script and strict verifier when native/frontend sources have frozen.
The 7 remaining source-reviewed candidates still lack managed installation
and their own separate project exercises; the 158 user-level files remain
unvetted. No MCP runtime is qualified.

**Observed:** 2026-10-30, Windows development host.

The manifest has **208 individually documented records**: 8 observed CLI
executables, 7 remaining source-reviewed skills from a sibling repository,
158 unvetted user-level skill files, and **35 locally installed development
audit skills with independently exercised Constellation-specific receipts**.
There are **200 distinct skill names**. The **current strict qualification
gap** depends on the precise source snapshot; all 34 older receipts matched
at the twenty-second-wave QA snapshot, but subsequent code changes have
staled eleven of them and the twenty-fifth wave added one valid receipt.
No MCP runtime has been qualified and these local development skills do not
grant the Constellation app tool privileges. File presence and a valid schema
do not confer product capabilities or trusted dependencies.

## Validate

Run from the Constellation repository root with Node.js 24+:

```powershell
node skills-manifest/validate.mjs
node skills-manifest/validate.mjs --verify-local
node --test skills-manifest/validate.test.mjs
node skills-manifest/refresh-observed-skills.mjs --check
# Independently audit all twenty original source-reviewed candidate identities:
node skills-manifest/audit-source-candidates.mjs
node --test skills-manifest/audit-source-candidates.test.mjs
node --test skills-manifest/seventeenth-exercise.test.mjs
node skills-manifest/seventeenth-exercise.mjs dead-feature-detector --check
node skills-manifest/qualify-seventeenth.mjs
# Regenerate real exercise receipts after relevant source changes:
node skills-manifest/exercise-local.mjs api-contract-auditor --write-receipt
node skills-manifest/exercise-local.mjs input-validation-auditor --write-receipt
node skills-manifest/exercise-local.mjs documentation-accuracy-auditor --write-receipt
node skills-manifest/exercise-local.mjs dependency-cycle-auditor --write-receipt
node skills-manifest/exercise-local.mjs completion-claims-auditor --write-receipt
node skills-manifest/exercise-local.mjs database-schema-reviewer --write-receipt
node skills-manifest/exercise-local.mjs security-misconfiguration-auditor --write-receipt
node skills-manifest/exercise-local.mjs agent-tool-trust-auditor --write-receipt
node skills-manifest/exercise-local.mjs authorization-boundary-auditor --write-receipt
node skills-manifest/exercise-local.mjs secret-leakage-detector --write-receipt
node skills-manifest/exercise-local.mjs ux-state-completeness-auditor --write-receipt
node skills-manifest/exercise-local.mjs data-consistency-auditor --write-receipt
node skills-manifest/exercise-local.mjs environment-config-auditor --write-receipt
node skills-manifest/exercise-local.mjs cross-platform-readiness-auditor --write-receipt
node skills-manifest/reviewed-exercise.mjs injection-risk-auditor --write-receipt
node skills-manifest/reviewed-exercise.mjs migration-safety-reviewer --write-receipt
node skills-manifest/reviewed-exercise.mjs requirement-traceability-auditor --write-receipt
node skills-manifest/reviewed-exercise.mjs scope-drift-guard --write-receipt
node skills-manifest/reviewed-exercise.mjs dependency-grounding-auditor --write-receipt
node skills-manifest/reviewed-exercise.mjs accessibility-auditor --write-receipt
node skills-manifest/reviewed-exercise.mjs responsive-design-auditor --write-receipt
node skills-manifest/reviewed-exercise.mjs test-quality-auditor --write-receipt
node skills-manifest/qualify-local.mjs
node --test skills-manifest/reviewed-exercise.test.mjs
```

`--verify-local` reads known host paths and checks skill SHA-256, frontmatter,
observed executable locations, and referenced license texts. For the twenty-two
qualified skills it also checks the installed copy against a pinned Git blob,
the MIT license, runner/receipt hashes and unchanged exercised source inputs.
It also independently replays the twenty-two approved local development exercises
against the current Constellation source and compares every assertion with
its receipt; a newly hashed invented receipt cannot earn credit. Additional
skills or MCPs need their own reviewed executable verification before they
can qualify. Eleven exercises inspect bounded source/evidence/configuration/UX
contracts, one runs two real SQLite schemas and constraint checks, and one
independently exercises transaction rollback and stale compare-and-swap writes
using extracted production SQL in in-memory SQLite. The cross-platform
exercise checks real Windows/POSIX path normalization fixtures and CI contracts.
The verifier rejects distinct entries that reuse the same source artifact,
pinned upstream skill or individual exercise receipt, including simple path
aliases. A shared MIT license or verified runner can legitimately serve
separate skills, but one implementation or receipt cannot be counted twice.
Windows CRLF checkout files are compared with the normalized LF Git blob
while their exact installed bytes retain separate SHA-256 hashes. It does not
run third-party installers or contact external services. The default
validation is portable and gives **zero local completion credit**. Running
`--require-target` intentionally **fails** until all 200 skills/MCPs qualify.

The separate read-only `audit-source-candidates.mjs` verifies the 20
remaining sibling-repository candidates against the pinned local Git revision,
MIT license, distinct file identity, SHA-256, required safety sections and
actual managed installation presence. It runs no candidate instructions and
grants **zero qualification credit**, even if an exact file happens to be
installed without a passing Constellation-specific exercise. Its three negative
tests reject forged hashes, switched sources/licenses and traversal paths.
See the 31 October audit *(unpublished historical local QA evidence)*
for the source-change/requalification caveat.

The read-only source discovery command `node
skills-manifest/refresh-observed-skills.mjs --check` confirms that the
manifest matches known source files. The deliberate `--write` option
regenerates the host-specific candidate snapshot from real files, preserving
pre-existing entries, including qualified installations. It does not execute user-level skill instructions or
download packages; the 200 identities are an **inventory**, not 200 adopted
development integrations. Every candidate must still be individually
evaluated for purpose, publisher, license, dependencies, execution,
permissions and semantic uniqueness before promotion.

## State definitions

| State | What is proven | Counts toward 200? |
| --- | --- | :---: |
| `observed_available` (CLI) | A local executable and version responded | No |
| `source_reviewed` (skill) | A source `SKILL.md`, hash and license were checked | No |
| `installed_unvetted` (skill) | A user-level `SKILL.md` exists, with hash; provenance/license or exercises remain unverified | No |
| `configured_unverified` (MCP) | Client configuration observed; live integration unverified | No |
| `installed_vetted` (skill/MCP) | Installed artifact, verified source/license, and concrete passing exercise evidence recorded | Yes |

Entries use unique stable identities (`skill:name`, `mcp:name`, `cli:name`)
and may never count the same underlying skill under multiple categories. A
passing schema check is not a safety review or a runtime integration test.
For a real `installed_vetted` entry, record the tested command, result, date,
installed file fingerprint, pinned upstream revision, license evidence,
permission constraints and remaining limitations. A redacted, hashed
exercise receipt must be under `skills-manifest/evidence/` or the legacy
`docs/dev-tools/evidence/` directory. The original fourteen qualifying receipts are
**bounded static development audits**: Tauri IPC contracts (11 checks),
input validation (7), documentation/QA accuracy (23), and Rust dependency
cycles (7), plus completion-claim evidence metadata (7, including two negative
fixtures for unsupported PASS and duplicate evidence rows), and SQLite schema
constraints (16, including in-memory execution of real Rust-authored SQL and
two negative mutation fixtures), and desktop security configuration (10,
including unsafe CSP and shell capability negative fixtures).
Four more distinct exercises cover native agent-tool trust (10 checks;
consent, proof and YES-origin bypass fixtures), task-scoped authorization
(10 checks; stale task, preflight and excessive-grant fixtures), bounded
credential leak detection (7 checks; redacted synthetic token and key fixtures),
and file-action UI state handling (10 checks; duplicate-submit, missing-alert
and stale-preview fixtures). Three additional audits exercise SQL transaction
and revision consistency (5 checks), environment and credential configuration
(8 checks), and cross-platform process/CI assumptions (8 checks). The new
exercises include separate regression fixtures for duplicate events, stale
concurrent snapshots, unsafe interface binds, missing credentials, broken
Windows/Unix process distinctions and removed macOS CI coverage. They are real
executed checks of current Constellation source,
but **they do not verify deployed GUI behavior, live MCPs, or external model
tool permissions**. The token scan is limited to selected public desktop
configuration. Native consent cannot prove physical input against privileged
OS window automation. The cross-platform audit did not run macOS/Linux
executables or hosted CI. Human review is still necessary to judge completeness.

Eight separately pinned exercises use the independent
`skills-manifest/reviewed-exercise.mjs` runner. The SQL injection audit
executes a Rust-extracted parameterized SQLite query with a synthetic malicious
ID and an unsafe interpolation control (6 checks). The migration review runs
production workflow DDL, tests SQLite forward/rollback, preserves payload bytes
and rejects invalid revisions (7 checks). The requirements audit parses all
188 platform-scoped rows and rejects missing entries, unsupported PASS and
duplicate keys (8 checks). The scope guard applies actual ownership rules to
in-scope, unauthorized and traversal path fixtures (5 checks). The dependency
grounding audit reconciles desktop npm dependency declarations with pnpm
resolutions, two installed package metadata files, and four Rust crates with
Cargo.lock (11 checks), including invented-version and stale-lock fixtures.
Three additional distinct audits check actual Svelte navigation/keyboard/alert semantics and CSS focus/reduced-motion contracts (6 checks), mobile viewport breakpoint/reflow/hidden-navigation CSS (7 checks), and meaningful negative assertions in native file-tool Rust tests (7 checks). Companion Node mutation fixtures reject removed accessible names/alerts/focus, broken narrow grids/keyboard navigation, and weakened anti-replay/confidentiality/proof tests. All three remain **source-only** audits: no actual desktop browser/screen reader/viewport render or native Rust test run is implied by these receipts. Their runner,
receipts and input hashes are independently replayed by strict verification.
These are bounded development checks: no user workspace, installed GUI,
live runtime MCP, external endpoint, or upstream publisher identity was verified.

The current local paths are host-specific by design. The manifest uses
`%USERPROFILE%` and `%APPDATA%` placeholders instead of storing an account
name. Twenty-two skill definitions from the clean sibling
`../vibe-coding-skills` checkout at commit
`d532b2b52725cbd92b2815fbc98e1e516503137b` were copied byte-for-byte
to the user-managed `/skills/constellation/` directory, with the exact
MIT license. That upstream revision and local hashes are verified; **the
upstream publisher's identity was not independently authenticated**.
The 158 remaining user-level skill licenses are unverified. Candidate
selection uses technical keywords and may include unrelated research/content
skills; no relevance or integration is assumed from a matching name.

## Remaining qualification work

There are **166 not currently qualified skill records** at the twenty-second-wave
source snapshot and **zero verified MCP runtime integrations**. The 8 remaining
`source_reviewed` skills are present in the sibling source checkout, but
this alone does not establish installation. The 158
`installed_unvetted` user-level files lack verified license/provenance and
project-specific execution evidence. Neither group earns credit merely by
having a distinct name or a matching source hash.

To advance the milestone, first establish the publisher, pinned immutable
revision, actual managed installation, applicable redistribution/use license,
reviewed dependencies and narrow permissions for each distinct capability.
Next implement a reproducible Constellation-specific exercise that tests its
claimed operation and important failure cases, with exact source/runner hashes
and a replayable evidence receipt. Extend the trusted verifier and negative
tests for each exercise family. For an MCP, additionally verify that the
actual configured server starts, negotiates its protocol and exposes the
intended operation through the application's permission boundary, with any
credentials redacted. Do not count a documented MCP configuration as a live
integration. Independent functionality is a separate human review: similarly
named files or wrappers around the same operation should not inflate totals.

The Rust/native/frontend/docs/Cargo tree is shared with other workers. Source
hashes in these receipts deliberately fail when those inputs change. Once
the active implementation settles, rerun `requalify-stable.mjs --check` and
`--write` only for independently stable, passing source exercises,
then `validate.mjs --verify-local` and the test suite before reporting
the final count. `--require-target` must continue to
fail until all 200 are demonstrably qualified.

See the original 30 October strict audit *(unpublished historical local QA evidence)*
for the earlier baseline and blockers; use `validate.mjs --verify-local` for
the authoritative current local count and per-skill receipts under `evidence/`.
