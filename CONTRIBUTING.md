# Contributing to Constellation

Constellation is under active development and not yet a validated public
release. Before changes, read [`AGENTS.md`](AGENTS.md), the
[`23-workstream plan`](docs/mission/COMPLETE_EXECUTION_PLAN.md), and the
[`188-requirement acceptance inventory`](docs/quality/acceptance-ledger.md).

## Working on code

Preserve unrelated working-tree changes and source-pinned QA snapshots.
Never run destructive Git cleanup or reset operations against a collaborator's
work. Keep Rust ownership of tasks, permission evaluation, file effects,
SQLite state and provider dispatch; Svelte renders state without making an
authority decision. Third-party content, skill definitions and model outputs
never grant their own tools, network access or permissions.

Use the project's existing standalone crate manifests rather than assuming a
single root Cargo workspace. On Windows, direct Cargo build output to a
dedicated temporary C: target; development builds on D: can exhaust space.
For example:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp\constellation-dev-target'
cargo test --manifest-path crates/constellation-tools/Cargo.toml --locked --offline --all-targets
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --offline --lib
node catalog/build-roles.mjs --check
node --test catalog/roles.test.mjs
node skills-manifest/validate.mjs --verify-local
cd apps/desktop
pnpm run check
pnpm run build
```

Run appropriate negative tests for permissions, cancellation, path changes,
network policy and restart when changing security-sensitive code. Record exact
binary SHA-256, source fingerprint, platform, tested behavior and result in
`docs/qa/`; don't infer installed OS behavior from a library test. For
unavailable hardware/services, document `BLOCKED` or `UNVERIFIED` and keep the
acceptance gate open.

## Dependencies and developer integrations

Review licenses, source pins and lockfiles when adding crates/npm dependencies.
Each Rust crate has its own `Cargo.lock`; all 13 should be committed with the
source before hosted CI. Development roles and 200 vetted skills/MCPs are
different objects: never claim an unexercised prompt or a copied skill file
as a verified executable integration. Use real per-integration receipts and
run the strict verifier.

Do not commit secrets, private profile paths, local databases, model API keys,
large generated binaries, working caches, or screenshots that expose user
information. Prepare source-controlled assets and reproducible release checks
before requesting a merge, public release, or broad tool permission.
