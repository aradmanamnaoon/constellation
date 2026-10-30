# ADR-0001 — Presentation framework for Project Constellation

**Date researched:** 2026-10-30
**Decision status:** **PROVISIONAL — Tauri 2 + Svelte/TypeScript favored; implementation gate UNVERIFIED**
**Source of requirements:** 1,237-line *Codex Master Prompt — Local-First Multi-Agent Studio*, §§1, 3, 6–7, 18, 21–25, 30, 32.
**Evidence categories:** `DOC` = official documentation verified; `HYPOTHESIS` = engineering judgment; `OBSERVED` = prototype/test result accompanied by artifact. There is **no OBSERVED evidence for either framework as of this ADR**.

## Decision and scope

Start with **Tauri 2 as the native desktop/Android host, a custom Svelte + TypeScript view, and an independent Rust application core and headless runner**. TypeScript may render chat, previews, editor/diff components and local UI state; it must not decide tool authorization, credentials, budgets, subprocess policy, remote execution or durable scheduling. Tauri commands are typed, narrow transports into Rust application services. The authorization decision happens again in the shared Rust policy engine immediately before each side effect.

The user demands a rich room/chat interface, long streaming transcripts, code diffs, large role catalogue, mobile approval interface and keyboard/screen-reader quality. Browser text rendering, selection, native text input, CSS layout and established editor/virtualization tooling make a WebView presentation layer the stronger **starting hypothesis**. Slint remains a real alternative; its Android backend, accessibility annotations and efficient `ListView` are documented. This is a conditional choice until representative installed GUI prototypes pass the gates below.

## Decision matrix

Scores are **relative pre-prototype hypotheses** (1 = serious concern; 3 = viable with material engineering; 5 = strong fit). They do **not** certify performance, accessibility, feature completeness, release signing, or cross-platform behavior. Re-score with measurements and links to test evidence; no aggregate constitutes a test result.

| Criterion | Tauri 2 + Svelte | Slint + Rust UI | Reason / decisive prototype question |
|---|---:|---:|---|
| Polished desktop composition | 4 | 4 | Both can implement original, adaptive layouts; judge actual UI and native input. |
| Android packaging path | 4 | 3 | Tauri documents `android build --apk`; Slint documents Android Rust and `xbuild` APK, both must be installed and tested. |
| Screen readers / accessibility | 4 | 3 | Semantic HTML + ARIA and compiler warnings versus Slint `accessible-*` and platform backends; neither grants WCAG compliance automatically. |
| 500-message virtualization | 4 | 5 | TanStack's Svelte virtualizer versus Slint `ListView` which instantiates only visible items; variable-height stream anchoring unproven in both. |
| Interactive code diff/editor | 5 | 2 | Web ecosystem offers text editors, syntax selection and diff widgets; Slint likely requires more custom interaction work. |
| Rich markdown, links and text | 5 | 3 | DOM rendering provides mature semantics; sanitize untrusted HTML/markdown in either implementation. |
| IME, international text, composing | 4 | 3 | Host WebView editing versus native Slint `TextInput`; must test Persian/Arabic, CJK, emoji, RTL and clipboard on each OS. |
| Transcript text selection/copy | 5 | 3 | DOM selectable text is convenient; Slint needs explicit design for rich text and selection. |
| Animation and microinteractions | 5 | 4 | CSS motion and Slint transitions both possible; honor reduced-motion setting. |
| Package size / idle overhead | 4 | 5 | Tauri uses installed OS WebView; Slint aims for small native runtime; actual bundled binaries, RAM and CPU unknown. |
| Fully custom design/tokens | 5 | 5 | Both support original design systems; prototype high contrast and dense states. |
| Toolchain/build complexity | 3 | 3 | Tauri needs Rust + JS/Android SDK and host WebViews; Slint needs Rust UI + Android packaging/renderer setup. |
| Maintenance and team fit | 4 | 4 | Rust backend in either; web editor dependencies and Slint native component upkeep carry different costs. |
| Licensing flexibility | 5 | 3 | Tauri MIT/Apache-2.0; Slint GPLv3, royalty-free with conditions, or commercial; review complete tree. |
| Cross-platform installed GUI automation | 4 | 2 | Tauri has WebDriver guidance plus OS-native tests; Slint likely requires accessibility/native automation harness. |
| Mobile safe platform adaptation | 4 | 4 | Both require Android platform adapters, storage access framework, lifecycle/background and scoped permissions. |

