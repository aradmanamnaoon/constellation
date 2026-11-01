# Windows NSIS verification candidate — 2026-11-01

## Source and package identity

- Source revision **`8206f4dd504fb207754536994d67e959f6a001e3`**; only the Windows UIA test itself was adjusted *after* this production binary was built. The production sources and lockfiles were unchanged during the build.
- Used Windows Rust 1.99.1, frozen Cargo lockfile and Tauri NSIS build in a dedicated D: target (`apps/desktop/src-tauri/target/candidate-8206f4d-win`) after D: had 50.6 GiB free. Tauri release build and frontend build both finished successfully.
- Unsigned installer `Constellation_0.1.0_x64-setup.exe`: 3,800,047 bytes; SHA-256 **`5041eee12a16250404f9f21a6b967cd3a8133872987a4836fd5575d5b7561fd6`**; Windows Authenticode `NotSigned`.
- Direct executable `constellation-desktop.exe`: 10,306,560 bytes; SHA-256 **`52671041520944b55d2b58a1e7158ba5f1802b7e86a399c6a0c195e7133796ec`**.
- Generated `installer.nsi`: SHA-256 **`5666d2229ff247c258676e3cbb970355da9538cfbd822bf9202c0124760cfe7b`**; `currentUser` install mode and WebView2 `downloadBootstrapper` mode.
- Independently source-hash-pinned 7-Zip at SHA-256 `cef3594ebe6568e66e1e6ea1a5f782d0f109a5e11764b5a02511a308d2a0cec7` inspected the actual NSIS payload. **`OFFLINE_PAYLOAD_MATCH_ONLY`**: exact executable bytes apart from Tauri's three-byte bundle marker `UNK` → `NSS`. Seven adversarial Node NSIS tests passed. This does NOT certify installation or signing.

## Private non-input Windows desktop results

All runs used the project-provided hidden Win32 desktop helper, unique `--isolated-data-dir`, private WebView2 profile and ephemeral synthetic fixtures. No application UI was opened on the user's active desktop.

The first broad test showed outdated UIA selectors; the current UI moved Evidence search labels, source hash into a collapsed disclosure, and per-step workflow action buttons into `<details>`. The QA script was adjusted to operate only through the actually visible, unique, actionable native UIA controls. A second run proved Evidence's source quote, UTF-8 byte range, full hash, encrypted backup/restore, encrypted two-database project archive/restore, and exactly one localhost-only workflow step. The third isolated run reproduced those passes but **failed closed** on workflow cancellation confirmation: Chromium exposes the mutated `aria-expanded` confirmation as `ExpandCollapsePattern` but its `Expand()` call fails. A legacy accessibility type was unavailable in this PowerShell 5.1 runtime and was removed rather than treating a partial step as success. The manual file-action disclosure was also not uniquely available in the prior full test. No native file operation was certified.

Evidence files reside under the local ignored `dist/verification-only/8206f4d-windows` folder. Do not upload private raw QA logs, screenshots or isolated project data as release assets.

## Intentionally unpassed release gates

- An **existing Constellation installation** is registered in the user's Windows current-user registry; the safe native installer checker deliberately refuses to run NSIS `Install-Isolated` where a previous app's product identity could be upgraded or uninstalled. The generated NSIS may download WebView2 prerequisites, independently disqualifying its side-effect-free isolated-install path. `windows-bundle-identity.ps1` returned **`IDENTITY_NOT_PROVEN`** as required.
- No Authenticode signature or independent clean-machine installed, upgrade or uninstall lifecycle acceptance. No claim of production Windows trust.
- Interactive native OS permission decision and cancellation acceptance remains pending. No tests were weakened or forced to pass.
- macOS ARM64/Intel signed and notarized installed packages, and signed Android ARM64 on-device native security/device acceptance, remain unverified.

**Publication classification:** `UNSIGNED EXPERIMENTAL WINDOWS CANDIDATE — release_ready=false`. It may be attached only to a clearly labeled experimental/prerelease, alongside a manifest and explicit warnings. It is not a complete production release.
