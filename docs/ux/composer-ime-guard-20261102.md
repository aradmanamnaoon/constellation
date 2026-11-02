# Composer input-method Enter guard (2026-11-02)

**Scope:** Local message composer only. A previously handled `isComposing`/keyCode 229 check cannot catch every WebView that issues a non-composing Enter immediately after `compositionend`. This can accidentally submit a just-confirmed IME candidate instead of letting the user continue editing.

Implementation: `apps/desktop/src/lib/composer-keyboard.ts` derives a testable three-way Enter decision; the Svelte textarea tracks composition events. Enter while composing is left to the input method, immediate post-composition Enter (up to 150 ms on the same timestamp scale) is suppressed, Shift+Enter remains a newline, and held-key repeat cannot send several messages. An ordinary subsequent Enter retains the explicit local-save shortcut. The submitted message remains an atomic local SQLite operation, with no implied model/provider call.

**Validation:** `node --test tests/composer-keyboard-ime.test.mjs` tests normal, multiline, repeat, active composition, legacy keyCode 229, immediate post-composition and timestamp-reset cases. Svelte static check confirms the event wiring compiles. These deterministic tests are not a substitute for real Windows IME input, Japanese/Korean/Chinese keyboard scenarios or an Android software keyboard. Real input-method acceptance remains required prior to final UX release.
