# Constellation development tools: expanded evidence inventory

**Environment:** Windows development host, October 30, 2026. **Scope:** inspect
local executables, skill metadata and MCP configuration read-only; update
the evidence inventory and its validator. No new skills/MCPs were installed
or executed by this audit.

## Verified observations

The machine-checkable source of this report is
[`skills-manifest/inventory.json`](../../skills-manifest/inventory.json).
Evidence includes observed CLI version responses, executable paths, SHA-256
digests for individual skills and the sibling repository's MIT license.
Version availability does not establish API authentication, safety, tool
permissions, license, platform portability or production suitability.

| Group | Observed evidence | Counts as a vetted Constellation skill/MCP? |
| --- | --- | :---: |
| 8 CLI tools | `node 24.17.0`, `pnpm 12.15.0`, `git 2.54.0.windows.1`, `rg 15.2.0`, `cargo 1.99.1`, `rustc 1.99.1`, `uv 0.12.23`, `gh 2.102.0` | No |
| 42 source-reviewed candidates | 42 individually hashed `SKILL.md` files, matching frontmatter and safety/completion headings observed, and sibling MIT license read | No: source reviewed only |
| 158 user-level skill files | Individually hashed `SKILL.md` files found under existing local user skill directories, with matching frontmatter | No: publisher, license, dependency safety, activation and tools unverified |
| **208 individually recorded entries** | 8 CLIs + 200 distinct skill names, portable schema validation and verified local file fingerprints | **0 qualifying** |

Additional executable/version observations, not counted as manifest entries:
`npm 11.13.0`, `rustup 1.29.2`, `docker 29.8.1`,
`git-lfs 3.7.1`, `codex-cli 0.164.0`, `corepack 0.35.0`,
`pip 22.3.1` (Python 3.10). `python` resolved to a Windows Store
alias that failed with “Python was not found”; an absolute Python
`%USERPROFILE%\AppData\Local\Programs\Python\Python310\python.exe`
returned `Python 3.10.10`. `pytest` was absent from the inspected
default `PATH`. These are command observations, not claims of unusability in
other environments.

## Larger discovery surface, without count inflation

- `%USERPROFILE%\.agents\skills`: **496 SKILL.md files** with **442**
  distinct terminal directory names observed. This is a filesystem count, not
  security review, license audit or execution validation.
- `%USERPROFILE%\.claude\skills`: **386 SKILL.md files**, with **386**
  distinct terminal directory names. Across these two trees, **720 distinct
  directory names** were observed; that is not evidence of 720 unique
  implementations or vetted capabilities.
- `..\vibe-coding-skills\skills`: **42 SKILL.md source files**
  under sibling repo commit `d532b2b52725cbd92b2815fbc98e1e516503137b`.
  The sibling's `LICENSE` contains MIT License terms. All 42 were
  SHA-256-checked with matching name/description and safety/completion
  sections. No sibling files were modified.
- The Constellation project had no `.agents/skills`, `.claude/skills` or
  project-local `skills/` directory at inventory time.
- `codex mcp list` showed `cua_repl` and `node_repl` enabled, and
  `code-review` and `codex_app` disabled. Their functionality,
  transport, permissions, health and licenses **were not exercised or vetted**.
  This observation is intentionally not a counted manifest entry. No
  environment variable values were recorded.
- The sibling repo's `config/mcp.example.json` lists Playwright and
  filesystem MCP *examples*. Examples are not evidence of installation.

## Real gaps before the §14 goal

**Qualifying installed, vetted and exercised skills/MCPs: 0 / 200.**
**Remaining: 200.** Inventory candidates are not installed product
integrations. Before a candidate can qualify, verify its canonical publisher
and license, inspect any scripts/resources and dependency graph, define the
minimal permission and data access boundary, install within a controlled
destination, exercise its real bounded workflow, retain a redacted result log,
and record failures and limitations. Distinctness must be semantic, not just
different labels. MCPs additionally need explicit scope, health, startup,
timeout, cancellation and auth tests. Never execute the large unreviewed user
skill trees merely to satisfy the target count.

The 200 skill file identities are a discovery milestone only. In particular,
the 158 user-level entries use `installed_unvetted` because the files
already exist on this workstation. That status does not establish they are
installed into Constellation, can run with this app's permissions, or have
safe third-party dependencies. The keyword-based candidate filter can include
research or content tools; an individual relevance review is pending.

Use `node skills-manifest/validate.mjs --verify-local` to recheck recorded
source hashes and local license locations, and `node
skills-manifest/refresh-observed-skills.mjs --check` to recheck discovery.
Use
`node --test skills-manifest/validate.test.mjs` to test the validator's
refusal to accept duplicates, fabricated counts and missing evidence.
