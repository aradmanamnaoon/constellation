#!/usr/bin/env node
// Regenerate exhaustive, conservative task traceability from the repository's
// master specification and inventories. This does not mark tasks complete.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('../../', import.meta.url)));
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const specPath = 'Codex_Master_Prompt_Local_Agent_Collaboration_Platform.md';
const master = read(specPath).split(/\r?\n/);
const ledger = read('docs/quality/acceptance-ledger.md').split(/\r?\n/);
const features = read('docs/mission/master-feature-acceptance.md').split(/\r?\n/);
const board = read('docs/mission/active-task-plan.md').split(/\r?\n/);
const roles = JSON.parse(read('catalog/roles.json')).roles;
const inventory = JSON.parse(read('skills-manifest/inventory.json')).entries;
const clean = (x) => String(x ?? '').replace(/\|/g, '\\|').replace(/\s+/g, ' ').replace(/^\[ \] /, '').trim();
const entries = [];
const sections = new Map();
const scope = {
  '00': ['Principal architect', 'AGENTS.md; docs/mission/; README.md', '00'],
  '01': ['Principal architect', 'docs/mission/; crates/; apps/desktop/', '00'],
  '02': ['Principal architect', 'AGENTS.md; docs/security/; README.md', '00'],
  '03': ['Research specialist (queued)', 'docs/architecture-decision.md; docs/security/', '00'],
  '04': ['Prime (UX wave; worker-3 startup failed)', 'apps/desktop/src/routes/; docs/design/', '01'],
  '05': ['Prime (UX wave; worker-3 startup failed)', 'apps/desktop/src/routes/; docs/design/', '01'],
  '06': ['Prime (UX wave; worker-3 startup failed)', 'apps/desktop/src/routes/; assets/; docs/design/', '01'],
  '07': ['Core worker-1 (second and fifth waves independently verified)', 'crates/; apps/desktop/src-tauri/', '02'],
  '08': ['Core worker-1 (second and fifth waves independently verified)', 'crates/constellation-core/', '02'],
  '09': ['Provider specialist (queued)', 'crates/constellation-provider-*/; apps/desktop/src-tauri/', '03'],
  '10': ['Agent loop worker-1 (third wave verified; native integration queued)', 'crates/constellation-agent-loop/; crates/constellation-workflows/', '03'],
  '11': ['Tool-security worker-1 (focused fix verified; residual queued)', 'crates/constellation-tools/; apps/desktop/src-tauri/', '02'],
  '12': ['Integrations specialist (queued)', 'skills-manifest/; catalog/', '04'],
  '13': ['Role evaluation specialist (queued)', 'catalog/; tests/', '04'],
  '14': ['Integrations specialist (queued)', 'skills-manifest/; /skills', '04'],
  '15': ['Tool-security worker-1 (focused fix verified; residual queued)', 'crates/constellation-tools/; apps/desktop/src-tauri/', '02'],
  '16': ['Runner specialist (queued)', 'crates/constellation-runner-protocol/; docs/server-runner.md', '05'],
  '17': ['Sync specialist (queued)', 'crates/constellation-sync/; crates/constellation-project-backup/', '05'],
  '18': ['Android specialist (queued)', 'apps/desktop/src-tauri/; docs/android.md', '06'],
  '19': ['Evidence/productivity specialist (queued)', 'crates/constellation-evidence/; apps/desktop/src-tauri/', '04'],
  '20': ['Feature specialist (queued)', 'crates/; apps/desktop/', '06'],
  '21': ['Security specialist (queued)', 'crates/constellation-tools/; docs/security/; tests/', '07'],
  '22': ['Perf specialist (queued)', 'crates/; apps/desktop/; docs/qa/', '07'],
  '23': ['GUI QA specialist (queued)', 'tests/windows-*; docs/qa/; apps/desktop/', '07'],
  '24': ['CI specialist (queued)', '.github/workflows/validate.yml; tests/', '07'],
  '25': ['Release specialist (queued)', 'dist/; docs/release/; tests/release-readiness.ps1', '08'],
  '26': ['Docs specialist (queued)', 'README.md; assets/; docs/', '08'],
  '27': ['Docs specialist (queued)', 'docs/; README.md', '08'],
  '28': ['Prime', 'docs/mission/; MASTER_CONTINUATION.md', '00'],
  '29': ['Prime', 'docs/mission/; Git worktrees', '00'],
  '30': ['Prime', 'docs/quality/acceptance-ledger.md; docs/qa/', '08'],
  '31': ['Prime', 'docs/mission/; docs/release/', '08'],
  '32': ['Research specialist (queued)', 'docs/; external source citations', '00'],
  '33': ['Prime', 'docs/mission/; tests/release-readiness.ps1', '08'],
};

function add({id, title, status='TODO', priority='P1', deps='P01; source-reviewed design and native boundaries', owner='Prime (queue)', files='docs/mission/', acceptance='', evidence='', blocker='', wave='02', source='original specification'}) {
  entries.push({id,title:clean(title),status,priority,deps:clean(deps),owner,files:clean(files),acceptance:clean(acceptance || `Implement the complete behavior described by ${id}, including persistence, negative and cancellation tests where applicable; verify installed app and target platform.`),evidence:clean(evidence),blocker:clean(blocker || 'No fully scoped end-to-end platform PASS attached'),wave,source});
}

