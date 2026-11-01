# Prompt-only role catalogue: 200 distinct specialties

`roles.json` contains **200 distinct planning/chat roles**: 42
originals preserved in `roles.foundation.json` and 158 individually
specified specialties in `roles-expansion.psv`. Each defines a
particular problem, method, evidence expectations and deliverable. The value of its
`status` field is intentionally `prompt_only_no_tool_runner`.

These definitions do **not** install tools, run code, grant permissions, pair
machines, validate provider capabilities, or constitute the requested 200
fully functional, installed or exercised agents. The desktop role picker
currently exposes a smaller starter set. Product integration still requires
the Rust permission, agent capability and provider-routing systems.

Validate the manifest with:

```sh
node catalog/build-roles.mjs --check
node --test catalog/roles.test.mjs
```

## Offline prompt-contract evaluation (eleventh wave)

The optional `role-evaluation.mjs` module composes a **trusted advisory system
message** for a canonical role ID, followed by separately encoded, untrusted
user tasks and external-source messages. It always includes the role-specific
method, required level-2 Markdown deliverable sections and a shared reminder
that native user consent and execution permissions are independent from text.
The catalogue's role data is frozen in memory; imported, unknown IDs cannot
select a privileged role. `composeRoleMessages()` is an offline building block,
not yet wired into the native model orchestration path.

Run all original and new deterministic tests and the human-readable JSON
evaluation report without credentials or additional packages:

```sh
node catalog/build-roles.mjs --check
node --test catalog/roles.test.mjs tests/role-evaluation-eleventh-wave.test.mjs
node catalog/evaluate-roles.mjs
```

The evaluator compares the **role-specific** methods of all 200 definitions
(excluding repeated generic builder boilerplate) using lexical Jaccard
similarity, checks generated prompt section contracts, and validates 10
human-written reference answer fixtures spanning all categories. It also
exercises malicious user/retrieval/MCP text confined to untrusted message
slots, detects five known classes of unsafe candidate claims, and rejects
malformed/broad/untrusted role selections. Builder tests regenerate a
temporary manifest from modified specialization source and prove stale or
malformed input fails. No catalogue source or generated `roles.json` is edited
by these tests.

These checks prove **only deterministic prompt structure and fixture
classification**. They cannot prove that any model follows the policy,
produces a correct specialty answer, resists semantic prompt injection, has
access to tools, or prevents a real privilege escalation. The simple
regex-based candidate-response checks may produce false positives and false
negatives. A genuine evaluation requires separately approved provider calls,
actual model transcripts, scored answer quality, independently enforced Core
and native permissions, negative tool tests and source-pinned GUI evidence.

## Real-provider evaluation harness (twentieth wave)

The opt-in `run-role-provider-evaluation.mjs` extends static preflight with
**reproducible behavioral challenges**. Every canonical role gets a normal
scenario and the same scenario with a hostile imported source: **200 × 2 =
400 generated challenge contracts**, not 400 provider evaluations. Each
includes two role-specific method terms, two specialty-category constraints,
task facts, required headings and a prompt-injection attempt. Synthetic
reference answers are always labeled as fixture output.

```powershell
node catalog/run-role-provider-evaluation.mjs --fixtures
node --test tests/role-real-evaluation-twentieth-wave.test.mjs
```

Live mode requires a **user-verified, explicitly selected local model**, an
OpenAI-compatible HTTP endpoint on literal `127.0.0.1` or `[::1]`, a
deliberate `--live --confirm-local-provider` opt-in, and explicit request,
time and spending limits. For a local model with zero reported price:

```powershell
node catalog/run-role-provider-evaluation.mjs --live --confirm-local-provider `
  --endpoint http://127.0.0.1:11434/v1/chat/completions `
  --model YOUR_CONFIRMED_LOCAL_MODEL --roles literature-scout,security-reviewer `
  --kind both --max-requests 4 --rpm 6 --timeout-ms 20000 `
  --max-total-ms 120000 --max-output-tokens 512 `
  --input-usd-per-million 0 --output-usd-per-million 0 `
  --max-usd 0 --confirm-zero-price
```

`--roles all` explicitly selects the full 200-role catalog, still subject
to the caller's `--max-requests` (1–400), `--rpm` (1–60), deadline,
output-token and estimated dollar-budget caps. Paid or proxied providers
need accurate price estimates supplied by the user, nonzero prices where
applicable, and an appropriate `--max-usd`. Input reservation uses
UTF-8 bytes plus framing as a conservative **estimate**, not a
provider-enforced billing ceiling. Counts are only per invocation.
Errors, ambiguous deliveries, and cancellation stop all further requests;
there are no automatic retries or implicit tool permissions. An optional
bearer credential comes only from `--api-key-env ENV_NAME` and is never
printed. HTTP redirects and non-loopback destinations are rejected.

The sanitized JSON result records fixture digest, role/case IDs, latency,
reported token counts, estimated reserved/observed costs, output SHA-256,
ordered nonempty Markdown sections, role- and task-specific anchors, failure
codes and dangerous-action signals. **No raw model replies, hostile source
text or credentials are logged.** Rule-based scoring is deliberately a proxy:
an answer could repeat keywords and still be factually or semantically wrong.
Every actual model reply requires independent human review, and this harness
does not prove native permission enforcement or real model identity.
Tests use a real local HTTP fixture plus deterministic fake responses;
these are never counted as live model evaluations.

To change an expanded role, edit its source record and run
`node catalog/build-roles.mjs`. Tests verify the original 42
are structurally preserved and all 200 are unique and substantive.

Before enabling a role for autonomous work, its tool requirements, evidence
rules, instruction hierarchy, resource limits, output evaluation and negative
tests must be reviewed against the user-controlled execution runtime.
