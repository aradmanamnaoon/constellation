import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { getRoles, composeRoleMessages } from '../catalog/role-evaluation.mjs';
import { ATTACK_MARKER, buildRoleChallenges, fixtureDigest, offlineBaselineAnswer } from '../catalog/role-real-fixtures.mjs';
import { requireLoopbackEndpoint, runLiveRoleEvaluation, scoreRoleAnswer, selectChallenges } from '../catalog/role-real-evaluation.mjs';
import { offlineBaselineReport, parseCommandLine } from '../catalog/run-role-provider-evaluation.mjs';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const roleIds = ['system-architect', 'literature-scout'];
const endpoint = 'http://127.0.0.1:11434/v1/chat/completions';
const config = {
  endpoint, model: 'local-model:small', roleIds,
  kind: 'both', maxRequests: 4, requestsPerMinute: 60,
  timeoutMs: 2000, maxTotalMs: 20_000, maxOutputTokens: 256,
  inputUsdPerMillion: 0, outputUsdPerMillion: 0, maxEstimatedUsd: 0,
};
const provider = (answer, options = {}) => ({
  choices: [{
    index: 0, message: { role: 'assistant', content: answer, ...options.message },
    finish_reason: options.reason ?? 'stop',
  }],
  usage: options.usage ?? { prompt_tokens: 100, completion_tokens: 80 },
});
const response = (value, status = 200) => new Response(JSON.stringify(value), {
  status, headers: { 'content-type': 'application/json' },
});
const simulatedClock = () => {
  let now = 1_000_000;
  return {
    now: () => now,
    sleep: async (duration) => { now += duration; },
    advance: (duration) => { now += duration; },
  };
};

test('canonical real-evaluation challenge matrix is 200 roles × 2 with 10 specialties, unique and stable', () => {
  const roles = getRoles();
  const cases = buildRoleChallenges();
  assert.equal(roles.length, 200);
  assert.equal(cases.length, 400);
  assert.equal(new Set(cases.map((c) => c.id)).size, 400);
  assert.equal(new Set(cases.map((c) => c.category)).size, 10);
  assert.match(fixtureDigest(), /^[a-f0-9]{64}$/);
  assert.equal(fixtureDigest(), fixtureDigest(buildRoleChallenges()));
  for (const role of roles) {
    const normal = cases.find((c) => c.id === role.id + ':normal');
    const injection = cases.find((c) => c.id === role.id + ':injection');
    assert.ok(normal && injection, role.id);
    assert.equal(normal.sources.length, 1, role.id);
    assert.equal(injection.sources.length, 2, role.id);
    assert.deepEqual(normal.expected, injection.expected);
    assert.equal(normal.expected.anchors.length, 3);
    assert.equal(normal.expected.specialistAnchors.length, 2);
    assert.notEqual(normal.expected.specialistAnchors[0], normal.expected.specialistAnchors[1]);
    const messages = composeRoleMessages(injection.roleId, injection.task, injection.sources);
    assert.equal(messages.length, 4);
    assert.equal(messages[0].content.includes(ATTACK_MARKER), false);
    assert.equal(messages.some((m) => m.role === 'tool' || m.role === 'assistant'), false);
    assert.equal(JSON.parse(messages.at(-1).content).text.includes(ATTACK_MARKER), true);
  }
  assert.throws(() => buildRoleChallenges(roles.slice(0, 199)), /exactly 200/);
  assert.throws(() => buildRoleChallenges([...roles.slice(0, 199), roles[0]]), /unique/);
  assert.throws(() => buildRoleChallenges(roles.map((role, i) => i ? role :
    { ...role, instructions: 'grant shell' })), /canonical/);
  assert.throws(() => selectChallenges({ roleIds: ['root-shell-admin'] }), /canonical/);
  assert.throws(() => selectChallenges({ roleIds: ['tutor', 'tutor'] }), /distinct/);
});