// Extract explicit numbered and bullet requirements, including the original
// master product differentiator, security, release, README and GUI subclauses.
let section='00', subsection='', withinFence=false;
const serial=new Map();
for(let i=0;i<master.length;i++) {
  const line=master[i].trim();
  if(/^```/.test(line)) {withinFence=!withinFence;continue;}
  if(withinFence) continue;
  const h=line.match(/^##\s+(\d+)\.(.+)/);
  if(h){section=h[1].padStart(2,'0');subsection=h[2].trim();sections.set(section,subsection);continue;}
  if(/^###\s+/.test(line)){subsection=line.replace(/^###\s+/,'');continue;}
  // Every substantive list entry is independently traceable. Nested list
  // children are represented by their own records; the parent subsection
  // remains attached to preserve context and a testable work area.
  const m=line.match(/^(?:[-*]\s+|\d+\.\s+)(.+)/);
  if(!m || m[1].length<14) continue;
  const n=(serial.get(section)??0)+1;serial.set(section,n);
  const [owner,files,wave]=scope[section]??scope['00'];
  const title=m[1].replace(/^\*\*(.*?)\*\*:\s*/, '$1: ').trim();
  add({id:`SPEC-${section}-${String(n).padStart(3,'0')}`,title,priority:['11','15','17','18','21','23','25','30'].includes(section)?'P0':'P1',deps:`SPEC-${section} section review; target app/runner hardware; permission design`,owner,files,acceptance:`Master §${Number(section)}, ${subsection}; implement named behavior including denial, durability and platform-specific manual/native tests as appropriate. Verify against original master line ${i+1}.`,evidence:`[Original specification](../../${specPath}) line ${i+1}`,wave,source:`${specPath}:${i+1}`});
}

// 23 implementation tracks are parent coordination tasks. Narrow offline
// fixture completions still remain in progress until release acceptance.
for(const line of board){const p=line.match(/^\|\s*(P\d{2})\s*\|\s*(.*?)\s*\|\s*(.*?)\s*\|\s*(.*?)\s*\|$/);if(!p)continue;
 const id=`TRACK-${p[1]}`;const status=p[3].includes('BLOCKED')?'BLOCKED':'IN PROGRESS';
 const idx=Number(p[1].slice(1));const sec=idx<=2?'08':idx<=8?'10':idx<=10?'14':idx<=13?'09':idx<=15?'19':idx<=17?'17':idx<=19?'18':idx<=21?'21':'25';const [owner,files,wave]=scope[sec];
 add({id,title:p[2],status,priority:idx===6||idx===7||idx===8||idx===10||idx===21||idx===22?'P0':'P1',deps:`All relevant SPEC and ACC rows; earlier ${p[1]} prerequisites`,owner,files,acceptance:`Close every platform-qualified subtask, negative test and source-matched GUI/release gate linked to ${p[1]}.`,evidence:`[P board](active-task-plan.md); ${p[4]}`,blocker:p[3],wave,source:'active-task-plan.md'});
}

// Original platform acceptance rows are each independently actionable.
let acceptanceRow=0;
for(const line of ledger){const cols=line.split('|').map(x=>x.trim()); if(cols.length<13)continue;
 const id=cols[1];if(!/^[A-Z]+-\d{2}$/.test(id))continue;
 acceptanceRow += 1;
 const req=cols[2];const platform=cols[3];const prefix=id.split('-')[0];
 const sec=({CORE:'08',UX:'06',PROV:'09',AGENT:'10',TOOLS:'11',PERM:'15',ROLE:'13',SKILL:'14',RAG:'19',DOC:'19',DATA:'19',FLOW:'10',REMOTE:'16',SYNC:'17',ANDROID:'18',SEC:'21',PERF:'22',CI:'24',SHIP:'25'})[prefix]??'20';
 const [owner,files,wave]=scope[sec];
 add({id:`ACC-${id}-${String(acceptanceRow).padStart(3,'0')}`,title:`${req} (${platform})`,status:'TODO',priority:prefix==='SEC'||prefix==='SHIP'||prefix==='PERM'?'P0':'P1',deps:`Implementation of relevant SPEC §${Number(sec)}; real ${platform} target; source-bound build`,owner,files,acceptance:`Execute ${cols[5]||'required'} acceptance on ${platform}; record requirement_id, build SHA, environment, test, manual/automated, observed result, screenshot/log, verifier, timestamp and blocker. Denial/failure cases included.`,evidence:'[188-row evidence ledger](../quality/acceptance-ledger.md)',blocker:`Ledger currently ${cols[8]}; ${cols[12]??'no platform receipt'}`,wave,source:'docs/quality/acceptance-ledger.md'});
}

let inCi=false;
for(const line of features){if(line.startsWith('## Seventeen'))inCi=true;
const m=line.match(/^\|\s*(\d{2})\s*\|\s*\[ \]\s*(.*?)\s*\|\s*(.*?)\s*\|$/);if(!m)continue;
const id=`${inCi?'CI':'FEATURE'}-${m[1]}`;const linked=m[3].match(/P\d{2}/g)?.join(', ')??'';
const sec=inCi?'24':(Number(m[1])===13?'06':Number(m[1])>=27&&Number(m[1])<=30?'19':'20');const [owner,files,wave]=scope[sec];
add({id,title:m[2].replace(/\*\*/g,'').replace(/`/g,''),status:'TODO',priority:inCi||['04','05','09','10','13','17','24'].includes(m[1])?'P0':'P1',deps:linked||'Host review and source-matched acceptance',owner,files,acceptance:m[3],evidence:'[40 features + 17 CI families](master-feature-acceptance.md)',blocker:inCi?'No verified hosted run with complete job family and immutable commit':'No end-to-end source-pinned platform acceptance',wave,source:'docs/mission/master-feature-acceptance.md'});
}

