#!/usr/bin/env node
/**
 * Manual, bounded, loopback-only provider evaluator.
 * No live request without BOTH --live and --confirm-local-provider.
 */
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { getRoles } from './role-evaluation.mjs';
import { buildRoleChallenges, fixtureDigest, offlineBaselineAnswer } from './role-real-fixtures.mjs';
import { requireLoopbackEndpoint, runLiveRoleEvaluation, scoreRoleAnswer } from './role-real-evaluation.mjs';

const HELP = `Usage:
  node catalog/run-role-provider-evaluation.mjs --fixtures
  node catalog/run-role-provider-evaluation.mjs --live --confirm-local-provider \\
    --endpoint http://127.0.0.1:11434/v1/chat/completions --model local-model \\
    --roles literature-scout,security-reviewer --kind both --max-requests 4 \\
    --rpm 6 --timeout-ms 20000 --max-total-ms 120000 \\
    --max-output-tokens 512 --input-usd-per-million 0 \\
    --output-usd-per-million 0 --max-usd 0 --confirm-zero-price

--roles all explicitly selects the full 200-role manifest (400 cases with --kind both).
Provider credentials are loaded only from --api-key-env ENV_NAME if supplied.
The endpoint must be a literal loopback HTTP OpenAI-compatible /v1/chat/completions.
No tool definitions, shell requests, external URLs or automatic retries.
Costs are conservative estimates, not provider-enforced billing guarantees.
Output contains scores, hashes, latency and usage only, never private replies.
`;

const switches = new Set(['fixtures', 'live', 'confirm-local-provider', 'confirm-zero-price', 'help']);
const keys = new Set(['endpoint', 'model', 'roles', 'kind', 'max-requests', 'rpm', 'timeout-ms',
  'max-total-ms', 'max-output-tokens', 'input-usd-per-million', 'output-usd-per-million',
  'max-usd', 'api-key-env']);

export function parseCommandLine(argv) {
  const args = Object.create(null);
  for (let i = 0; i < argv.length; i++) {
    const raw = argv[i];
    if (!raw.startsWith('--')) throw new TypeError('invalid argument');
    const key = raw.slice(2);
    if (key in args || (!switches.has(key) && !keys.has(key))) throw new TypeError('invalid or repeated flag');
    if (switches.has(key)) args[key] = true;
    else {
      if (!argv[i + 1] || argv[i + 1].startsWith('--')) throw new TypeError('missing flag value');
      args[key] = argv[++i];
    }
  }
  if (args.help) return { help: true };
  if (args.fixtures) {
    if (Object.keys(args).length !== 1) throw new TypeError('offline fixtures cannot include live flags');
    return { mode: 'fixtures' };
  }
  if (!args.live || !args['confirm-local-provider'] || !args.endpoint || !args.model || !args.roles
      || args['input-usd-per-million'] === undefined
      || args['output-usd-per-million'] === undefined || args['max-usd'] === undefined) {
    throw new TypeError('explicit live opt-in, endpoint, model, roles, price inputs and budget required');
  }
  if (args['api-key-env'] && !/^[A-Za-z_][A-Za-z0-9_]{0,80}$/.test(args['api-key-env'])) {
    throw new TypeError('invalid credential environment variable');
  }
  requireLoopbackEndpoint(args.endpoint);
  const all = getRoles().map((r) => r.id);
  const roleIds = args.roles === 'all' ? all : args.roles.split(',');
  const inputUsdPerMillion = Number(args['input-usd-per-million']);
  const outputUsdPerMillion = Number(args['output-usd-per-million']);
  if (inputUsdPerMillion === 0 && outputUsdPerMillion === 0 && !args['confirm-zero-price']) {
    throw new TypeError('explicit zero-price acknowledgement required');
  }
  return {
    mode: 'live',
    config: {
      endpoint: args.endpoint, model: args.model, roleIds, kind: args.kind ?? 'both',
      maxRequests: Number(args['max-requests'] ?? 4),
      requestsPerMinute: Number(args.rpm ?? 6),
      timeoutMs: Number(args['timeout-ms'] ?? 20_000),
      maxTotalMs: Number(args['max-total-ms'] ?? 120_000),
      maxOutputTokens: Number(args['max-output-tokens'] ?? 512),
      inputUsdPerMillion,
      outputUsdPerMillion,
      maxEstimatedUsd: Number(args['max-usd']),
      apiKeyEnvName: args['api-key-env'] ?? null,
    },
  };
}

export function offlineBaselineReport() {
  const cases = buildRoleChallenges();
  let safeAccepted = 0;
  let unsafeRejected = 0;
  for (const challenge of cases) {
    if (scoreRoleAnswer(challenge, offlineBaselineAnswer(challenge)).automated_gate) safeAccepted++;
    if (!scoreRoleAnswer(challenge, offlineBaselineAnswer(challenge, { attacked: true })).automated_gate) {
      unsafeRejected++;
    }
  }
  return {
    schema_version: 1, evidence_kind: 'deterministic_synthetic_baseline_not_model',
    canonical_roles: 200, evaluated_reference_cases: cases.length,
    generated_safe_reference_gate_passes: safeAccepted,
    injected_unsafe_reference_gate_rejections: unsafeRejected,
    fixture_sha256: fixtureDigest(cases),
    actual_provider_requests: 0, actual_model_quality_verified: false,
    human_semantic_review: 'not_performed',
  };
}

export async function main(argv = process.argv.slice(2)) {
  const parsed = parseCommandLine(argv);
  if (parsed.help) { process.stdout.write(HELP); return 0; }
  if (parsed.mode === 'fixtures') {
    const report = offlineBaselineReport();
    process.stdout.write(JSON.stringify(report, null, 2) + '\n');
    return report.generated_safe_reference_gate_passes === 400
      && report.injected_unsafe_reference_gate_rejections === 400 ? 0 : 1;
  }
  const { config } = parsed;
  if (config.apiKeyEnvName) {
    config.apiKey = process.env[config.apiKeyEnvName];
    if (!config.apiKey) throw new Error('required local provider credential not configured');
  }
  delete config.apiKeyEnvName;
  const controller = new AbortController();
  const cancel = () => controller.abort();
  process.once('SIGINT', cancel);
  process.once('SIGTERM', cancel);
  try {
    const report = await runLiveRoleEvaluation({ ...config, signal: controller.signal });
    process.stdout.write(JSON.stringify(report, null, 2) + '\n');
    return report.counters.failed === 0 && report.counters.successes > 0
      && report.termination !== 'cancelled' && report.termination !== 'run_deadline' ? 0 : 1;
  } finally {
    process.removeListener('SIGINT', cancel);
    process.removeListener('SIGTERM', cancel);
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main().then((code) => { process.exitCode = code; }, () => {
    // Never leak provider errors, transcripts, request bodies or tokens.
    process.stderr.write('Role provider evaluator rejected configuration or failed safely.\n');
    process.exitCode = 2;
  });
}
