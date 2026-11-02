# Provider credential retirement: UX and security acceptance

**Branch:** `ux/competitor-rebuild-20261101`

## Implemented

The Settings provider row now exposes **Forget saved key** only for a profile backed by the OS credential store. The user must open a separate confirmation and type the exact displayed provider name. Clicking **Delete saved API key** invokes a distinct native Tauri command; no model request, automatic retry or file operation occurs.

The native command checks the full source provider profile, known credential service/account, task-admission lock, and active-room response state before requesting deletion from the operating system. It does not enumerate other applications' keyring entries. An unavailable vault fails closed. An already absent entry is treated as a retry of an interrupted deletion, not a successful remote authentication.

After OS deletion, an optimistic SQLite transaction removes the saved credential reference while retaining the original provider ID, endpoint/model, assigned agents, tasks, messages, workflow history, and append-only provider update audit event. The native session-key cache is cleared. Users may re-enter a replacement key later. Changing/removing the key never restores prior file consent, native tool permissions or spending authority.

## Important limits

- **No OS vault integration mutation was performed during automated tests.** Rust unit tests validate Core CAS/history with synthetic references. JavaScript source tests validate explicit UI action and native boundaries. The real vault deletion path requires a disposable separate OS user/account, a synthetic secret and observation of the platform credential store before/after.
- OS-vault deletion and SQLite commit are not one distributed transaction. If the app crashes between them, a historical profile may still reference a missing key; requests then fail closed. Retrying after reopening Settings handles `NotFound` and updates the provider record. This does not prove cross-process atomicity.
- An already-dispatched model request may continue using credentials captured before a user revokes the stored key. The UI explicitly warns of this and the native handler denies changes during known active room replies. The revocation is **local device key removal**, not provider-side token invalidation. To prevent the provider from accepting a previously issued token elsewhere, users must revoke the token with the provider itself.
- This change does not delete provider profiles with history. The pre-existing **Remove unused** action remains deliberately narrow and rejects saved credentials and referenced/historical profiles.

## Verification

- `cargo test --locked --lib` and strict `cargo clippy --all-targets --locked -- -D warnings` in `crates/constellation-core` and `apps/desktop/src-tauri`: pass.
- `node --test tests/provider-key-revocation-ux.test.mjs`: 3/3 pass.
- Full frontend regression: **156/156 PASS**; Svelte check: **0 errors / 0 warnings**; source inventory and documentation links PASS.
- Latest Windows release-mode executable SHA-256: `C67E5C688D257DE4F63E7CA1EAEB574948BA7A2CBF775F1444039DEC76831E55`. NSIS bundle compiled, **not installed**. Its read-only generated-NSI lifecycle audit PASS and independent guest-guard self-tests PASS; the actual upgrade/uninstall remain **not run** because this host's HKCU is shared with an existing installation.
- Hidden WebView2/UIA acceptance, **same exact Windows binary**, two separate disposable profiles: isolated provider journey `baed69b19f73483da5b8af20b00676d4` **PASS** (search positive/negative, revision, confirmed credential-free removal, exactly one preserved profile and one removal audit event, no provider calls); isolated palette/task/team journey `853ebeb2e4014a0681022cfafd50e51c` **PASS** (palette search, task planning/filtering, saved team and audit). Both exited 0, used separate hidden Win32 desktops and synthetic data, returned `process_closed=true` and no errors. Evidence screenshots are stored under ignored `apps/desktop/src-tauri/target/ux-provider-isolated-20261102/` and `target/ux-task-team-isolated-20261102/`.
- A combined native fixture (task history plus unused-provider deletion in one workspace) **correctly failed** to delete the provider: current Core deliberately retains all provider identities whenever historical task/message events exist. SQLite inspection showed two provider records and zero `provider_removed` events. This was a **test-precondition conflict**, not a reason to weaken provenance protection; the two independent fixture journeys both passed afterward.
- Real OS key deletion is **not tested**. Do not confuse native UIA evidence for credential-free provider removal with verification of Windows Credential Manager secret deletion. That boundary requires a separate disposable OS user and a test key with known pre/post state.

## Remaining UX and release gates

Credentialed provider lifecycle testing in a disposable account, keyboard/IME/200% zoom and screen-reader support, real scale performance and all-platform installed UX acceptance remain open. The draft PR must remain unmerged and no final release is authorized by this feature alone.
