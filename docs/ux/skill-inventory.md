# Available UX skills and actual-use ledger — 2026-11-01

**Counting method:** 36 readable Constellation-managed skill directories under `/skills/constellation` were observed by an independent read-only worker. The local manifest contains 208 integration entries (8 CLI, 36 declared installed/vetted, 6 source-reviewed, 158 unvetted). Strict `node skills-manifest/validate.mjs --verify-local` failed; only **9 of 200** source-matched exercises currently receive credit and **0 live MCPs** are verified. Another 530 `SKILL.md` file instances across four project-namespaced roots are *not* 530 qualified Constellation skills; some are duplicate or designed for other codebases.

No external skills were newly installed in this slice. Source/license/commit/permissions for cross-project transfer candidates remain unverified. These facts prevent counting 50–80 vetted tools or 20 fully executed skills as already achieved.

**Version/license:** the four directly inspected Constellation SKILL.md files have no version or license value in their YAML headers; they are local paths, with attribution/provenance not established here. Do not infer SPDX license from location. The remaining entries below were reviewed by the read-only worker; their version/license and runtime qualification likewise remain unverified.

| Skill (source: `/skills/constellation/<name>/SKILL.md`) | Installed | Applied to this slice (actual evidence) | Runtime/native evidence |
|---|---|---|---|
| accessibility-auditor | yes | Source: combo/listbox names, keyboard selection, focus, disabled actions; `WorkspaceCommandPalette.svelte` | Native pending |
| responsive-design-auditor | yes | CSS: bounded room/team scroll, narrow palette/task filters in `studio.css` | Viewport matrix pending |
| ux-state-completeness-auditor | yes | Palette/room/task no-match states; first-task missing prerequisites and return flow | Native pending |
| dead-feature-detector | yes | Traced palette callbacks to navigate/selectRoom/dialog, task setup to existing Rust `invoke`; no noop command | Real execution pending |
| requirement-traceability-auditor | yes | Reviewed; mapped issues in `baseline-audit.md` | Not exercised independently |
| regression-detector | yes | Reviewed; scoped static tests and frontend checks | Not fully exercised |
| test-quality-auditor | yes | Reviewed; negative cases for search and per-task selection added | Native pending |
| missing-test-detector | yes | Reviewed; native test gaps logged in `journeys.md` | Not fully exercised |
| reproduction-builder | yes | Reviewed | Not exercised |
| root-cause-debugger | yes | Reviewed; source-level shared provider state root cause noted | Not fully exercised |
| completion-claims-auditor | yes | Reviewed; unverified checks remain labeled | No full mission signoff |
| api-contract-auditor | yes | Reviewed; no Rust command signature changed | Not fully exercised |
| multi-agent-integration-reviewer | yes | Reviewed | Not exercised |
| data-consistency-auditor | yes | Reviewed; task filtering leaves `snapshot.tasks` intact | Not fully exercised |
| environment-parity-auditor | yes | Reviewed | Not exercised |
| cross-platform-readiness-auditor | yes | Reviewed | Not exercised |
| agent-tool-trust-auditor | yes | Reviewed; preserved native model dispatch | Not fully exercised |
| authorization-boundary-auditor | yes | Reviewed; provider choice does not send | Not fully exercised |
| input-validation-auditor | yes | Reviewed; filter whitespace/case tests | Not fully exercised |
| production-readiness-auditor | yes | Reviewed; public release remains blocked by native/cross-platform acceptance | Not fully exercised |

**Transfer candidates:** names such as `command-palette-keystrokes`, `dialog-focus-return`, `search-result-scope`, `responsive-pane-states` and `provider-egress-consent` are readable in `/skills/researchweave`, but target a separate Slint/Rust product and cannot be credited as installed Constellation skills without permission/source/licensing review and real application.

The applied entries are **source-review applications**, not completed skill certification. Use this table as a working evidence ledger; advance only after applicable actual GUI and negative checks are observed.
