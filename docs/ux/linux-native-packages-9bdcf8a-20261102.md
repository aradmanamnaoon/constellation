# Linux native packages — source-pinned local evidence

**Date:** November 2, 2026. **Source:** `9bdcf8a2e2a284c5bc6d33ee25f4086c353ff942`, clean Ubuntu WSL2 checkout at `/home/saba/constellation-ux-qa-9bdcf8a`. This is native Linux userspace, not a macOS, Android or independent Windows test host.

## Source and build checks

- Clean offline pnpm install, source gate PASS, **169/169 JavaScript tests PASS**, Svelte zero errors and warnings.
- `CARGO_BUILD_JOBS=1`, Linux-only cached Cargo target, and offline Cargo dependencies. `pnpm --dir apps/desktop tauri build --bundles deb,appimage` compiled and linked the actual Linux x86-64 release executable, then **successfully generated the native `.deb`**.
- The same Tauri run **failed during `linuxdeploy` AppImage packaging**. No successful end-to-end Tauri AppImage build is claimed. The generated `Constellation.AppDir` (267 MiB) was subsequently processed by the locally cached `appimagetool` into a manual type-2 AppImage. The bundling tool retrieved a type-2 runtime from the AppImage project's GitHub release; the runtime download was not independently signed/attested in this exercise. The retry to run the complete Tauri AppImage command again was terminated because Windows free memory fell below 1 GiB. No source code or permissions were weakened to force success.

## Artifact identity and inspection

| Artifact | SHA-256 | Status |
| --- | --- | --- |
| `Constellation_0.1.2_amd64.deb` | `d40316f97778753993623571ef7d8eb7ac594b8db31b1471640edd2a9a29923b` | Tauri-created, `dpkg-deb` fields/contents inspected; extracted rootlessly |
| Extracted `.deb` executable | `14fc1420f7535ecdfaca3650c0dcedca4f765b84aed5d13144b76b4790fa0ae4` | Native ELF x86-64, dynamic libraries available on test Ubuntu; actual Xvfb launch PASS |
| `Constellation_0.1.2_amd64.AppImage` | `4d6ec24da65fd7a92a3312e99161b8e7dff6b5ba6ca4edf98375c0f588dadebb` | Manually packaged from Tauri's AppDir, not Tauri's successful one-shot output; Xvfb launch PASS in both rootless extraction and FUSE modes |

The post-`.deb` `target/release/constellation-desktop` binary has a **different hash**, because the subsequent Tauri bundler pass patches the application again; that binary must **not** substitute for the exact `.deb` payload. The rootless `.deb` extraction was inspected rather than installing with the host system's dpkg database.

## Real GUI observations

Using the exact clean-checkout `tests/linux-gui-xvfb.sh` with a dedicated synthetic home/data/cache/runtime, Xvfb `1280x800x24`, a private openbox window manager, no physical display/user profile and bounded process cleanup:

- Extracted `.deb`: `GUI_WINDOW_VISIBLE=YES`, `GUI_WORKSPACE_RENDERED=YES`, viewport entropy `0.28195`.
- Manual AppImage with `APPIMAGE_EXTRACT_AND_RUN=1`: same visibility, rendered-content and entropy checks PASS.
- Manual AppImage using the normal FUSE runtime: same checks PASS.
- Both `.deb` and AppImage screenshots were **visually inspected**. They show the Constellation first-run workspace, navigation/sidebar, create-room action, legible typography and no obvious clipping in the tested viewport. Linux screenshot files and process logs are retained only under ignored local QA cache/target paths, not committed as fabricated CI evidence.

**Local evidence directory:** `/home/saba/.cache/constellation-linux-packages/run.rTNnYfOF`. Both packages were exercised as binaries in isolated profiles, **not installed, upgraded, or uninstalled through the actual OS package manager**. No real provider/network operation was authorized during the GUI tests. Real Linux upgrade/uninstall, further GUI flows, native screen readers, other Linux distributions and signing/attestation still require separate acceptance. The existing Windows/macOS/Android and full 188-row acceptance gates are not cleared by this result.
