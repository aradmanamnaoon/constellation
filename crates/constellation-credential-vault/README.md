# Constellation credential vault

Small opt-in provider API key vault. `Vault` uses the `keyring` crate with
Windows Credential Manager and macOS Keychain backends. Linux and Android
currently return `Unavailable` because their secure-store dependencies are
not available in the offline build; there is no mock or plaintext fallback.
Only the native host should call this crate; the webview must never receive
stored credentials. The namespace is fixed, and `ProviderKey` accepts UUIDs
only. Secrets are zeroized when the returned `Secret` is dropped and omitted
from debug output. No plaintext fallback or network request exists.

This component does **not** automatically migrate legacy session credentials,
persist a provider profile, grant any agent tools or prove secure behavior on
an untested operating system. A desktop integration and an actual OS vault
roundtrip still require verification. The OS account may allow other processes
with equivalent privileges to access stored credentials; this is not a
process isolation boundary.

```powershell
cargo test --manifest-path crates/constellation-credential-vault/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-credential-vault/Cargo.toml --all-targets --offline -- -D warnings
```

For an explicitly authorized, local Windows/macOS credential-manager smoke
test, set `CONSTELLATION_TEST_OS_VAULT=1` and run `cargo test` on the crate.
This creates a random isolated temporary entry, loads it, deletes it and
verifies absence. Its cleanup guard attempts deletion again if a test fails;
the test is inert in ordinary CI. It never uses an actual provider API key.
