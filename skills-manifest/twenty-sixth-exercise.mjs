#!/usr/bin/env node
// Measured read-only batching of actual Constellation P10 evidence sources.
// No claim of model tokens, external service, CI run or fabricated MCP.
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const upstream = resolve(repo, '..', 'vibe-coding-skills');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const norm = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
export const twentySixthNames = Object.freeze(['token-efficient-workflow']);
const sources = Object.freeze([
  {path:'AGENTS.md', reason:'Original 200-skill task objectives and integrity constraints'},
  {path:'.github/workflows/validate.yml', reason:'Live CI source-validation commands and failure propagation'},
  {path:'.github/workflows/source-gate.mjs', reason:'Committed-source gate used in the workflow'},
  {path:'skills-manifest/audit-source-candidates.mjs', reason:'Immutable MIT source candidate identities and license provenance'},
  {path:'skills-manifest/validate.test.mjs', reason:'Exact strict receipt checks and adverse validation fixtures'},
]);
const check = (name, ok, evidence) => ({name,result:ok?'pass':'fail',evidence});

// 10 real baseline file-read operations versus 5 optimized file-read
// operations. Read all sources again for policy checks, not as a fabricated
// LLM tool-call or elapsed-time measurement.
export function measureEvidenceReads(reader, captured) {
  let baselineReads = 0;
  let optimizedReads = 0;
  let baselineBytes = 0;
  let optimizedBytes = 0;
  const baseline = new Map();
  const stable = new Map();
  const optimized = new Map();
  const cachedBytes = new Map();
  for (const item of sources) {
    const a=reader(item.path,'baseline_1'),b=reader(item.path,'baseline_2');
    if (!Buffer.isBuffer(a) || !Buffer.isBuffer(b)) throw new TypeError('Expected source bytes');
    baselineReads+=2;baselineBytes+=a.length+b.length;
    baseline.set(item.path,sha(a));stable.set(item.path,sha(a)===sha(b));
  }
  for(const item of sources) {
    const once=reader(item.path,'optimized');
    if(!Buffer.isBuffer(once)) throw new TypeError('Expected source bytes');
    optimizedReads++;optimizedBytes+=once.length;
    optimized.set(item.path,{sha256:sha(once),bytes:once.length});
    cachedBytes.set(item.path,once);
  }
  return {
    ledger:sources.map((item)=>({
      path:item.path,reason:item.reason,
      sha256:optimized.get(item.path)?.sha256,
      bytes:optimized.get(item.path)?.bytes,
      fresh_against_snapshot:captured[item.path]?.sha256===optimized.get(item.path)?.sha256,
      baseline_stable:stable.get(item.path),
      optimized_equals_baseline:baseline.get(item.path)===optimized.get(item.path)?.sha256,
    })),
    counters:{baseline_reads:baselineReads,optimized_reads:optimizedReads,
      baseline_bytes:baselineBytes,optimized_bytes:optimizedBytes},
    cachedBytes,
  };
}