// A definition existing in the catalog is not an empirical evaluation. Each
// of 200 roles needs a separately verified appropriate model + permission
// matrix before claiming a working professional specialization.
for(const role of roles){add({id:`ROLE-${role.id}`,title:`Evaluate ${role.name} role against its stated output contract`,status:'TODO',priority:'P2',deps:'TRACK-P03; TRACK-P08; FEATURE-33; real permitted model',owner:'Role evaluation specialist (queued)',files:'catalog/roles.json; catalog/roles.test.mjs; docs/qa/role-evaluations/',acceptance:`Run baseline, user task and negative/unavailable-capability scenario; verify ${role.deliverable}; record source, selected model, limits, actual output, quality and tool-authorization outcome.`,evidence:'[Role definitions](../../catalog/roles.json)',blocker:'Only 200 structurally valid prompt templates; no independently evaluated model/tool behavior',wave:'04',source:`catalog/roles.json:${role.id}`});}

// Exactly 200 non-CLI integrations are listed but qualification is different
// from file presence. Keep even previously exercised candidates pending final
// replay on the frozen source, and preserve real unverified statuses.
for(const item of inventory.filter(entry=>entry.kind!=='cli')){const qualified=item.status==='installed_vetted';
 add({id:`INTEGRATION-${item.id.replace(':','-')}`,title:`Vet, install and independently exercise ${item.id}`,status:qualified?'IN PROGRESS':'TODO',priority:'P1',deps:'TRACK-P10; verified publisher/license/source; independent scoped exercise',owner:'Integrations specialist (queued)',files:'skills-manifest/inventory.json; skills-manifest/evidence/; /skills',acceptance:`Verify exact pinned source hash, publisher, SPDX/license, trust and permission boundaries, managed installed entrypoint, independent live or appropriately scoped exercise, error/denial behavior, source-bound receipt replay; for MCP, verify actual runtime handshake.`,evidence:qualified?'[Prior skill evidence](../../skills-manifest/evidence/) (must requalify against current source)':'[Inventory](../../skills-manifest/inventory.json)',blocker:qualified?'Current source changed; strict replay may reject receipt and no runtime MCP evidence':'Source reviewed or installed alone does not earn qualification',wave:'04',source:`skills-manifest/inventory.json:${item.id}`});
}

