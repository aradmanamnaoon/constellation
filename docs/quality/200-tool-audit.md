# Development skills and MCP milestone — auditable status

**Observed checkpoint: 30 October 2026. Milestone: INCOMPLETE.**

Constellation's original specification requires **exactly 200 distinct,
installed, vetted and genuinely exercised development skills or MCP
integrations**. A role prompt, a CLI executable on PATH, an observed file,
an entry in a manifest or a copied local skill is insufficient. This file
reports the observed evidence without conflating those categories.

| Category | Strict observed count | Qualification |
| --- | ---: | --- |
| Installed/vetted/exercised development audit skills | **19** | Local deterministic exercises with source hashes, license and receipts |
| Source-reviewed skill candidates lacking full qualification | **23** | Excluded until independently exercised/installed/verified |
| Observed skills without sufficient provenance/qualification | **158** | Excluded |
| Runtime MCP integrations with proved installed execution | **0** | No real runtime MCP server passed |
| Separately observed CLI tools | **8** | Excluded from the requested 200 skills/MCP total |
| **Required remaining integrations** | **181** | Each needs an independently verified lifecycle |

There are 208 manifest records including eight explicitly excluded CLI
observations. These records neither imply 208 validated integrations nor prove
200 useful capabilities. The accepted 19 are **local developer audit skills**:
they perform Constellation-specific checks and adversarial fixture exercises.
They are not operational provider, cloud, calendar, browser or filesystem MCP
servers and do not grant agents execution privileges.

## Repeatable strict checks

From the repository root:

```sh
node skills-manifest/refresh-observed-skills.mjs --check
node skills-manifest/validate.mjs --verify-local
node --test skills-manifest/reviewed-exercise.test.mjs skills-manifest/validate.test.mjs
```

At this checkpoint, strict local validation reported `19/200` and the
expanded security/forgery suite **52/52 passed**. The verifier checks local
paths, license attestations, per-skill receipt hashes, independently replayed
project tasks, source pinning, duplicate identities and tampering cases.
It deliberately invalidates previously qualified receipts if source inputs
change. Since Constellation is undergoing concurrent development, always
rerun these commands against the actual current checkout before citing a
strict tally. The developer audit record is
[`docs/qa/skills-expansion-2026-10-30.md`](../qa/skills-expansion-2026-10-30.md).

## Required work per remaining integration

For each remaining distinct candidate, establish a verifiable upstream
publisher, license and pinned source/version; review the exact requested
filesystem/network/secret permissions; install within an approved managed
scope; execute a harmless but meaningful Constellation task with negative
cases; store an independently checkable outcome/receipt; and determine if
the integration may be safely made available to a real agent. A runtime MCP
also needs a real transport handshake, validated schemas, explicit consent,
timeout/cancellation, credential lifecycle and an observed tool response.

Do not install unreviewed external executables, contact authenticated
services, generate synthetic `PASS` receipts, duplicate one canonical source
under multiple IDs or count a prewritten role description as an integration.
The 200-skill/MCP release gate remains open until each item genuinely
qualifies and the full end-to-end security and product tests pass.
