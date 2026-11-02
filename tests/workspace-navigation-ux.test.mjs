import assert from 'node:assert/strict';
import { test } from 'node:test';
import { filterWorkspaceRooms, searchWorkspaceCommands } from '../apps/desktop/src/lib/workspace-navigation.ts';
import { readFileSync } from 'node:fs';

const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
const palette = readFileSync(new URL('../apps/desktop/src/lib/WorkspaceCommandPalette.svelte', import.meta.url), 'utf8');
const styles = readFileSync(new URL('../apps/desktop/src/routes/studio.css', import.meta.url), 'utf8');

const rooms = [
  { id: 'room-1', name: 'Medical Research' },
  { id: 'room-2', name: 'Long-term Product Design' },
  { id: 'room-3', name: 'Research methods' },
];

test('room search matches all tokens and preserves stable IDs and list order', () => {
  assert.deepEqual(filterWorkspaceRooms(rooms, '  RESEARCH  '), [rooms[0], rooms[2]]);
  assert.deepEqual(filterWorkspaceRooms(rooms, 'product long'), [rooms[1]]);
  assert.deepEqual(filterWorkspaceRooms(rooms, 'missing'), []);
  assert.deepEqual(filterWorkspaceRooms(rooms, ''), rooms);
});

test('palette returns real room targets and relevant actions without executing anything', () => {
  const result = searchWorkspaceCommands(rooms, 'research');
  assert.deepEqual(result.filter((command) => command.type === 'room').map((command) => command.roomId), ['room-3', 'room-1']);
  assert.deepEqual(searchWorkspaceCommands(rooms, 'connect').map((command) => command.id), ['action:provider']);
  assert.equal(searchWorkspaceCommands(rooms, 'evidence')[0].id, 'section:evidence');
  assert.equal(searchWorkspaceCommands(rooms, 'notfound').length, 0);
  assert.ok(searchWorkspaceCommands(rooms, '').length >= 9);
  assert.equal(searchWorkspaceCommands(rooms, 'room', 2).length, 2);
});

test('keyboard search, focus and navigation semantics are wired into the real view', () => {
  assert.match(page, /aria-label="Filter rooms"/);
  assert.match(page, /<WorkspaceCommandPalette rooms=\{snapshot\.rooms\}/);
  assert.match(palette, /aria-activedescendant=\{commands\.length/);
  assert.match(palette, /event\.key === 'ArrowDown'/);
  assert.match(palette, /event\.key === 'Enter'/);
  assert.match(page, /const editing = target instanceof HTMLElement/);
  assert.match(page, /if \(dialog\) closeDialog\(\)/);
  assert.match(palette, /scrollIntoView\(\{ block: 'nearest' \}\)/);
  assert.match(page, /button:not\(\[disabled\]\):not\(\[tabindex="-1"\]\)/);
});

test('conversation message search preserves local-only filtering and bounded pagination', () => {
  assert.match(page, /let messageQuery = \$state\(''\)/);
  assert.match(page, /const matchingMessages = \$derived\(messages\.filter/);
  assert.match(page, /message\.sender_name\} \$\{message\.body\}/);
  assert.match(page, /matchingMessages\.slice\(/);
  assert.match(page, /id="conversation-message-search"/);
  assert.match(page, /No messages match this search/);
  assert.match(page, /messageQuery = '';\s*\/\/ Explicitly reconfirm/);
  assert.doesNotMatch(palette, /invoke\(|fetch\(/);
});

test('small-height palette prioritizes visible results but keeps accessible keyboard help', () => {
  assert.match(styles, /\.studio \.modal\.command-dialog\s*\{[^}]*height:min\(90dvh,710px\)/);
  assert.match(styles, /\.workspace-command-search input\s*\{[^}]*height:39px/);
  assert.match(styles, /\.workspace-command-results button\s*\{[^}]*min-height:45px/);
  assert.match(styles, /@media\(max-height:640px\)/);
  assert.match(styles, /\.studio \.command-dialog #workspace-command-hint\s*\{[^}]*clip-path:inset\(50%\)/);
  assert.match(styles, /\.studio \.command-dialog \.workspace-command-results button\s*\{[^}]*min-height:41px/);
  assert.match(palette, /aria-describedby="workspace-command-hint"/);
  assert.match(palette, /role="listbox"/);
});

test('task provider choices are independent of chat and from other task cards', () => {
  assert.match(page, /let taskProviders = \$state<Record<string, string>>\(\{\}\)/);
  assert.match(page, /const choice = taskProviders\[task\.id\] \?\? ''/);
  assert.match(page, /value=\{taskProviders\[task\.id\] \?\? ''\}/);
  assert.match(page, /\[task\.id\]: event\.currentTarget\.value/);
  assert.match(page, /taskProviderId\(task\)/);
  assert.match(page, /taskSearch\.trim\(\)\.toLocaleLowerCase\(\)/);
  assert.match(page, /paginateLocal\(visibleRoomTasks, taskPage, taskPageSize\)/);
  assert.match(page, /\{#each pagedRoomTasks as task \(task\.id\)\}/);
});

test('first-task setup offers working prerequisite actions and resumes the draft', () => {
  assert.match(page, /onclick=\{\(\) => continueTaskSetup\('new-room'\)\}/);
  assert.match(page, /onclick=\{\(\) => continueTaskSetup\('new-agent'\)\}/);
  assert.match(page, /taskSetupReturn === 'new-room'/);
  assert.match(page, /taskSetupReturn === 'new-agent'/);
  assert.match(page, /taskRoomId = created\.id/);
  assert.match(page, /taskAgentId = snapshot\.agents\.find/);
  assert.match(page, /dialog = 'new-task';\s*await focusCurrentDialogField\(\)/);
});

test('home dashboard links to real rooms and task filters without executing models', () => {
  assert.match(page, /const homeRooms = \$derived\(snapshot\.rooms\.slice\(-5\)\.reverse\(\)\)/);
  assert.match(page, /const homeOpenTasks = \$derived\(snapshot\.tasks\.filter/);
  assert.match(page, /onclick=\{\(\) => openCommandRoom\(recentRoom\.id\)\}/);
  assert.match(page, /taskFilterRoomId = activeTask\.room_id/);
  assert.match(page, /taskSearch = activeTask\.instructions\.slice\(0, 40\)/);
  assert.match(page, /No tasks currently awaiting action/);
});
