import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const source = readFileSync(
  resolve(import.meta.dirname, '../apps/desktop/src/routes/+page.svelte'),
  'utf8',
);

function quarantineStatusIsSafe(markup) {
  const branch = markup.lastIndexOf(
    "{#if multiStepStates[task.id].status === 'quarantined'}",
    markup.indexOf('Authenticated step: <strong>quarantined</strong>'),
  );
  const description = markup.indexOf(
    'Budget counters and previous tool outcomes are not independently recoverable here.',
    branch,
  );
  const prohibition = markup.indexOf(
    'No model request or native action may continue from this checkpoint.',
    description,
  );
  const otherBranch = markup.indexOf('{:else}', prohibition);
  const regularCounters = markup.indexOf(
    "multiStepStates[task.id].model_reservations ?? 'Unknown'",
    otherBranch,
  );
  return branch >= 0
    && description > branch
    && prohibition > description
    && otherBranch > prohibition
    && regularCounters > otherBranch
    && !markup.slice(branch, otherBranch).includes('model_limit ??');
}

test('quarantined task distinguishes unrecoverable counters from authenticated activity', () => {
  assert.equal(quarantineStatusIsSafe(source), true);
  const statusArea = source.slice(
    source.indexOf("{#if multiStepStates[task.id] && multiStepStates[task.id].status !== 'not_started'}"),
    source.indexOf('{#if multiStepNotices[task.id]}'),
  );
  assert.match(statusArea, /An interrupted, stale or mismatched checkpoint cannot be resumed/);
});

test('negative mutations cannot turn a quarantined step into an ordinary budget/progress report', () => {
  assert.equal(
    quarantineStatusIsSafe(source.replace(
      'No model request or native action may continue from this checkpoint.',
      'Request another model response now.',
    )),
    false,
  );
  assert.equal(
    quarantineStatusIsSafe(source.replace(
      "{#if multiStepStates[task.id].status === 'quarantined'}\n                          <p role=\"status\">Authenticated step:",
      "{#if multiStepStates[task.id].status === 'awaiting_model'}\n                          <p role=\"status\">Authenticated step:",
    )),
    false,
  );
});
