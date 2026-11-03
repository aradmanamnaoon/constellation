/** Render untrusted chat text as text and fenced code blocks without HTML injection. */
export type ChatBlock =
  | { kind: 'text'; text: string }
  | { kind: 'code'; text: string; language: string };

const OPEN_FENCE = /^```([\w.+#-]{0,24})[ \t]*$/;
const CLOSE_FENCE = /^```[ \t]*$/;

export function splitChatBlocks(body: string): ChatBlock[] {
  // Large or malformed messages remain accessible as raw text and are never
  // parsed with unbounded, backtracking regular expressions.
  if (body.length > 1_000_000 || !body.includes('```')) return [{ kind: 'text', text: body }];

  const lines = body.replaceAll('\r\n', '\n').split('\n');
  const result: ChatBlock[] = [];
  let plain: string[] = [];
  let code: string[] = [];
  let language = '';
  let inCode = false;

  for (const line of lines) {
    if (!inCode) {
      const opening = OPEN_FENCE.exec(line);
      if (opening) {
        if (plain.length) result.push({ kind: 'text', text: plain.join('\n') });
        plain = [];
        language = opening[1];
        inCode = true;
      } else plain.push(line);
    } else if (CLOSE_FENCE.test(line)) {
      result.push({ kind: 'code', text: code.join('\n'), language });
      code = [];
      language = '';
      inCode = false;
    } else code.push(line);
  }

  if (inCode) {
    // A broken fence is still user content; it must not disappear.
    plain.push(`\`\`\`${language}`, ...code);
  }
  if (plain.length) result.push({ kind: 'text', text: plain.join('\n') });
  return result.length ? result : [{ kind: 'text', text: '' }];
}