const extra=[
 ['PLAN-001','Recover original 1,237-line master specification into repository','P0','Prime','00','Codex_Master_Prompt_Local_Agent_Collaboration_Platform.md','Verify complete 1,237 lines and SHA, compare against original library version','Original file recovered from user Library; line count verified'],
 ['PLAN-002','Generate and maintain complete task inventory without fabricating completion','P0','Prime','00','docs/mission/COMPLETE_EXECUTION_PLAN.md','Check deterministic counts and IDs, all 188 ACC rows, 40 features, 17 CI, 23 tracks, 200 roles, 200 integrations','Generator at docs/mission/generate-complete-execution-plan.mjs'],
 ['PLAN-003','Perform post-wave independent specification-to-source completeness re-audit','P0','Prime','07','docs/mission/; apps/; crates/; docs/qa/','Reconcile code with every SPEC/ACC/FEATURE/CI row and changed input hashes; record newly found gaps','No final all-requirement audit yet'],
 ['WORKER-001','Verify actual worker sessions and exclusive file ownership','P0','Prime','00','MASTER_CONTINUATION.md; docs/mission/COMPLETE_EXECUTION_PLAN.md','Compare current agents status and diff to mission assignment before credit; capture failures','worker-1 second wave active, worker-2 and worker-3 failed to start'],
 ['WORKER-002','Resolve agent-loop worker startup failure and reassign implementation','P0','Prime','03','crates/constellation-agent-loop/','Verify chat identity and actual worker patch/test; prime can implement independently','worker-2 failed to start; no agent-loop edits credited'],
 ['UX-COLOR-001','Read applicable accessible design system and color theory skills','P1','Prime (worker-3 startup failed)','01','docs/design/; skills://plugins/uiuxdesigner','Report exact skill content, three original color systems, evaluation criteria and provenance','Four relevant installed skills read; independent designer evaluation pending'],
 ['UX-COLOR-002','Create semantic light/dark/high contrast color tokens','P1','Prime (worker-3 startup failed)','01','apps/desktop/src/routes/palette.css; docs/design/','Select distinctive palette; run contrast 4.5:1 normal, 3:1 large/icons; semantic warning/disabled/focus; no uncontrolled hard-coded colors','Semantic foundation and 3 Node contrast/theme tests pass; remaining legacy CSS literal colors require migration'],
 ['UX-COLOR-003','Apply palette across every screen, native consent context and responsive variant','P1','Prime (worker-3 startup failed)','01','apps/desktop/src/routes/; apps/desktop/src-tauri/','Source-matched light/dark states, small/large window, native dialog consistency and no regressions','Major shell and controls tokenized; entire screen/native consent coverage and installed GUI still pending'],
 ['UX-COLOR-004','Verify color/UX against real GUI screenshots and accessibility tooling','P0','Prime; independent UI QA specialist queued','07','docs/qa/; tests/','Installed private Win32 screenshot and keyboard tree, compare all screens, RTL/IME, 900x600 and 200% DPI','No source-matched post-palette GUI test yet'],
 ['RELEASE-001','Choose public brand and check source/marks/licensing','P1','Release specialist (queued)','08','README.md; LICENSE; assets/','Document real distinct brand and third-party asset licenses','Brand uniqueness not verified'],
 ['RELEASE-002','Reconcile untracked crate code, generated files and lockfiles without data loss','P0','Prime','07','.gitignore; crates/; apps/desktop/','Review every untracked/modified file, include needed source/lock and exclude secrets/caches; verify no deletions','94 source paths currently outside Git index'],
 ['RELEASE-003','Run full secret, license, history, vulnerability, SBOM and binary audit','P0','Security/CI specialist (queued)','07','.github/workflows/; docs/security/; tests/','Record exact tools/results, secret/history review and dependency mitigation','Independent full supply-chain review incomplete'],
 ['RELEASE-004','Stabilize clean reviewed release commit and reproducible checkout','P0','Prime','08','.git/; docs/mission/; crates/; apps/','Review diffs, freeze source, run reproducible --locked tests and compare source hashes','Working tree actively changing; never destructively reset'],
 ['RELEASE-005','Select semantic version and update all versioned manifests','P1','Release specialist (queued)','08','CHANGELOG.md; apps/desktop/package.json; Cargo.toml; docs/release/','Same version in app/about/files/tags/release docs; choose v0.x if incomplete','No verified version alignment'],
 ['RELEASE-006','Verify intended Git remote, repository identity, target branch and permissions','P0','Prime','08','.git/config; docs/release/','Owner and permissions confirmed from authenticated remote; recorded commit and branch','git remote -v empty'],
 ['RELEASE-007','Prepare safe reviewable Git commits and push intended branch','P0','Prime','08','.git/','No data loss, secret scan, successful push with remote commit equals intended local commit SHA','Remote unconfigured; no public push'],
 ['RELEASE-008','Run and independently inspect all 17 hosted CI job families','P0','Security/CI specialist (queued)','08','.github/workflows/validate.yml; docs/qa/','Record immutable run URLs, job IDs, expected OS, success/failure logs and artifact hash','Only 9 job declarations, no hosted runs'],
 ['RELEASE-009','Build and test clean-user Windows NSIS installation and uninstall','P0','Release specialist (queued)','08','apps/desktop/src-tauri/; tests/windows-*; dist/','Installed actual setup with registry/lifecycle, consent, GUI, and upgrade tests','Existing registry collision; isolated VM needed'],
 ['RELEASE-010','Build and test Linux deb and AppImage in clean environments','P0','Release specialist (queued)','08','tests/linux-*; dist/','Reproducible source-pinned binaries, dpkg/AppImage install, ELF identity, Xvfb and clean VM GUI','Previous frozen snapshots predate current source'],
 ['RELEASE-011','Build/inspect/test macOS arm64 DMG','P0','Release specialist (queued)','08','apps/desktop/src-tauri/; dist/','macOS arm64 real GUI, app/DMG identity, signing/notarization truth','No accessible macOS target/signing evidence'],
 ['RELEASE-012','Build/inspect/test macOS x64 DMG','P0','Release specialist (queued)','08','apps/desktop/src-tauri/; dist/','macOS x64 real GUI, app/DMG identity, signing/notarization truth','No accessible macOS target/signing evidence'],
 ['RELEASE-013','Build and test release-signed Android arm64 APK','P0','Android specialist (queued)','08','apps/desktop/src-tauri/; docs/android.md; dist/','Verify apksigner, stable key, ADB install/upgrade, lifecycle, Task/runner/permission flow','No authorized signing key, emulator/device proof'],
 ['RELEASE-014','Build and validate paired Linux runner distribution','P0','Runner specialist (queued)','08','crates/constellation-runner-protocol/; dist/','Headless binary/service/scheduler install, pairing/revocation, restart, offline queue, checksums','Real runner service absent'],
 ['RELEASE-015','Generate SHA256SUMS, SBOM, provenance and asset manifests','P0','Release specialist (queued)','08','dist/; docs/release/','Exact byte hash of all source-matched platform artifacts, signed status truthful','No current-source complete asset matrix'],
 ['RELEASE-016','Create and verify public GitHub Release with correct assets','P0','Prime','08','README.md; CHANGELOG.md; docs/release/','Public GitHub release tag/commit equal tested revision; all assets uploaded, links and SHA verified','Remote/publication auth unavailable'],
 ['RELEASE-017','Independently download and install each published asset','P0','Independent QA specialist (queued)','08','docs/qa/; dist/','Fresh downloads, checksum recomputation, installed clean-machine smoke and provider/task flows','No published assets'],
 ['RELEASE-018','Publish real screenshots, demos, README and release notes','P1','Docs/release specialist (queued)','08','README.md; assets/; docs/','Truthful links, native screenshots and GIFs, platform table, privacy/limitations, license and FAQ','Needs tested native builds and release URLs'],
 ['RELEASE-019','Run final independent full-spec acceptance audit','P0','Prime + independent QA (queued)','08','docs/mission/COMPLETE_EXECUTION_PLAN.md; docs/quality/acceptance-ledger.md','Zero unjustified PASS, all mandatory checks green across 188 rows/40 features/17 CI, 200 roles and integrations','Mandatory gates remain open'],
 ['RELEASE-020','Verify public download URLs, remote HEAD, workflow URLs and SHA matches','P0','Prime','08','docs/release/; README.md','Open every public URL, compare remote commit/tag and uploaded bytes to tested local artifacts','No remote or public release configured'],
];
// Source-to-spec gap audit: additional actionable tasks independently
// identified in real checked-in/untracked implementation trees. Keep each
// grounded in an inspected source and avoid interpreting the absence of a
// lexical TODO marker as proof that a feature is implemented.
const discoveries = [
  ['D-001','Turn effect-free runner protocol into authenticated deployable Linux daemon','P0','Runner specialist (queued)','05','crates/constellation-runner-protocol/src/lib.rs; new runner service crate','Real paired daemon accepts scoped jobs with persistent queue; restart, revocation and denied writes demonstrated','Protocol src/lib.rs declares no I/O or tool execution; no runner binary source found'],
  ['D-002','Create Android-native project and safe companion lifecycle, beyond launcher icons','P0','Android specialist (queued)','06','apps/desktop/src-tauri/; docs/android.md','Release-signed APK and ADB emulator smoke, Android background/URI/security behavior','Only Android icon assets found; docs/android.md explicitly says APK unverified'],
  ['D-003','Add real multi-file PDF and office ingestion with citation provenance','P1','Evidence specialist (queued)','04','crates/constellation-evidence/src/lib.rs; apps/desktop/src-tauri/','PDF extraction/page anchors, DOCX/CSV import, bounded provenance and malicious document tests','Current evidence crate accepts caller-provided PlainText/Markdown only'],
  ['D-004','Persist/reload multi-document indexes with incremental freshness controls','P1','Evidence specialist (queued)','04','crates/constellation-evidence/; crates/constellation-core/','Durable source corpus, change/deletion detection, reindex exact changed documents, retrieval permission tests','Evidence crate states no filesystem/network/model APIs; no complete corpus user workflow verified'],
  ['D-005','Implement source-grounded model answers with hard unsupported-claim detection','P1','Research/product specialist (queued)','04','crates/constellation-evidence/; apps/desktop/src-tauri/','Cross-document QA with accurate page/span citation, unsupported state and prompt injection isolation','Lexical hits and quoted spans do not constitute source-grounded AI answers'],
  ['D-006','Add real model-directed Process and Git steps through independent native consent','P0','Agent-engine specialist (queued)','03','crates/constellation-agent-loop/; apps/desktop/src-tauri/','Provider proposes bounded Process/Git, user approves native preview, sandboxed exact effects and replay/denial tests','Model decision schema currently supports only read/create/final'],
  ['D-007','Complete durable multi-turn agent scheduler with true fail-edit-test iteration','P0','Agent-engine specialist (queued)','03','crates/constellation-agent-loop/; crates/constellation-core/; apps/desktop/src-tauri/','Live model creates plan from failed real test and produces fix; repeated bounded cycles persisted across restart and stopped by user','Current offline agent-loop explicitly disclaims autonomous desktop integration'],
  ['D-008','Install effective OS sandbox and verify process filesystem/network isolation','P0','Security specialist (queued)','02','crates/constellation-tools/; apps/desktop/src-tauri/','Adversarial malicious process cannot escape allowed workspace, spawn descendants or send unapproved network traffic','Write parent identity hardening mitigates races but does not provide kernel sandbox'],
  ['D-009','Provide atomic compare-and-swap replacement against malicious concurrent writes','P0','Security specialist (queued)','02','crates/constellation-tools/src/execution.rs; verified_open.rs','Concurrent adversarial writer test proves no stale compare-and-replace at target, including moved directories','Current Windows and Unix implementation still has post-hash replacement window'],
  ['D-010','Close first-stage native read-denial authenticated journaling gap','P0','Security specialist (queued)','02','apps/desktop/src-tauri/src/lib.rs; crates/constellation-tools/','First user denial journal persists across restart and never probes private bytes before approval','P07 evidence still reports first-stage denial journal absent'],
  ['D-011','Implement provider retry/failover and monetary spend accounting','P1','Provider specialist (queued)','03','crates/constellation-provider-adapters/; apps/desktop/src-tauri/','Provider matrix exercises bounded jitter/backoff, provider switch, real usage/cost accounting, cancel, budget denial','P11/P13 explicitly report real billing and failover remain unverified'],
  ['D-012','Make provider streaming preview/Stop real installed GUI acceptance','P0','GUI QA specialist (queued)','07','apps/desktop/src/routes/+page.svelte; tests/','Source-pinned native GUI streams partial text, stops socket, saves no failed final message','Streaming parser/native localhost tests exist but latest installed GUI acceptance absent'],
  ['D-013','Convert remaining legacy colors to semantic theme tokens across all screens','P1','Prime/UX specialist (queued)','01','apps/desktop/src/routes/studio.css; palette.css','Contrast scanner and screenshots detect no leftover light-only surfaces or unreadable dark-mode labels','Principal UI tokenized; many literal legacy greens remain'],
  ['D-014','Test native Windows consent with new theme and focus controls','P0','GUI QA specialist (queued)','07','tests/windows-installed-uia-smoke.ps1; apps/desktop/src/routes/','Private screenshot, keyboard, dialog visibility, 900x600, 200% DPI, RTL and IME with exact matching build','No new installed GUI artifact after palette/focus source edits'],
  ['D-015','Implement actual cross-device sync opt-in UI and trusted checkpoint recovery','P0','Sync specialist (queued)','05','crates/constellation-sync/; apps/desktop/src-tauri/','Live S3 conditional CAS from two devices, conflict recovery, rollback detection, opt-in activation','Sync crate explicitly disables remote CAS until service probe and has no native UI acceptance'],
  ['D-016','Add actual full-file attachment backup with one coherent snapshot boundary','P1','Backup specialist (queued)','05','crates/constellation-project-backup/; crates/constellation-core/','Crash during two-db/attachment backup restores consistent recoverable state on second host','P16 reports non-atomic cross-database snapshots and missing attachment recovery'],
  ['D-017','Implement 17 distinct verifiable CI job families and hosted matrix acceptance','P0','CI specialist (queued)','07','.github/workflows/validate.yml; tests/','17 required families on fixed source, signed artifact jobs, real hosted run IDs; failures block release','validate.yml currently defines 9 jobs / 20 matrix instances, not 17 families'],
  ['D-018','Replace misleading unfinished-settings copy with precise capability statuses','P1','Prime/product specialist (queued)','01','apps/desktop/src/routes/+page.svelte','Every Settings/Tasks screen truthfully distinguishes verified workflow steps and absent tool/runner/sync','Current advanced-limitations copy may differ from implemented partial workflow execution'],
  ['D-019','Install fresh Windows setup in disposable isolated account and validate registry lifecycle','P0','Release specialist (queued)','08','tests/windows-installed-uia-smoke.ps1; docs/qa/; dist/','Clean install, start, upgrade and uninstall, no modification of existing Constellation user registry','Existing account registry has old Constellation identity, previous tests use extracted direct EXE'],
  ['D-020','Prove every intended source and locked dependency exists in eventual Git commit','P0','Prime','08','crates/**/Cargo.lock; .github/workflows/validate.yml; tests/release-readiness.ps1','Release preflight passes committed source completeness and frozen lockfiles on clean checkout','Current `main` tree has 12/14 untracked Rust lockfiles and many sources not in Git index'],
  ['D-021','Implement versioned transactional SQLite migrations and interruption recovery','P0','Core specialist (queued follow-up)','02','crates/constellation-core/src/store.rs; crates/constellation-core/tests/','Persist schema version, upgrade every supported legacy version atomically, refuse unknown future version without partial mutations; interrupt/reopen tests and independent connection contention','Core atomic initial DDL fix has no user_version migration ledger or process-kill crash proof'],
  ['D-022','Integrate authenticated agent checkpoint storage and trusted revision floor into native task ledger','P0','Prime/native integration (queued)','03','apps/desktop/src-tauri/src/lib.rs; crates/constellation-agent-loop/src/host_steps.rs; crates/constellation-core/','Persist host checkpoint and monotonic floor transactionally with task state; HMAC/rollback and crash pending-approval journal reconciliation; stale model results cannot grant or repeat tools','Third-wave agent-loop crate only exports untrusted serialized checkpoint contract; native authoritative storage and recovery absent'],
  ['D-023','Source-matched native UI verification of failed stream provisional-text cleanup','P0','GUI QA specialist (queued)','07','apps/desktop/src/routes/+page.svelte; tests/windows-*; docs/qa/','Private Win32 delayed/invalid SSE test shows failed model delta removed, other agent replies retained, no invalid answer persisted and cancellation works under room switch','Prime implemented failure-scoped preview clearing and Svelte build passed, but no native GUI/source-matched channel test'],
];
const extraInProgress = new Set(['PLAN-002','WORKER-001','UX-COLOR-001','UX-COLOR-002','UX-COLOR-003','RELEASE-002']);
const externallyBlocked = new Set(['WORKER-002','UX-COLOR-004','RELEASE-006','RELEASE-007','RELEASE-008','RELEASE-009','RELEASE-011','RELEASE-012','RELEASE-013','RELEASE-016','RELEASE-017','RELEASE-020']);
for(const [id,title,priority,owner,wave,files,acceptance,blocker] of extra){add({id,title,status:id==='PLAN-001'?'VERIFIED DONE':extraInProgress.has(id)?'IN PROGRESS':externallyBlocked.has(id)?'BLOCKED':'TODO',priority,deps:id==='PLAN-001'?'Library source available':id.startsWith('RELEASE-')?'Preceding release gates and clean source':'Relevant SPEC and ACC requirements',owner,files,acceptance,evidence:id==='PLAN-001'?`[Restored original](../../${specPath})`:id==='PLAN-002'?'[Generator](generate-complete-execution-plan.mjs)':'[Master mission board](active-task-plan.md)',blocker:id==='PLAN-001'?'':'No final acceptance proof; '+blocker,wave,source:'2026-10-31 user continuation requirements'});}
const sourceSlicesInProgress = new Set(['D-013','D-021','D-023']);
for(const [id,title,priority,owner,wave,files,acceptance,blocker] of discoveries){add({id:`DISCOVERY-${id}`,title,status:sourceSlicesInProgress.has(id)?'IN PROGRESS':'TODO',priority,deps:'Master specification plus source-gap audit; related SPEC/ACC and feature gates',owner,files,acceptance,evidence:'[Independent inspected-source gap audit](../qa/source-gap-audit-2026-10-31.md)',blocker,wave,source:'docs/qa/source-gap-audit-2026-10-31.md'});}

