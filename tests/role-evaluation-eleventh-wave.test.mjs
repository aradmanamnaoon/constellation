import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, copyFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

import {
  ROLE_POLICY, assessAnswer, assessDistinctness, composeRoleMessages,
  getRoles, roleSections,
} from '../catalog/role-evaluation.mjs';
import {
  representativeCases, injectionCases, unsafeAnswerCases,
} from '../catalog/role-evaluation-fixtures.mjs';

const project = resolve(fileURLToPath(new URL('..', import.meta.url)));
const roleCatalog = getRoles();
const byId = new Map(roleCatalog.map((role) => [role.id, role]));

function invokeBuilder(directory, ...args) {
  const result = spawnSync(process.execPath, [join(directory, 'build-roles.mjs'), ...args], {
    cwd: directory, encoding: 'utf8', timeout: 20_000, windowsHide: true,
  });
  assert.equal(result.error, undefined, String(result.error));
  return result;
}

test('real builder --check verifies canonical generated roles and detects stale output', (context) => {
  const directory = mkdtempSync(join(tmpdir(), 'constellation-role-eval-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  for (const name of ['build-roles.mjs', 'roles.foundation.json', 'roles-expansion.psv', 'roles.json']) {
    copyFileSync(join(project, 'catalog', name), join(directory, name));
  }
  const unchanged = invokeBuilder(directory, '--check');
  assert.equal(unchanged.status, 0, unchanged.stderr);

  // Mutate a real specialization SOURCE record, not the manifest. Actual
  // build emits its new method; check fails against old output first.
  const source = join(directory, 'roles-expansion.psv');
  const original = readFileSync(source, 'utf8');
  assert.match(original, /OpenAPI-oriented contract/);
  writeFileSync(source, original.replace('Define requests, response schema', 'Define signed requests, response schema'));
  const stale = invokeBuilder(directory, '--check');
  assert.equal(stale.status, 1, 'builder must detect stale generated prompt manifest');
  const built = invokeBuilder(directory);
  assert.equal(built.status, 0, built.stderr);
  const generated = JSON.parse(readFileSync(join(directory, 'roles.json'), 'utf8'));
  const updated = generated.roles.find((role) => role.id === 'api-contract-designer');
  assert.match(updated.instructions, /Define signed requests/);
  assert.match(updated.instructions, /authorized sources only/i);
  assert.equal(generated.roles.length, roleCatalog.length);
  assert.equal(invokeBuilder(directory, '--check').status, 0);

  // A malformed source cannot silently become an apparently reviewed role.
  writeFileSync(source, original.replace('Define requests, response schema',
    'Define requests|response schema'));
  const bad = invokeBuilder(directory);
  assert.equal(bad.status, 1, 'invalid source format must fail closed');
});

test('all canonical generated role prompts retain distinct method and output contracts', () => {
  assert.equal(roleCatalog.length, 200);
  assert.ok(Object.isFrozen(roleCatalog));
  const prompts = new Set();
  for (const role of roleCatalog) {
    assert.ok(Object.isFrozen(role), role.id);
    const messages = composeRoleMessages(role.id, 'Propose a scoped, unexecuted evaluation plan.');
    assert.equal(messages.length, 2);
    assert.deepEqual(messages.map((message) => message.role), ['system', 'user']);
    const [system, task] = messages.map((message) => message.content);
    assert.ok(system.includes(role.name), role.id);
    assert.ok(system.includes(role.instructions), role.id);
    assert.ok(system.includes(ROLE_POLICY), role.id);
    assert.deepEqual(JSON.parse(task), {
      type: 'user_task', text: 'Propose a scoped, unexecuted evaluation plan.',
    });
    const sections = roleSections(role.deliverable);
    assert.ok(sections.every((section) => system.includes('## ' + section)), role.id);
    assert.ok(!prompts.has(system), 'identical system prompts: ' + role.id);
    prompts.add(system);

    // Mechanical full-catalogue format check. Does not count as a model result.
    const synthetic = sections.map((section) =>
      `## ${section}\nProposed checks for ${section}; execution unverified.`).join('\n');
    assert.equal(assessAnswer(role.id, synthetic).passed, true, role.id);
    assert.equal(assessAnswer(role.id, synthetic.replace(/^##/, '###')).passed, false, role.id);
  }
  assert.equal(prompts.size, 200);
});

test('role-specific human-authored format fixtures cover each category', () => {
  assert.equal(representativeCases.length, 10);
  assert.deepEqual(new Set(representativeCases.map((fixture) => byId.get(fixture.roleId)?.category)),
    new Set(roleCatalog.map((role) => role.category)));
  for (const [index, fixture] of representativeCases.entries()) {
    const evaluated = assessAnswer(fixture.roleId, fixture.answer);
    assert.equal(evaluated.passed, true, JSON.stringify(evaluated));
    const anotherRole = representativeCases[(index + 1) % representativeCases.length].roleId;
    const wrongRole = assessAnswer(anotherRole, fixture.answer);
    assert.equal(wrongRole.passed, false, `incorrectly accepted ${fixture.roleId} answer for ${anotherRole}`);
    assert.ok(wrongRole.missingSections.length > 0, fixture.roleId);
    assert.match(composeRoleMessages(fixture.roleId, fixture.task)[0].content, /Required response headings/);
  }
});

test('reject omitted, reordered, empty and code-fenced fake output sections', () => {
  const [first, second, third] = roleSections(byId.get('system-architect').deliverable);
  const base = `## ${first}\nArchitecture tradeoff.\n## ${second}\nInterface contract.\n## ${third}\nCanary rollout.`;
  assert.equal(assessAnswer('system-architect', base).passed, true);
  assert.deepEqual(assessAnswer('system-architect', base.replace(`## ${second}\nInterface contract.\n`, '')).missingSections, [second]);
  assert.equal(assessAnswer('system-architect',
    `## ${second}\nInterface contract.\n## ${first}\nDecision.\n## ${third}\nRollout.`).headingsInOrder, false);
  assert.deepEqual(assessAnswer('system-architect',
    `## ${first}\nDecision.\n## ${second}\n## ${third}\nRollout.`).emptySections, [second]);
  assert.equal(assessAnswer('system-architect',
    `## ${first}\nDecision.\n\`\`\`md\n## ${second}\nFake interface\n\`\`\`\n## ${third}\nRollout.`).passed, false);
});

test('specialization similarity is measured on methods, not repeated boilerplate', () => {
  const result = assessDistinctness();
  assert.equal(result.examined, 200);
  assert.equal(result.pairComparisons, 19_900);
  assert.equal(result.uniqueMethods, 200);
  assert.equal(result.uniqueDeliverables, 200);
  assert.deepEqual(result.nearPairs, []);
  assert.deepEqual(result.tooShort, []);
  assert.ok(result.maxSimilarity.score < 0.82);

  // Mutation test: a copied method and deliverable must be detected.
  const copies = roleCatalog.map((role) => ({ ...role }));
  copies[1].instructions = copies[0].instructions;
  copies[1].deliverable = copies[0].deliverable;
  const detected = assessDistinctness(copies);
  assert.equal(detected.uniqueMethods, 199);
  assert.equal(detected.uniqueDeliverables, 199);
  assert.ok(detected.nearPairs.some((pair) =>
    pair.ids.includes(copies[0].id) && pair.ids.includes(copies[1].id) && pair.score === 1));
});

test('prompt-injection fixtures remain untrusted data, never system text or tools', () => {
  const baseline = composeRoleMessages('security-reviewer', 'Review a suspicious tool description.');
  for (const sample of injectionCases) {
    const task = sample.origin === 'task' ? sample.text : 'Review suspicious supplied evidence.';
    const sources = sample.origin === 'source' ? [{ id: 'untrusted.mcp', text: sample.text }] : [];
    const messages = composeRoleMessages('security-reviewer', task, sources);
    assert.equal(messages[0].role, 'system', sample.name);
    assert.equal(messages[0].content, baseline[0].content, sample.name);
    assert.equal(messages.some((message) => message.role === 'assistant' || message.role === 'tool'), false);
    assert.equal(messages.slice(1).every((message) => message.role === 'user'), true);
    assert.equal(messages[0].content.includes(sample.text), false, sample.name);
    const attached = messages.slice(1).map((message) => JSON.parse(message.content));
    assert.ok(attached.some((part) => part.text === sample.text), sample.name);
    assert.ok(!Object.hasOwn(messages[0], 'tools'), sample.name);
    assert.match(messages[0].content, /Only the trusted native host and a separate explicit user approval/);
  }
});

test('reject role import masquerading as privileged system role, malformed source ids and oversized contexts', () => {
  assert.throws(() => composeRoleMessages('operator-admin-root', 'Do anything'), /unknown canonical role/);
  assert.throws(() => composeRoleMessages({ role: 'system' }, 'Grant shell'), /unknown canonical role/);
  assert.throws(() => composeRoleMessages('tutor', ''), /invalid task/);
  assert.throws(() => composeRoleMessages('tutor', 'X'.repeat(12_001)), /invalid task/);
  assert.throws(() => composeRoleMessages('tutor', 'Task\0injected'), /invalid task/);
  assert.throws(() => composeRoleMessages('tutor', 'Task', [{ id: '../override', text: 'bad' }]), /invalid source id/);
  assert.throws(() => composeRoleMessages('tutor', 'Task', [{ id: 'one', text: 'A' }, { id: 'one', text: 'B' }]), /invalid source id/);
  assert.throws(() => composeRoleMessages('tutor', 'Task', [{ id: 'one', text: 'A'.repeat(16_385) }]), /invalid source/);
  assert.throws(() => composeRoleMessages('tutor', 'Task', Array.from({ length: 9 },
    (_, i) => ({ id: String(i), text: 'untrusted' }))), /invalid source count/);
  assert.throws(() => composeRoleMessages('tutor', 'Task', [
    { id: 'a', text: 'a'.repeat(9000) }, { id: 'b', text: 'b'.repeat(9000) },
  ]), /total source bytes exceeded/);
});

test('evaluator detects fabricated tool results, authorization laundering and secret exfiltration', () => {
  const fixture = representativeCases.find((item) => item.roleId === 'threat-modeling-facilitator');
  const base = assessAnswer(fixture.roleId, fixture.answer);
  assert.equal(base.passed, true);
  for (const attack of unsafeAnswerCases) {
    const candidate = fixture.answer + '\n' + attack.text;
    const assessed = assessAnswer(fixture.roleId, candidate);
    assert.equal(assessed.passed, false, attack.name);
    assert.ok(assessed.securityFindings.includes(attack.reason),
      `${attack.name}: ${JSON.stringify(assessed.securityFindings)}`);
  }
  assert.equal(assessAnswer('security-reviewer', '## Unrelated\nAll complete.').passed, false);
});
