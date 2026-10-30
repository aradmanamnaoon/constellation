import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

const document = JSON.parse(readFileSync(new URL('./roles.json', import.meta.url), 'utf8'));

test('catalogue has unprivileged role status and no implicit tool grants', () => {
  assert.equal(document.schema_version, 1);
  assert.equal(document.status, 'prompt_only_no_tool_runner');
  assert.match(document.notice, /do not grant tools/i);
});

test('each role is distinct, substantive, and has an output contract', () => {
  assert.equal(document.roles.length, 200);
  const ids = new Set();
  const names = new Set();
  for (const role of document.roles) {
    assert.match(role.id, /^[a-z][a-z0-9-]+$/);
    assert.ok(!ids.has(role.id), `duplicate id: ${role.id}`);
    ids.add(role.id);
    assert.ok(!names.has(role.name), `duplicate name: ${role.name}`);
    names.add(role.name);
    for (const property of ['name', 'category', 'summary', 'instructions', 'deliverable']) {
      assert.equal(typeof role[property], 'string');
      const minLength = property === 'category' || property === 'name' ? 3 : 10;
      assert.ok(role[property].trim().length >= minLength, `${role.id}.${property} empty/shallow`);
    }
    assert.ok(role.instructions.length >= 170, `${role.id}: instructions are too shallow`);
    assert.deepEqual(Object.keys(role).sort(), ['category', 'deliverable', 'id', 'instructions', 'name', 'summary'].sort());
  }
  assert.ok(new Set(document.roles.map(role => role.category)).size >= 8);
});

test('original 42 curated roles remain unchanged', () => {
  const foundation = JSON.parse(readFileSync(new URL('./roles.foundation.json', import.meta.url), 'utf8'));
  assert.deepEqual(document.roles.slice(0, 42), foundation.roles);
  assert.equal(foundation.roles.length, 42);
});

test('expanded roles have different outputs, clear methods and no implied execution', () => {
  const expanded = document.roles.slice(42);
  assert.equal(expanded.length, 158);
  assert.equal(new Set(expanded.map((role) => role.deliverable.toLowerCase())).size, expanded.length);
  assert.equal(new Set(expanded.map((role) => role.summary.toLowerCase())).size, expanded.length);
  for (const role of expanded) {
    assert.match(role.instructions, /verify factual claims using authorized sources only/i);
    assert.match(role.instructions, /label outcomes untested/i);
    assert.ok(role.instructions.length > 300, role.id + ' has no substantive procedure');
  }
});