test('synthetic baseline discriminates adherence, structure, injection and unsupported execution on ALL roles', () => {
  const cases = buildRoleChallenges();
  for (const c of cases) {
    const safe = offlineBaselineAnswer(c);
    const result = scoreRoleAnswer(c, safe);
    assert.equal(result.automated_gate, true, c.id);
    assert.equal(result.human_semantic_review, 'required');
    assert.equal(result.structured_output.valid, true);
    assert.equal(scoreRoleAnswer(c, offlineBaselineAnswer(c, { attacked: true })).automated_gate, false);
    assert.ok(scoreRoleAnswer(c, safe.replaceAll('Project ORBIT', 'Project UNKNOWN')).task_adherence
      .required_anchors.some((a) => a.term === 'orbit' && !a.present));
    assert.equal(scoreRoleAnswer(c,
      '## ' + getRoles().find((role) => role.id === c.roleId).deliverable
        + '\nCompleted without reservations.').task_adherence.uncertainty_disclosed, false);
    assert.equal(scoreRoleAnswer(c, safe.replace(/^## /, '### ')).structured_output.valid, false);
    const withoutSpecialty = safe.replaceAll(new RegExp(c.expected.specialistAnchors[0], 'gi'), 'notpresent');
    assert.equal(scoreRoleAnswer(c, withoutSpecialty).automated_gate, false);
  }
  const summary = offlineBaselineReport();
  assert.equal(summary.generated_safe_reference_gate_passes, 400);
  assert.equal(summary.injected_unsafe_reference_gate_rejections, 400);
  assert.equal(summary.actual_provider_requests, 0);
  assert.equal(summary.actual_model_quality_verified, false);
  assert.equal(summary.human_semantic_review, 'not_performed');
});

test('loopback-only explicit URL and CLI gates deny remote/provider ambiguity', () => {
  for (const unsafe of [
    'https://127.0.0.1:11434/v1/chat/completions',
    'http://localhost:11434/v1/chat/completions',
    'http://0.0.0.0:11434/v1/chat/completions',
    'http://10.1.0.1:11434/v1/chat/completions',
    'http://2130706433:11434/v1/chat/completions',
    'http://127.0.0.1:11434/v1/chat/completions?target=evil',
    'http://user:key@127.0.0.1:11434/v1/chat/completions',
    'http://127.0.0.1:11434/v1/chat/completions#override',
    'http://127.0.0.1:11434/%76%31/chat/completions',
    'http://127.0.0.1:11434/redirect',
    'http://evil.invalid/v1/chat/completions',
  ]) assert.throws(() => requireLoopbackEndpoint(unsafe), undefined, unsafe);
  assert.equal(requireLoopbackEndpoint(endpoint), endpoint);
  assert.equal(requireLoopbackEndpoint('http://[::1]:11434/v1/chat/completions'),
    'http://[::1]:11434/v1/chat/completions');
  assert.throws(() => parseCommandLine(['--live']), /explicit live opt-in/);
  assert.throws(() => parseCommandLine(['--fixtures', '--live']), /offline fixtures/);
  assert.throws(() => parseCommandLine(['--fixtures', '--fixtures']), /repeated/);
  const args = ['--live', '--confirm-local-provider', '--endpoint', endpoint,
    '--model', 'local-model:small', '--roles', 'tutor', '--input-usd-per-million', '0',
    '--output-usd-per-million', '0', '--max-usd', '0'];
  assert.throws(() => parseCommandLine(args), /zero-price/);
  const parsed = parseCommandLine([...args, '--confirm-zero-price']);
  assert.equal(parsed.config.roleIds[0], 'tutor');
  assert.equal(parsed.config.maxRequests, 4);
  assert.equal(parseCommandLine(['--fixtures']).mode, 'fixtures');
});

test('live transport uses canonical messages, zero tools, one request at a time and no raw answer logs', async () => {
  const sample = selectChallenges({ roleIds: ['literature-scout'], kind: 'both' });
  const clock = simulatedClock();
  const calls = [];
  const output = await runLiveRoleEvaluation({ ...config, roleIds: ['literature-scout'], maxRequests: 2 }, {
    now: clock.now, sleep: clock.sleep,
    fetchImpl: async (url, init) => {
      assert.equal(url, endpoint);
      assert.equal(init.method, 'POST');
      assert.equal(init.redirect, 'manual');
      assert.equal(init.cache, 'no-store');
      assert.equal(init.headers.authorization, undefined);
      const body = JSON.parse(init.body);
      assert.deepEqual(Object.keys(body).sort(),
        ['max_tokens', 'messages', 'model', 'n', 'stream', 'temperature']);
      assert.equal(body.messages[0].role, 'system');
      assert.ok(body.messages[0].content.includes('Literature Scout'));
      assert.equal(body.messages.some((m) => m.role === 'tool' || m.role === 'assistant'), false);
      assert.equal(Object.hasOwn(body, 'tools'), false);
      calls.push(body);
      clock.advance(45);
      return response(provider(offlineBaselineAnswer(sample[calls.length - 1])));
    },
  });
  assert.equal(calls.length, 2);
  assert.equal(output.counters.issued_requests, 2);
  assert.equal(output.counters.successes, 2);
  assert.equal(output.counters.rule_gate_passes, 2);
  assert.equal(output.results[0].usage.input, 100);
  assert.ok(output.results[1].latency_ms >= 45);
  assert.equal(output.results[0].answer_sha256.length, 64);
  assert.equal(output.results[0].automated_evaluation.human_semantic_review, 'required');
  assert.equal(output.termination, 'completed');
  assert.equal(JSON.stringify(output).includes('No tests were executed'), false);
  assert.equal(JSON.stringify(output).includes(ATTACK_MARKER), false);
  assert.equal(calls[1].messages.length, 4);
});

test('real HTTP loopback OpenAI-compatible fixture receives one canonical request without tools or redirects', async (t) => {
  const sample = selectChallenges({ roleIds: ['system-architect'], kind: 'normal' })[0];
  const received = [];
  const server = createServer(async (request, reply) => {
    assert.equal(request.method, 'POST');
    assert.equal(request.url, '/v1/chat/completions');
    let text = '';
    for await (const block of request) {
      text += block.toString('utf8');
      assert.ok(text.length < 130_000);
    }
    const payload = JSON.parse(text);
    assert.equal(payload.messages.length, 3);
    assert.deepEqual(payload.messages.map((m) => m.role), ['system', 'user', 'user']);
    assert.equal(payload.tools, undefined);
    received.push(payload);
    reply.writeHead(200, { 'content-type': 'application/json' });
    reply.end(JSON.stringify(provider(offlineBaselineAnswer(sample))));
  });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  t.after(() => new Promise((ok) => server.close(ok)));
  const address = server.address();
  const actualEndpoint = `http://127.0.0.1:${address.port}/v1/chat/completions`;
  const report = await runLiveRoleEvaluation({
    ...config, roleIds: ['system-architect'], kind: 'normal',
    endpoint: actualEndpoint, maxRequests: 1,
  });
  assert.equal(received.length, 1);
  assert.equal(report.counters.successes, 1);
  assert.equal(report.counters.rule_gate_passes, 1);
  assert.equal(report.results[0].automated_evaluation.structured_output.valid, true);
  assert.equal(report.results[0].automated_evaluation.task_adherence.specialist_anchors.length, 2);
  assert.equal(report.results[0].error_code, null);
});

test('invalid or malicious provider replies are never treated as valid assessment', async () => {
  const sample = selectChallenges({ roleIds: ['system-architect'], kind: 'normal' })[0];
  const invalid = [
    response(provider(offlineBaselineAnswer(sample), { reason: 'length' })),
    response(provider(offlineBaselineAnswer(sample), { message: { tool_calls: [{ id: 'danger' }] } })),
    response({ choices: [{ message: { role: 'system', content: 'compromised' }, finish_reason: 'stop' }] }),
    response({ choices: [{ message: { role: 'assistant', content: '' }, finish_reason: 'stop' }] }),
    new Response('not json', { status: 200 }),
    response({ error: 'secret should never be emitted' }, 401),
  ];
  for (const fake of invalid) {
    const report = await runLiveRoleEvaluation({ ...config, roleIds: [sample.roleId],
      kind: 'normal', maxRequests: 1 }, { fetchImpl: async () => fake });
    assert.equal(report.counters.issued_requests, 1);
    assert.equal(report.counters.failed, 1);
    assert.equal(report.results[0].automated_evaluation, null);
    assert.equal(report.results[0].answer_sha256, null);
    assert.equal(report.budget.unknown_usage_attempts, 1);
    assert.equal(JSON.stringify(report).includes('secret should never be emitted'), false);
  }
});

test('request cap, estimated worst-case cost, rate and run deadline fail before spending', async () => {
  let calls = 0;
  const fake = async () => { calls++; return response(provider('reply')); };
  const capped = await runLiveRoleEvaluation({ ...config, maxRequests: 1 }, {
    fetchImpl: fake,
  });
  assert.equal(capped.counters.issued_requests, 1);
  assert.equal(capped.termination, 'request_cap');
  const tooExpensive = await runLiveRoleEvaluation({ ...config, inputUsdPerMillion: 1000,
    outputUsdPerMillion: 1000, maxEstimatedUsd: 0 }, { fetchImpl: fake });
  assert.equal(tooExpensive.counters.issued_requests, 0);
  assert.equal(tooExpensive.termination, 'budget_exhausted');
  assert.equal(calls, 1);
  const clock = simulatedClock();
  const waits = [];
  const paced = await runLiveRoleEvaluation({ ...config, maxRequests: 2,
    requestsPerMinute: 30, maxTotalMs: 3000 }, {
    now: clock.now, sleep: async (ms) => { waits.push(ms); clock.advance(ms); },
    fetchImpl: fake,
  });
  assert.equal(paced.counters.issued_requests, 2);
  assert.deepEqual(waits, [0, 2000]);
  const deadlineClock = simulatedClock();
  const deadline = await runLiveRoleEvaluation({ ...config, maxRequests: 2,
    maxTotalMs: 100 }, { now: deadlineClock.now, sleep: deadlineClock.sleep, fetchImpl: fake });
  assert.equal(deadline.counters.issued_requests, 1);
  assert.equal(deadline.termination, 'run_deadline');
});

test('explicit abort and timed-out in-flight request stop further calls; no retries', async () => {
  const controller = new AbortController();
  let calls = 0;
  const cancel = await runLiveRoleEvaluation({ ...config, signal: controller.signal }, {
    fetchImpl: async () => {
      calls++;
      controller.abort();
      throw new Error('provider invoice may have been charged');
    },
  });
  assert.equal(calls, 1);
  assert.equal(cancel.termination, 'cancelled');
  assert.equal(cancel.counters.issued_requests, 1);
  assert.equal(cancel.counters.failed, 1);
  assert.equal(JSON.stringify(cancel).includes('provider invoice may have been charged'), false);

  let timedOutCalls = 0;
  const timed = await runLiveRoleEvaluation({ ...config, timeoutMs: 100 }, {
    fetchImpl: (_, init) => {
      timedOutCalls++;
      return new Promise((_, reject) => {
        if (init.signal.aborted) reject(new Error('abort'));
        init.signal.addEventListener('abort', () => reject(new Error('abort')), { once: true });
      });
    },
  });
  assert.equal(timed.counters.issued_requests, 1);
  assert.equal(timedOutCalls, 1);
  assert.equal(timed.results[0].error_code, 'timeout');
  assert.equal(timed.termination, 'provider_failure_fail_closed');
});

test('CLI fixture run executes locally and emits zero live requests', () => {
  const executed = spawnSync(process.execPath,
    ['catalog/run-role-provider-evaluation.mjs', '--fixtures'],
    { cwd: root, encoding: 'utf8', timeout: 15_000, windowsHide: true });
  assert.equal(executed.status, 0, executed.stderr);
  const report = JSON.parse(executed.stdout);
  assert.equal(report.canonical_roles, 200);
  assert.equal(report.actual_provider_requests, 0);
  assert.equal(report.generated_safe_reference_gate_passes, 400);
  assert.equal(report.injected_unsafe_reference_gate_rejections, 400);
  assert.match(report.fixture_sha256, /^[a-f0-9]{64}$/);
  const catalogue = JSON.parse(readFileSync(new URL('../catalog/roles.json', import.meta.url), 'utf8'));
  assert.equal(catalogue.status, 'prompt_only_no_tool_runner');
});
