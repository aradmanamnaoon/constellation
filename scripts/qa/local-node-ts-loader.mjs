// Local Ubuntu Node 22 fallback. The system Node may expose
// --experimental-strip-types but be built *without* the TS parser. We load
// only local source .ts files for offline Node tests via the repository's
// pinned TypeScript dependency. This never changes production bundles.
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';

const ts = createRequire(new URL('../../apps/desktop/package.json', import.meta.url))('typescript');

export async function load(url, context, nextLoad) {
  if (!url.startsWith('file:') || !url.split('?', 1)[0].endsWith('.ts')) {
    return nextLoad(url, context);
  }
  const source = await readFile(new URL(url), 'utf8');
  const { outputText, diagnostics = [] } = ts.transpileModule(source, {
    fileName: new URL(url).pathname,
    reportDiagnostics: true,
    compilerOptions: {
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ES2022,
      verbatimModuleSyntax: true,
    },
  });
  const errors = diagnostics.filter((item) => item.category === ts.DiagnosticCategory.Error);
  if (errors.length) {
    throw new Error('Local TS test loader failed with ' + errors.length + ' compile diagnostics');
  }
  return { format: 'module', shortCircuit: true, source: outputText };
}
