# Removing an unused provider — narrow, safety-first implementation

The Settings provider directory now offers a separate **Remove unused** action for a locally saved profile that the UI does not identify as having a persistent OS key reference. This is intentionally **not** general provider retirement, credential revocation or historical-route rewriting. No provider endpoint is contacted when the user clicks Remove, Keep or Confirm.

### Enforced scope

After a second explicit confirmation, native `studio_remove_unused_provider` validates the caller's provider ID and the exact expected public name, endpoint, model and protocol. It obtains the existing local task authorization gate, refuses if there are saved workflows or currently generating replies, and serializes against session-key changes. Core `StudioStore::remove_unused_provider` atomically checks the full saved `ProviderProfile` equality, rejects any OS keyring or environment credential source, any agent with that provider assigned, and **any** workspace task/message history. Only then does it delete the row and record `ProviderRemoved` in the event log in the same SQLite transaction. It removes the session key only after the transaction commits. The event retains the exact public identity for local provenance and repeat operations with the same ID are idempotent.

**Known limits:** A remembered-credential connection cannot be removed through this action, because deleting its OS vault entry requires a separately audited, failure-recoverable credential lifecycle. Workspaces with any saved task/message or workflow intentionally refuse removal even if the provider was never used. This conservative policy avoids breaking historical authenticated model-route identities. It does not make an unsafe in-place endpoint mutation or silently migrate existing agent/task/workflow references. Independent separate app processes and external project archives are not proved mutually exclusive by this UI-level workflow; exhaustive multi-process admission and upgrade/restore acceptance remain open release gates.

### Verification

Core contract tests cover persistence/reopen, exactly-once event, stale public fields, credentials, active agent assignment and conversation history refusal. `tests/provider-unused-removal-ux.test.mjs` tests source-level human confirmation and native/Core denial contracts. Windows hidden-desktop GUI acceptance in `tests/windows-installed-uia-smoke.ps1 -TestProviderDirectory` creates two synthetic localhost profiles, confirms removal of only the revised, credential-free unused profile, verifies one remaining original provider and exactly one removal audit event directly in the disposable SQLite database; it sends **zero** network/model requests.

Actual build/QA commit and native execution output are recorded separately under `docs/ux/qa-results-2026-11-02.md`. The tests do not assert OS-vault cleanup, production credential deletion or macOS/Android native support.
