/**
 * Enter is a send shortcut only when it is not finalizing an input-method
 * composition. Some WebViews dispatch a second Enter immediately after
 * compositionend with isComposing=false and keyCode !== 229.
 */
export type ComposerEnterAction = 'keep-editing' | 'suppress-ime-commit' | 'send';

export type ComposerKey = Pick<KeyboardEvent,
  'key' | 'shiftKey' | 'isComposing' | 'keyCode' | 'repeat' | 'timeStamp'
>;

export const IME_ENTER_GRACE_MS = 150;

export function composerEnterAction(
  event: ComposerKey,
  compositionActive: boolean,
  lastCompositionEnd: number | null,
): ComposerEnterAction {
  if (event.key !== 'Enter' || event.shiftKey || event.repeat) return 'keep-editing';
  if (compositionActive || event.isComposing || event.keyCode === 229) return 'keep-editing';
  // Event timestamps must share an epoch. A reset/negative delta must not
  // suppress a legitimate later Enter (e.g. after WebView navigation).
  const sinceCompositionEnd = lastCompositionEnd === null
    ? Number.POSITIVE_INFINITY
    : event.timeStamp - lastCompositionEnd;
  if (sinceCompositionEnd >= 0 && sinceCompositionEnd <= IME_ENTER_GRACE_MS) {
    return 'suppress-ime-commit';
  }
  return 'send';
}
