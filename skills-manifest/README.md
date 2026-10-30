# Development skill evidence inventory: 200 observed candidates, 0 exercised integrations

**Observed:** 2026-10-30, Windows development host.

The manifest has **208 individually documented records**: 8 available CLI
executables, 42 source-reviewed skills from a sibling repository, and 158
user-level skill files whose SHA-256 and frontmatter were locally checked.
There are **200 distinct skill names**, but **0 of the requested 200
installed, vetted and exercised Constellation skills/MCPs**. No MCP runtime
has been evaluated. File presence and a valid schema do not confer product
capabilities, canonical provenance, trusted dependencies or safe permissions.

## Validate

Run from the Constellation repository root with Node.js 24+:

```powershell
node skills-manifest/validate.mjs
node skills-manifest/validate.mjs --verify-local
node --test skills-manifest/validate.test.mjs
node skills-manifest/refresh-observed-skills.mjs --check
```

`--verify-local` reads known host paths and checks skill SHA-256, frontmatter,
observed executable locations, and referenced license texts. It does not run
or install third-party packages or contact external services. The default
validation is portable and checks the evidence schema without depending on
this machine. Running `--require-target` intentionally **fails** until 200
distinct installed and exercised skills/MCPs have qualifying evidence.

The read-only source discovery command `node
skills-manifest/refresh-observed-skills.mjs --check` confirms that the
manifest matches known source files. The deliberate `--write` option
regenerates the host-specific candidate snapshot from real files, preserving
pre-existing entries. It does not execute user-level skill instructions or
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
For a real `installed_vetted` entry, record the tested command, observed
result, date, file fingerprint, relevant permission constraints and remaining
limitations. Its runtime exercise receipt must be a redacted file under
`docs/dev-tools/evidence/`, with a SHA-256 stored in `exercise.receipt_sha256`.
`--verify-local` checks that file's hash and is mandatory for counted
`installed_vetted` entries; portable validation gives those entries zero
completion credit. Human review remains necessary to confirm the exercise
tested the intended behavior and the receipt is authentic.

The current local paths are host-specific by design. The manifest uses the
`%USERPROFILE%` placeholder instead of storing the developer account name.
The sibling `../vibe-coding-skills` tree was inspected read-only and was not
installed into Constellation. Its local MIT license is observed; this alone
does not prove upstream publisher authenticity. User-level skill file
licenses remain unverified. Current script selection considers technical
keywords and can include adjacent research/content skills; selection is not
a claim that every candidate is relevant to Constellation.
