#!/usr/bin/env node
/**
 * Read-only discovery followed by an optional local manifest write.
 * This creates an evidence-backed candidate inventory, NOT 200 working MCPs.
 * No SKILL.md instructions are executed and nothing is installed.
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..');
const sourceRoot = join(root, '..', 'vibe-coding-skills', 'skills');
const inventoryPath = join(here, 'inventory.json');
const manifest = JSON.parse(readFileSync(inventoryPath, 'utf8'));
const saved = new Map(manifest.entries.map((entry) => [entry.id, entry]));
const listed = new Set();
const entries = [];
function keep(entry) {
  if (listed.has(entry.id)) throw new Error('Duplicate ID in inventory: ' + entry.id);
  listed.add(entry.id);
  entries.push(entry);
}
const sha = (data) => createHash('sha256').update(data).digest('hex');
const walk = (path) => {
  if (!existsSync(path)) throw new Error('Expected source directory not found: ' + path);
  const result = [];
  for (const item of readdirSync(path, { withFileTypes: true })) {
    const full = join(path, item.name);
    if (item.isDirectory()) result.push(...walk(full));
    else if (item.isFile() && item.name === 'SKILL.md') result.push(full);
  }
  return result.sort();
};
const header = (data) => {
  const text = data.toString('utf8');
  if (!text.startsWith('---\n') && !text.startsWith('---\r\n')) return null;
  const frontmatter = text.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (!frontmatter) return null;
  const name = frontmatter[1].match(/^name:\s*(.+)\s*$/m)?.[1]?.trim();
  const description = frontmatter[1].match(/^description:[ \t]*(.*)$/m)?.[1]?.trim();
  if (!name || !description || !/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(name)) return null;
  // Handle both inline YAML and folded/literal descriptions. Do not parse
  // arbitrary YAML objects or execute any of the untrusted skill contents.
  const folded = /^[>|][-+]?$/.test(description);
  const following = frontmatter[1].split(/\r?\n/);
  const index = following.findIndex((line) => /^description:/.test(line));
  const descriptionLines = [];
  if (folded) {
    for (const line of following.slice(index + 1)) {
      if (!/^\s+/.test(line) && line.trim()) break;
      if (line.trim()) descriptionLines.push(line.trim());
    }
  }
  const summary = (folded ? descriptionLines.join(' ') : description)
    .replace(/^["']|["']$/g, '').trim().slice(0, 500);
  if (summary.length < 18) return null;
  return { name, summary, text };
};
const observedOn = manifest.observed_on;
const sourceFiles = walk(sourceRoot);

for (const entry of manifest.entries.filter((item) => item.kind === 'cli')) keep(entry);
for (const file of sourceFiles) {
  const data = readFileSync(file);
  const front = header(data);
  if (!front || front.name !== basename(dirname(file))
    || !front.text.includes('# Safety restrictions')
    || !front.text.includes('# Completion criteria')) {
    throw new Error('Unexpected sibling skill metadata or safeguards: ' + file);
  }
  const id = 'skill:' + front.name;
  // Keep previously vetted installations and their source/revision/receipt
  // identities. Regeneration must never downgrade a counted audit result or
  // replace its installed source path with the sibling candidate path.
  if (saved.has(id)) {
    keep(saved.get(id));
  } else {
    keep({
      id, kind: 'skill', status: 'source_reviewed',
      purpose: front.summary,
      source: {
        path: relative(root, file).split(sep).join('/'),
        provenance: 'Read-only inspection of the sibling vibe-coding-skills source; author/repo identity not independently verified',
        sha256: sha(data),
      },
      evidence: {
        command: 'Read SKILL.md, inspect name/description/safety/completion headings and calculate SHA-256 with Node crypto',
        output: 'Local file observed with matching frontmatter, safety/completion sections and ' + data.byteLength + ' bytes on ' + observedOn,
      },
      license: {
        status: 'verified', spdx: 'MIT', evidence: '../vibe-coding-skills/LICENSE',
      },
      limitations: [
        'Source and sibling repository license reviewed; canonical upstream publisher not independently authenticated',
        'Not installed in Constellation or exercised in the product; tool and dependency access not vetted',
      ],
    });
  }
}
if (sourceFiles.length !== 42) throw new Error('Sibling skill source changed; inspect manually');

const relevant = /(agent|ai|api|app|architecture|backend|benchmark|bioinformatic|build|cloud|code|coding|container|database|data|debug|design|dev|docker|experiment|github|git|infra|integration|java|kubernetes|langgraph|linux|llm|mcp|model|monitor|notion|openai|ops|performance|python|quality|react|research|rust|security|software|sql|test|typescript|ui|ux|web|workflow|machine|statistics|accessibility|frontend|deployment|production|privacy)/i;
const priority = /(agent|api|architecture|backend|build|cloud|code|database|data|debug|dev|docker|flutter|github|git|infra|integration|llm|mcp|model|monitor|performance|python|quality|react|research|rust|security|software|sql|test|typescript|web)/i;
const home = process.env.USERPROFILE;
if (!home) throw new Error('USERPROFILE unavailable; source paths cannot be verified');
const userRoots = ['.agents/skills', '.claude/skills'];
const userFiles = userRoots.flatMap((name) => walk(join(home, name)));
const candidates = [];
for (const path of userFiles) {
  const data = readFileSync(path);
  const meta = header(data);
  if (!meta || meta.name !== basename(dirname(path))) continue;
  const id = 'skill:' + meta.name;
  if (listed.has(id) || !relevant.test(meta.name)) continue;
  const localPath = '%USERPROFILE%/' + relative(home, path).split(sep).join('/');
  candidates.push({
    id, kind: 'skill', status: 'installed_unvetted', purpose: meta.summary,
    source: {
      path: localPath,
      provenance: 'Observed user-level skill file; publisher, dependency chain, scope and execution unverified',
      sha256: sha(data),
    },
    evidence: {
      command: 'Read user-level SKILL.md name/description frontmatter and calculate SHA-256 with Node crypto',
      output: 'Observed matching frontmatter and ' + data.byteLength + ' bytes on ' + observedOn + '; never executed',
    },
    license: { status: 'unverified', spdx: null, evidence: null },
    limitations: [
      'Exists in user-level skill directories, not in Constellation runtime',
      'Upstream origin, license, dependency safety, permissions and live tool behavior unverified',
    ],
  });
}
const candidateOrder = (entry) => [
  priority.test(entry.id) ? 0 : 1,
  entry.source.path.includes('/.claude/') ? 0 : 1,
  entry.id,
];
candidates.sort((a, b) => {
  const aa = candidateOrder(a), bb = candidateOrder(b);
  return aa[0] - bb[0] || aa[1] - bb[1] || aa[2].localeCompare(bb[2]);
});
// Preserve two previously observed files regardless of lexical selection.
for (const old of manifest.entries.filter((entry) => entry.status === 'installed_unvetted')) {
  if (!listed.has(old.id)) keep(old);
}
for (const candidate of candidates) {
  if (entries.filter((entry) => entry.status === 'installed_unvetted').length >= 158) break;
  if (!listed.has(candidate.id)) keep(candidate);
}
for (const old of manifest.entries) {
  if (!listed.has(old.id)) keep(old); // Never discard pre-existing evidence.
}
if (entries.filter((entry) => entry.kind === 'skill').length < 200) {
  throw new Error('Fewer than 200 unique observed skill files; no count padding: '
    + JSON.stringify({ sourceFiles: sourceFiles.length, userFiles: userFiles.length,
      candidates: candidates.length, observed: entries.length, skills: entries.filter((entry) => entry.kind === 'skill').length }));
}
const encoded = JSON.stringify({ ...manifest, entries }, null, 2) + '\n';
if (process.argv.includes('--check')) {
  if (readFileSync(inventoryPath, 'utf8') !== encoded) {
    process.stderr.write('Local inventory differs from observed files; review sources and run --write\n');
    process.exitCode = 1;
  } else {
    process.stdout.write('Observed inventory matches local files; qualifying vetted integrations are separately evaluated.\n');
  }
} else if (process.argv.includes('--write') && process.argv.length === 3) {
  writeFileSync(inventoryPath, encoded);
  const skills = entries.filter((entry) => entry.kind === 'skill');
  process.stdout.write('Recorded ' + skills.length + ' unique skill files with hashes: '
    + skills.filter((entry) => entry.status === 'source_reviewed').length + ' source reviewed; '
    + skills.filter((entry) => entry.status === 'installed_unvetted').length + ' installed unvetted; '
    + 'existing installed_vetted records are preserved and must still pass --verify-local.\n');
} else {
  process.stderr.write('Usage: node skills-manifest/refresh-observed-skills.mjs --check|--write\n');
  process.exitCode = 2;
}