**Decision rationale:** Prefer Tauri for a content-heavy interface with advanced text editing, accessible semantics and rapid high-fidelity UI iteration; use Slint if the tested Slint prototype measurably beats Tauri for accessibility, performance and interaction fidelity **and** its license is acceptable. Retain a UI-independent Rust core so a later switch is bounded. Electron is excluded by the user's specification.

## What the documentation does and does not establish

1. **Desktop engines:** Tauri's WRY chooses system WebViews (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux). Platform engine/version differences mean visual behavior and selection/IME/accessibility need testing per OS. Tauri requires system dependencies, notably WebView2 and Windows C++ tools for development, and WebKitGTK development dependencies on Linux. [Tauri architecture](https://v2.tauri.app/concept/architecture/), [prerequisites](https://v2.tauri.app/start/prerequisites/), [WRY platform documentation](https://docs.rs/crate/wry/latest).
2. **Android:** Tauri documents Android packaging and APK output, with minimum Android **7 / API 24**, and requires signing for distribution. Slint's current Android guide describes **API 26** minimum for its Android backend and Rust-based packaging. Neither document proves Constellation's Android UX or runner behavior. [Tauri APK](https://v2.tauri.app/distribute/google-play/), [Tauri signing](https://v2.tauri.app/distribute/sign/android/), [Slint Android](https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/android/).
3. **Mobile tool limits:** Tauri's shell plugin on Android permits URL opening only; it is **not** an arbitrary subprocess layer. Android task/background restrictions also rule out guarantees of an unattended, all-purpose local phone runner. Allow Android-safe app-private/doc-picker/network tasks and monitoring of an explicitly paired user-owned desktop/VM. [Tauri shell platforms](https://v2.tauri.app/plugin/shell/), [Android background restrictions](https://developer.android.com/develop/background-work/background-tasks/bg-work-restrictions), [foreground-service changes](https://developer.android.com/develop/background-work/services/fgs/changes).
4. **500+ messages:** TanStack Virtual explicitly supports Svelte and long feeds; Slint's `ListView` creates visible items only. Neither official page proves stable anchored scroll for *streaming variable-height* Markdown with search/keyboard selection. [TanStack Virtual](https://tanstack.com/virtual/latest/docs/introduction), [Svelte virtualization adapter](https://tanstack.com/table/latest/docs/framework/svelte/guide/virtualization), [Slint ListView](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/listview/).
5. **Accessibility:** Svelte's build-time warnings cover some markup mistakes but cannot replace screen-reader/runtime tests. Slint exposes accessible roles, names and live regions; this is an interface to assistive technology, not proof of TalkBack, VoiceOver, NVDA or AT-SPI quality. Follow WCAG 2.2 AA where applicable. [Svelte accessibility](https://svelte.dev/docs/svelte/compiler-warnings), [Slint accessible properties](https://docs.slint.dev/latest/docs/slint/reference/common/), [WCAG 2.2](https://www.w3.org/TR/WCAG22/).
6. **Licensing:** Tauri is MIT OR Apache-2.0. Slint has GPLv3, a royalty-free desktop/mobile/web license requiring the specific attribution route in its current terms, and a commercial option. Its royalty-free terms differ from unconstrained permissive open source; review the actual applicable version and transitive license reports before distribution. Svelte and chosen JS editor/virtualizer dependencies need their own lockfile audit. [Tauri license](https://v2.tauri.app/concept/architecture/), [Slint licensing](https://slint.dev/faqs), [Slint royalty-free license v2.0](https://slint.dev/agreements/slint-royalty-free-license.pdf).
7. **GUI automation:** Tauri documentation describes WebDriver-oriented testing, but coverage varies by driver and platform. Under Tauri 2's documented `tauri-driver` path, Windows/Linux are supported; macOS requires another native/embedded approach, and Android requires mobile-specific tooling. Confirm tool versions rather than assuming a cross-platform test runner works. Use native UIA, macOS accessibility, AT-SPI and Android UI Automator/ADB as needed. [Tauri WebDriver guide](https://v2.tauri.app/develop/tests/webdriver/), [direct driver setup](https://v2.tauri.app/develop/tests/webdriver/manual-setup/).
8. **Security boundary:** Tauri provides webview capabilities and CSP configuration, but these are defense in depth. The Rust policy engine must authorize every operation, including tools invoked via MCP, workflows, remote runners and Rust commands. Never expose generic `shell.execute` to ordinary frontend code. [Tauri capabilities](https://v2.tauri.app/security/capabilities/), [CSP](https://v2.tauri.app/security/csp/), [IPC isolation](https://v2.tauri.app/concept/inter-process-communication/isolation/).

## Required A/B engineering spikes — all UNVERIFIED

Create **two actual one- or two-screen applications** (chat room with task drawer) using a shared deterministic Rust event fixture; implement **one Android installable candidate**. For each, preserve build SHA, exact dependency versions, OS/VM/emulator, driver/device, screenshot/video and the raw test log. Require:

- `SPIKE-01`: Generate **500 mixed-height messages**, including Markdown, code blocks, inline mentions, file previews and concurrent streaming; scroll newest→oldest→newest and assert correct anchoring, no jumps, text selection/copy and input focus. Also profile 10,000-message navigation separately, without loading 10,000 DOM nodes.
- `SPIKE-02`: Use a real edit/diff card with 100+ changed lines; select/copy syntax; approve a specific immutable diff and invalidate approval if path/content/target changes; keyboard-only navigation and an accessible rejection state.
- `SPIKE-03`: Exercise screen reader and keyboard with rooms, message list, task drawer, modal, progress announcements, search and provider setup. Try NVDA/Windows, VoiceOver/macOS, AT-SPI/Linux and TalkBack/Android; measure real issues and contrast rather than relying only on axe/lint.
- `SPIKE-04`: Test IME composition with Persian/RTL, Japanese or Chinese composition, combining characters, emoji, copy/paste, input preserving drafts, zoom/text-scale 200%, a 900×600 desktop window, Android portrait/landscape and hardware/software keyboards. No destructive command should trigger from an unfinished composition.
- `SPIKE-05`: Build/install/launch desktop bundles and release-like APK on available hosts; record binary sizes, cold launch, idle RAM/CPU, time to interactive, UI thread latency and battery impact; include offline startup and invalid-provider message.
- `SPIKE-06`: Verify Svelte/Slint selected semantics under a hostile rendered Markdown/HTML fixture; ensure no HTML/JS can invoke privileged commands or bypass Rust authorization.

**Go/no-go:** No final framework verification, WCAG claim, Android parity claim, launch budget claim, package-size comparison, or measured score until these test artifacts exist. If a spike exposes a blocking limitation, fix and retest or reconsider Slint; record the changed ADR with real evidence.

## Implementation boundaries once gate passes

- `ui/`: custom Svelte view, accessible semantic components, CSS tokens, markdown sanitizer, chat virtualizer and code-diff/editor presentation. No secrets in JS state, untrusted HTML, unaudited remote scripts or arbitrary Rust-command names.
- `apps/desktop` and Android app: lifecycle, native file picker/URI grants, secure credential/key APIs, notifications, platform capability reporting and Tauri IPC adapters.
- `crates/`: typed domain, contracts, permissions, execution, agents, workflows, providers, storage, sync and remote transport. Keep policy, budgets and durable transitions inside Rust.
- `apps/runner-cli`: separately installed, authenticated, opt-in user-owned host runner with its own central policy checks and revocation.
- Model **provider endpoint** and **execution target** as different explicit selectors. A remote model can suggest actions executed locally; connecting a local model cannot confer filesystem privileges.

**Follow-up:** put observed spike outcomes in `docs/qa/evidence/`, link each row in `docs/quality/acceptance-ledger.md`, record dependency license/SBOM output, and supersede `PROVISIONAL` only after real native/Android interaction evidence.
