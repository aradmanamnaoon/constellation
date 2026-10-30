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

To change an expanded role, edit its source record and run
`node catalog/build-roles.mjs`. Tests verify the original 42
are structurally preserved and all 200 are unique and substantive.

Before enabling a role for autonomous work, its tool requirements, evidence
rules, instruction hierarchy, resource limits, output evaluation and negative
tests must be reviewed against the user-controlled execution runtime.
