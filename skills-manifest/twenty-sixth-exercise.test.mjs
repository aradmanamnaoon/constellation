import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import test from 'node:test';
import {auditEfficiency, measureEvidenceReads, runTwentySixth,
  twentySixthNames} from './twenty-sixth-exercise.mjs';

const repo = resolve(import.meta.dirname,'..');
const sha = (bytes)=>createHash('sha256').update(bytes).digest('hex');
const paths = [
  'AGENTS.md',
  '.github/workflows/validate.yml',
  '.github/workflows/source-gate.mjs',
  'skills-manifest/audit-source-candidates.mjs',
  'skills-manifest/validate.test.mjs',
];
const real = new Map(paths.map((path)=>[path,readFileSync(resolve(repo,path))]));
const captured = Object.fromEntries(paths.map((path)=>[path,{
  path,sha256:sha(real.get(path)),
}]));
const reader = (path)=>real.get(path);

test('unique previously source-reviewed MIT candidate exercises real P10 source reads',()=>{
  assert.deepEqual(twentySixthNames,['token-efficient-workflow']);
  const report=runTwentySixth('token-efficient-workflow');
  assert.equal(report.outcome,'passed');
  assert.equal(report.checks.length,10);
  assert.equal(report.qualification_credit,0);
  assert.equal(report.upstream.publisher_identity,'unverified');
  assert.ok(report.installed_source_sha256===null
    ||report.installed_source_sha256===report.upstream.sha256);
  assert.equal(report.observations.measured.baseline_reads,10);
  assert.equal(report.observations.measured.optimized_reads,5);
  assert.equal(report.observations.measured.baseline_bytes,
    report.observations.measured.optimized_bytes*2);
  assert.equal(report.observations.provider_tokens,null);
  assert.equal(report.observations.tool_host_calls,null);
});

test('actual duplicate file reads are removed without dropping one authoritative source',()=>{
  const measured=measureEvidenceReads(reader,captured);
  assert.equal(measured.ledger.length,5);
  assert.equal(measured.counters.baseline_reads,10);
  assert.equal(measured.counters.optimized_reads,5);
  assert.ok(measured.ledger.every((item)=>item.fresh_against_snapshot
    && item.baseline_stable && item.optimized_equals_baseline));
  const changed=(path,stage)=>stage==='optimized'&&path==='AGENTS.md'
    ? Buffer.concat([real.get(path),Buffer.from('\nUNTRUSTED_CACHE_MUTATION')])
    : real.get(path);
  const negative=auditEfficiency({reader:changed,captured});
  assert.equal(negative.checks[2].result,'fail');
  assert.equal(negative.checks[9].result,'fail');
});

test('changing the CI command or relaxing failure propagation is explicitly rejected',()=>{
  const altered=new Map(real);
  altered.set('.github/workflows/validate.yml',Buffer.from(real.get('.github/workflows/validate.yml')
    .toString('utf8').replace('node skills-manifest/validate.mjs',
      'node -e "console.log(true)"')));
  const current=Object.fromEntries(paths.map((path)=>[path,{
    path,sha256:sha(altered.get(path)),
  }]));
  const report=auditEfficiency({reader:(path)=>altered.get(path),captured:current});
  assert.equal(report.checks[2].result,'pass');
  assert.equal(report.checks[5].result,'fail');
  const failOpen=new Map(real);
  failOpen.set('.github/workflows/validate.yml',Buffer.concat([
    real.get('.github/workflows/validate.yml'),Buffer.from('\ncontinue-on-error: true'),
  ]));
  const snapshots=Object.fromEntries(paths.map((path)=>[path,{
    path,sha256:sha(failOpen.get(path)),
  }]));
  assert.equal(auditEfficiency({reader:(path)=>failOpen.get(path),
    captured:snapshots}).checks[5].result,'fail');
});

test('missing strict receipt regression cannot be traded for fewer reads',()=>{
  const altered=new Map(real);
  altered.set('skills-manifest/validate.test.mjs',Buffer.from(
    real.get('skills-manifest/validate.test.mjs').toString('utf8')
      .replace('fabricated exercise receipt cannot pass local verification or inflate totals',
        'a missing adversarial test was deliberately deleted')));
  const snapshots=Object.fromEntries(paths.map((path)=>[path,{
    path,sha256:sha(altered.get(path)),
  }]));
  const result=auditEfficiency({reader:(path)=>altered.get(path),captured:snapshots});
  assert.equal(result.checks[6].result,'fail');
  const absent=auditEfficiency({reader:(path)=>path==='AGENTS.md'?
    Buffer.alloc(0):real.get(path),captured});
  assert.equal(absent.checks[0].result,'fail');
  assert.equal(absent.checks[2].result,'fail');
  assert.equal(absent.checks[7].result,'fail');
});

test('unreviewed runtime/GUI/advisory candidates cannot borrow this measured read exercise',()=>{
  for(const name of ['mcp-reliability-auditor','ui-flow-verifier',
    'ci-failure-investigator','dependency-vulnerability-auditor']){
    assert.throws(()=>runTwentySixth(name),/No approved/);
  }
});
