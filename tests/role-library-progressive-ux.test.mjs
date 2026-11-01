import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { roleCategories, filterRoleLibrary } from '../apps/desktop/src/lib/role-library.ts';

const roles = JSON.parse(readFileSync(new URL('../catalog/roles.json', import.meta.url), 'utf8')).roles
  .map(({ name, category, summary }) => ({ name, group: category, blurb: summary }));
const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');

test('all 200 real roles remain reachable through categories and search', () => {
  assert.equal(roles.length, 200);
  assert.deepEqual(filterRoleLibrary(roles, '', 'All'), roles);
  const categories = roleCategories(roles);
  assert.ok(categories.length >= 5);
  assert.equal(categories.reduce((sum, category) => sum + filterRoleLibrary(roles, '', category).length, 0), roles.length);
  assert.deepEqual(filterRoleLibrary(roles, 'not-a-real-specialty', 'All'), []);
});

test('role search matches case-insensitively, honors category and does not alter data', () => {
  const original = structuredClone(roles);
  assert.ok(filterRoleLibrary(roles, 'literature', 'All').length > 0);
  const first = roles[0];
  assert.ok(filterRoleLibrary(roles, first.name.toUpperCase(), first.group).some((role) => role.name === first.name));
  assert.equal(filterRoleLibrary(roles, first.name, 'not-a-category').length, 0);
  assert.deepEqual(roles, original);
});

test('role UI starts with a bounded page and permits exploring every result', () => {
  assert.match(page, /const visibleRoles = \$derived\(filteredRoles\.slice\(0, rolePageSize\)\)/);
  assert.match(page, /let rolePageSize = \$state\(24\)/);
  assert.match(page, /rolePageSize \+= 24/);
  assert.match(page, /roleCategory = group; rolePageSize = 24/);
  assert.match(page, /\{#each visibleRoles as role \(role\.name\)\}/);
});