const ordered=[...entries].sort((a,b)=>a.wave.localeCompare(b.wave)||a.id.localeCompare(b.id));
const idSet=new Set();for(const item of ordered){if(idSet.has(item.id))throw Error(`Duplicate task ID ${item.id}`);idSet.add(item.id);}
const counts={};for(const item of ordered)counts[item.status]=(counts[item.status]??0)+1;
const perWave=new Map();for(const item of ordered){let arr=perWave.get(item.wave)??[];arr.push(item);perWave.set(item.wave,arr);}
const esc=x=>clean(x).replace(/\[/g,'\\[').replace(/\]/g,'\\]');
const out=[];
out.push('# Constellation — COMPLETE EXECUTION PLAN', '',
 'Auto-generated from the **restored original 1,237-line master specification** and the live requirement inventories. Run `node docs/mission/generate-complete-execution-plan.mjs` after each integration wave. Change statuses only after reviewing actual source-matched evidence. This is a conservative work inventory; overlapping SPEC/FEATURE/ACC/ROLE tasks are independent **acceptance lenses**, not duplicate source ownership.',
 '',`**Total independent traceability tasks: ${ordered.length}.** ${Object.entries(counts).map(([k,v])=>`${k}: **${v}**`).join(' · ')}. No release readiness is implied.`,
 '', `**Coverage:** ${serial.size} master specification sections with ${[...serial.values()].reduce((a,b)=>a+b,0)} explicit list tasks; ${entries.filter(x=>x.id.startsWith('TRACK-')).length}/23 tracks; ${entries.filter(x=>x.id.startsWith('ACC-')).length}/188 platform acceptance rows; ${entries.filter(x=>x.id.startsWith('FEATURE-')).length}/40 features; ${entries.filter(x=>x.id.startsWith('CI-')).length}/17 CI families; ${roles.length}/200 role evaluations; ${inventory.filter(x=>x.kind!=='cli').length}/200 integrations; ${extra.length} extra execution/release tasks; ${discoveries.length} independent source-gap tasks.`,
 '', '**Worker session truth:** This deterministic generator does not connect to currently running workers. Every owner in the task inventory is a planning assignment, **not** verified current chat ownership or completion. Check the actual live worker-status tool before editing overlapping files; preserve the reported source-specific QA receipts, even if a worker has since exited.',
 '', `**Source truth:** Source checks still require a clean, reviewed Git revision, verified intended remote, hosted CI, real model behavior, and platform-specific acceptance. The inventory currently labels ${inventory.filter(x=>x.kind!=='cli'&&x.status==='installed_vetted').length}/200 skills as vetted; these are **not** automatically verified: re-run \`node skills-manifest/validate.mjs --verify-local\` to measure current source-bound strict credit. ${roles.length}/200 roles have templates but empirical live-model qualification remains separately open. The 188-row platform ledger, 40 product features and 17 hosted CI families remain separate release blockers.`,
 '', '**Priority:** P0 security, native authorization, CI/source/release dependency; P1 core functionality and user experience; P2 empirically evaluating catalog role behavior. **Status:** VERIFIED DONE is limited to direct, narrowly scoped source/artifact proof; a source-only/fixture-only PASS never closes a full platform acceptance task.',
 '', '## Worker coverage and ownership audit', '',
 '| Role | Active chat | Owned files | Risk / next reassignment |', '| --- | --- | --- | --- |',
 '| Prime | Query current worker status | Mission plan, integration, CI/release gate coordination | Dirty worktree and incomplete release acceptance |',
 '| Security/path specialist | See actual live worker status and source QA | `crates/constellation-tools/` and native trust bridge | Real OS isolation, rollback and installed-app verification must be evidenced |',
 '| Core persistence specialist | See actual live worker status and source QA | `crates/constellation-core/` | Power-loss, supported legacy formats and native restart acceptance still open |',
 '| UX/color specialist | See actual live worker status and source QA | `apps/desktop/src/routes/`, design palette and QA | Installed-GUI keyboard/RTL/IME, theme and responsiveness still open |',
 '| Agent loop specialist | See actual live worker status and source QA | `crates/constellation-agent-loop/`, native model/tool bridge | Durable production provider-to-tool-to-model continuation and consent still open |',
 '| Provider specialist | See actual live worker status and source QA | `crates/constellation-provider-adapters/`, `constellation-provider-probe/` | Real API, transport policy, cancellation and billing still unverified |',
 '| Other specialisms | See actual live worker status | Queued per rows below; no ownership assumed | Assign disjoint source paths after checking active workers |',
 '', '## Execution waves and task-by-task acceptance', '');
