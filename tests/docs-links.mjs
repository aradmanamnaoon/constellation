#!/usr/bin/env node
/** Validate local Markdown/image dependencies before distributing documentation. */
import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const DEFAULT_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ROOT_DOCS = ['README.md', 'CHANGELOG.md', 'CONTRIBUTING.md', 'SECURITY.md'];
const DOC_DIRECTORIES = ['docs', 'crates', 'catalog', 'skills-manifest'];

function markdownFiles(directory, results) {
  if (!existsSync(directory)) return;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.isSymbolicLink() || entry.name === 'target' || entry.name === 'node_modules') continue;
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) markdownFiles(entryPath, results);
    else if (entry.isFile() && entry.name.toLowerCase().endsWith('.md')) results.push(entryPath);
  }
}

function references(markdown) {
  // Strip fenced examples: snippets that demonstrate a link are not docs links.
  const withoutFences = markdown.replace(/^\s*(`{3,}|~{3,})[^\r\n]*\r?\n[\s\S]*?^\s*\1[^\r\n]*$/gm, '');
  const links = [];
  const inline = /!?\[[^\]\r\n]*\]\(\s*(<[^>\r\n]+>|[^\s)]+)(?:\s+['"][^'"\r\n]*['"])?\s*\)/g;
  for (const match of withoutFences.matchAll(inline)) links.push(match[1]);
  const definition = /^\s{0,3}\[[^\]\r\n]+\]:\s*(<[^>\r\n]+>|\S+)/gm;
  for (const match of withoutFences.matchAll(definition)) links.push(match[1]);
  const htmlImage = /<(?:img|source)\b[^>]*\bsrc\s*=\s*['"]([^'"]+)['"][^>]*>/gi;
  for (const match of withoutFences.matchAll(htmlImage)) links.push(match[1]);
  return links;
}

function localTarget(raw) {
  const target = raw.startsWith('<') && raw.endsWith('>') ? raw.slice(1, -1) : raw;
  if (!target || target.startsWith('#') || /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(target)) return null;
  const pathname = target.split(/[?#]/, 1)[0];
  if (!pathname) return null;
  try {
    return decodeURIComponent(pathname).replace(/\\/g, '/');
  } catch {
    return { error: 'invalid URL encoding' };
  }
}

function insideRoot(root, target) {
  const relative = path.relative(root, target);
  return relative === '' || (relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative));
}

export function scanDocumentation(root, options = {}) {
  const absoluteRoot = realpathSync(root);
  const files = ROOT_DOCS.map((entry) => path.join(absoluteRoot, entry)).filter(existsSync);
  for (const directory of DOC_DIRECTORIES) markdownFiles(path.join(absoluteRoot, directory), files);
  const tracked = options.trackedPaths ?? null;
  const errors = [];
  let checked = 0;
  for (const filename of files) {
    const relativeSource = path.relative(absoluteRoot, filename).split(path.sep).join('/');
    if (tracked && !tracked.has(relativeSource)) {
      errors.push(`${relativeSource}: Markdown source is not tracked in Git`);
    }
    for (const raw of references(readFileSync(filename, 'utf8'))) {
      const target = localTarget(raw);
      if (target === null) continue;
      ++checked;
      if (typeof target === 'object') {
        errors.push(`${relativeSource}: ${raw}: ${target.error}`);
        continue;
      }
      const candidate = path.resolve(target.startsWith('/') ? absoluteRoot : path.dirname(filename), target.replace(/^\//, ''));
      if (!insideRoot(absoluteRoot, candidate)) {
        errors.push(`${relativeSource}: ${raw}: link escapes repository root`);
      } else if (!existsSync(candidate)) {
        errors.push(`${relativeSource}: ${raw}: file does not exist`);
      } else if (!insideRoot(absoluteRoot, realpathSync(candidate))) {
        errors.push(`${relativeSource}: ${raw}: symlink escapes repository root`);
      } else if (lstatSync(candidate).isDirectory()) {
        // Markdown commonly links to a directory index (for example ./).
        if (!target.endsWith('/')) errors.push(`${relativeSource}: ${raw}: expected a file, found a directory`);
      } else if (tracked && !tracked.has(path.relative(absoluteRoot, candidate).split(path.sep).join('/'))) {
        errors.push(`${relativeSource}: ${raw}: target is not tracked in Git`);
      }
    }
  }
  return { documents: files.length, localLinks: checked, errors };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const requireTracked = process.argv.includes('--require-tracked');
  const trackedPaths = requireTracked
    ? new Set(execFileSync('git', ['-C', DEFAULT_ROOT, 'ls-files', '--cached', '-z'], { encoding: 'utf8' }).split('\0').filter(Boolean))
    : null;
  const result = scanDocumentation(DEFAULT_ROOT, { trackedPaths });
  for (const error of result.errors) console.error(`ERROR ${error}`);
  console.log(`Documentation links: ${result.documents} Markdown files, ${result.localLinks} local references, ${result.errors.length} errors`);
  if (result.errors.length) process.exitCode = 1;
}