export function auditEfficiency({reader,captured}) {
  const measured=measureEvidenceReads(reader,captured);
  // Reuse the exact optimized evidence bytes for the subsequent policy
  // analysis. No hidden sixth read or stale second source view is needed.
  const docs=new Map(sources.map((item)=>[item.path,
    measured.cachedBytes.get(item.path).toString('utf8')]));
  const instructions=docs.get('AGENTS.md')??'';
  const workflow=docs.get('.github/workflows/validate.yml')??'';
  const sourceGate=docs.get('.github/workflows/source-gate.mjs')??'';
  const sourceAudit=docs.get('skills-manifest/audit-source-candidates.mjs')??'';
  const regression=docs.get('skills-manifest/validate.test.mjs')??'';
  const ledgerValid=measured.ledger.length===5
    && new Set(measured.ledger.map((item)=>item.path)).size===5
    && measured.ledger.every((item)=>item.bytes>0&&item.bytes<=256*1024
      && /^[a-f0-9]{64}$/.test(item.sha256??''));
  const exact=measured.ledger.every((item)=>item.fresh_against_snapshot
    && item.baseline_stable && item.optimized_equals_baseline);
  const reduced=measured.counters.baseline_reads===10
    && measured.counters.optimized_reads===5
    && measured.counters.baseline_bytes===2*measured.counters.optimized_bytes;
  const pipeline=workflow.includes('node skills-manifest/validate.mjs')
    && workflow.includes('node --test --test-name-pattern')
    && workflow.includes('skills-manifest/validate.test.mjs')
    && !/continue-on-error:\s*true/.test(workflow);
  const tests=regression.includes("test('local read-only SHA256, frontmatter, license and executable paths match observed evidence'")
    && regression.includes("test('fabricated exercise receipt cannot pass local verification or inflate totals'")
    && regression.includes("test('a copied development skill receipt cannot masquerade as a live MCP integration'");
  const sourceGuard=sourceGate.includes("['ls-files', '-z', '--cached']")
    && sourceGate.includes('process.exitCode')
    && sourceGate.includes('UNTRACKED_REQUIRED');
  const original= instructions.includes('200 real role definitions and 200 vetted, installed/exercised skills/MCP')
    && instructions.includes('Never pad or')
    && instructions.includes('Preserve repo/history without force-push or destructive');
  const audit=sourceAudit.includes('source_checked')&&sourceAudit.includes('seen');
  return {
    purpose:'Measure redundant actual Constellation P10 repository evidence reads against single-pass digest-cached reads while preserving original strict CI, non-padding and receipt tests',
    checks:[
      check('All five selected real source identities are distinct and bounded',ledgerValid,'fixed real paths and byte SHA-256'),
      check('Measured 10 baseline source reads versus 5 optimized source reads',reduced,'instrumented readFileSync counts and bytes, not claimed LLM tokens'),
      check('Every optimized SHA and baseline source revision is exactly equal',exact,'3 independent reads on actual source and independent original snapshot'),
      check('Context ledger has useful source-specific reasons and SHA digests',
        measured.ledger.every((item)=>item.reason.length>20&&item.fresh_against_snapshot),
        'reusable five-source evidence ledger'),
      check('Original source candidate audit code remains present',audit,'existing source-review auditor'),
      check('CI validation stage retains required test and non-ignoring failure handling',pipeline,'real .github/workflows/validate.yml'),
      check('Strict verifier tests still reject forged receipts and fake MCPs',tests,'real validate.test.mjs'),
      check('200 real integrations and no-count-padding policy still exist',original,'real AGENTS.md'),
      check('Committed source gate still checks tracked paths',sourceGuard,'real CI source-gate implementation'),
      check('Read optimization does not omit or silently use stale evidence',exact&&reduced&&ledgerValid,'all selected source hashes agree'),
    ],
    observations:{
      measured:measured.counters,
      source_ledger:measured.ledger,
      provider_tokens:null,
      tool_host_calls:null,
      scope:'Only filesystem source-read operations were measured; platform model/tool usage metrics unavailable',
    },
    limitations:[
      'This exercises real repository read operations only, not the ChatGPT client tool-call or token meter',
      'No whole-repository 200-item milestone, CI cloud job, external model, or actual MCP runtime is claimed',
      'A changed source digest fails the exercise rather than treating stale cached facts as efficient',
      'Optimizes the distinct P10 source-orientation subtask, not overall agent reasoning quality',
    ],
  };
}