for(const [wave,items] of [...perWave].sort(([a],[b])=>a.localeCompare(b))){out.push(`### Wave ${wave} — ${items.length} tasks`,'');
 const byOwner=new Map();for(const e of items){const arr=byOwner.get(e.owner)??[];arr.push(e);byOwner.set(e.owner,arr);}
 for(const [owner,group] of byOwner){out.push(`#### Workstream — ${esc(owner)} (${group.length} tasks)`,'');
  for(const e of group){const mark=e.status==='VERIFIED DONE'?'x':' ';
  out.push(`- [${mark}] **${e.id}** — **${esc(e.title)}**`, `  - **Status:** ${e.status} · **Priority:** ${e.priority} · **Worker/chat:** ${esc(e.owner)} · **Dependencies:** ${esc(e.deps)}`, `  - **Files/components:** ${esc(e.files)} · **Acceptance/tests:** ${esc(e.acceptance)}`, `  - **Evidence:** ${e.evidence||'No source-matched end-to-end evidence'} · **Remaining blocker:** ${esc(e.blocker||'None for this scoped task')}`);
  }
  out.push('');
 }
}
out.push('## Reconciliation protocol','',
 '1. Before each wave, confirm current `git status --porcelain=v1`, relevant code/lockfile hashes, `AGENTS.md`, actual `agents status`, free space, source and accepted scope. Prevent duplicate file ownership.',
 '2. Review each worker patch and test logs independently. Re-run targeted negative tests, rustfmt/strict Clippy, Svelte checks, source-bound GUI/IME/accessibility and platform packaging as required. Record observed SHA and environment.',
 '3. Check all SPEC/ACC/FEATURE/CI/ROLE/INTEGRATION and release rows against the source and original paragraphs; create new task IDs for newly discovered subclauses. Preserve original failures and avoid claiming coverage from mere source presence.',
 '4. Freeze a reviewed source revision, rebuild exact artifacts on available platforms, verify credentials/external conditions and public release separately. Never mark a mandatory gate complete until genuine evidence exists.',
 '5. Regenerate this matrix after each integration wave and append source hashes, worker reports, validations and external blockers in `MASTER_CONTINUATION.md` and `active-task-plan.md`. If tasks are added by humans, add them to this generator’s source arrays before regenerating.',
 '', '## Explicit release hold', '',
 'The project is **not finished**, no verified remote push or public release exists, and the mandatory real-provider, native OS containment, 200 integrations, 200 role evaluations, 40 features, 188 acceptance rows, 17 hosted job families and Android/macOS platform gates remain open. Release steps are executable tasks rather than optimistic claims.', '');
const path=resolve(root,'docs/mission/COMPLETE_EXECUTION_PLAN.md');
const document=out.join('\n');
console.log('Planning document byte count before writing:',Buffer.byteLength(document));
if(Buffer.byteLength(document)>10*1024*1024)throw Error('Unexpectedly large planning document');
if(process.argv.includes('--check')) {
  const current=readFileSync(path,'utf8');
  if(current!==document) {
    console.error('COMPLETE_EXECUTION_PLAN.md is stale. Re-run the plan generator, review diff, then commit both files.');
    process.exitCode=1;
  } else {
    console.log('Verified deterministic plan content and unique IDs; no files changed.');
  }
} else {
  writeFileSync(path,document,'utf8');
  console.log(JSON.stringify({file:path,total:ordered.length,counts,sections:serial.size,sectionsTasks:[...serial.values()].reduce((a,b)=>a+b,0),waves:[...perWave].map(([wave,e])=>[wave,e.length]),bytes:Buffer.byteLength(document)},null,2));
}