export function runTwentySixth(name,overrides={}) {
  if (!twentySixthNames.includes(name)) throw new Error('No approved twenty-sixth verifier');
  const inventory=JSON.parse(readFileSync(join(here,'inventory.json'),'utf8'));
  const entry=inventory.entries.find((item)=>item.id==='skill:'+name);
  if (!entry||entry.kind!=='skill'||!['source_reviewed','installed_vetted'].includes(entry.status))
    throw new Error('Not already source reviewed');
  const revision=execFileSync('git',['-C',upstream,'rev-parse','HEAD'],
    {encoding:'utf8',windowsHide:true}).trim();
  const dirty=execFileSync('git',['-C',upstream,'status','--porcelain'],
    {encoding:'utf8',windowsHide:true}).trim();
  if(!/^[a-f0-9]{40}$/.test(revision)||dirty) throw new Error('Dirty or unpinned upstream');
  const path='skills/efficiency/'+name+'/SKILL.md';
  const source=readFileSync(join(upstream,path));
  const pinned=execFileSync('git',['-C',upstream,'show',revision+':'+path],{windowsHide:true});
  const license=readFileSync(join(upstream,'LICENSE'));
  const pinnedLicense=execFileSync('git',['-C',upstream,'show',revision+':LICENSE'],{windowsHide:true});
  if(sha(source)!==entry.source.sha256||sha(norm(source))!==sha(pinned)
    ||sha(norm(license))!==sha(pinnedLicense)
    ||!license.toString('utf8').includes('MIT License')
    ||entry.license.spdx!=='MIT'||entry.license.status!=='verified'
    ||!norm(source).toString('utf8').startsWith('---\nname: '+name+'\n')
    ||!source.toString('utf8').includes('# Safety restrictions')
    ||!source.toString('utf8').includes('# Completion criteria'))
      throw new Error('Source/license/frontmatter/safety verification failed');
  const managed=join(process.env.APPDATA??'','chat-on-steroids','skills','constellation',name,'SKILL.md');
  let installedSHA=null;
  try {if(sha(readFileSync(managed))===sha(source)) installedSHA=sha(source);} catch {}
  const inputs=Object.fromEntries(sources.map((item)=>[item.path,{
    path:item.path,sha256:sha(readFileSync(resolve(repo,item.path))),
  }]));
  const reader=(file,stage)=>stage==='optimized'&&Object.hasOwn(overrides,file)
    ? Buffer.from(overrides[file]) : readFileSync(resolve(repo,file));
  const result=auditEfficiency({reader,captured:inputs});
  const runnerSHA=sha(readFileSync(fileURLToPath(import.meta.url)));
  return {
    id:'skill:'+name,
    status:installedSHA?'installed_exercised_pending_manifest_verification':'source_reviewed_uninstalled_not_qualified',
    project:'Constellation',qualification_credit:0,
    command:'node skills-manifest/twenty-sixth-exercise.mjs '+name+' --write-receipt',
    scope:'bounded_offline_measured_p10_source_read_optimization',
    purpose:result.purpose,checks:result.checks,
    outcome:result.checks.every((item)=>item.result==='pass')?'passed':'failed',
    limitations:[...result.limitations,'Pinned upstream Git origin does not independently verify publisher account identity'],
    observations:result.observations,
    upstream:{revision,path:'../vibe-coding-skills/'+path,sha256:sha(source),
      pinned_blob_sha256:sha(pinned),license_sha256:sha(license),
      license_pinned_sha256:sha(pinnedLicense),publisher_identity:'unverified'},
    inputs,installed_source_sha256:installedSHA,
    exercise_script_sha256:runnerSHA,
    runner:{path:'skills-manifest/twenty-sixth-exercise.mjs',sha256:runnerSHA},
  };
}

if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const [name,flag]=process.argv.slice(2);
  if(!twentySixthNames.includes(name)||!['--check','--write-receipt'].includes(flag)
    ||process.argv.length!==4){
    process.stderr.write('Usage: twenty-sixth-exercise.mjs token-efficient-workflow --check|--write-receipt\n');
    process.exitCode=2;
  }else{
    const report=runTwentySixth(name);
    if(report.outcome!=='passed'){
      process.stderr.write(JSON.stringify(report.checks.filter((item)=>item.result==='fail'),null,2)+'\n');
      process.exitCode=1;
    }else if(flag==='--write-receipt'){
      if(!report.installed_source_sha256)throw new Error('Not byte-identically installed');
      const file=join(here,'evidence','twenty-sixth',name+'.json');
      mkdirSync(dirname(file),{recursive:true});
      writeFileSync(file,JSON.stringify({...report,observed_on:new Date().toISOString().slice(0,10)},null,2)+'\n');
      process.stdout.write(relative(repo,file)+' sha256='+sha(readFileSync(file))+' checks='+report.checks.length+'\n');
    }else process.stdout.write(name+': '+report.checks.length+' checks passed; not yet credited\n');
  }
}
