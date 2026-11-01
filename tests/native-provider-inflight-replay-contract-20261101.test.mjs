// Static anti-regression guard for a REAL native Windows GUI scenario.
// This file does not replace executing the PowerShell/Win32/UIA test itself.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const harness = readFileSync(
  new URL('./windows-native-multistep-provider-inflight-crash-e2e-20261101.ps1', import.meta.url),
  'utf8'
);

function assertAllConnectionsCounted(source) {
  assert.match(source, /private int stopped, requestCount, rejectedCount, replyCount, connectionCount;/);
  assert.match(source, /public int ConnectionCount \{ get \{ return Interlocked\.CompareExchange\(ref connectionCount, 0, 0\);/);
  assert.match(source, /\$fixtureCsharp = \$fixtureCsharp\.Replace\(\$acceptPoint, 'Interlocked\.Increment\(ref connectionCount\); ' \+ \$acceptPoint\)/);
  assert.match(source, /if \(\$fixture\.RequestCount -ne 1 -or \$fixture\.ConnectionCount -ne 1 -or\s+\$fixture\.ReplyCount -ne 0/);
  assert.match(source, /\$result\.connections = \$fixture\.ConnectionCount/);
  assert.match(source, /\$result\.requests -ne 1 -or \$result\.connections -ne 1/);
}

function assertPostRestartNoReplay(source) {
  const checkpoint = source.indexOf('# A malformed retry would increment ConnectionCount');
  assert.ok(checkpoint >= 0, 'missing explicit malformed-retry adversarial guard');
  const post = source.slice(checkpoint, source.indexOf('$result.multistep_restart_quarantine =', checkpoint));
  assert.match(post, /Start-Sleep -Milliseconds (?:1[0-9]{3}|[2-9][0-9]{3})/);
  assert.match(post, /\$fixture\.RequestCount -ne 1 -or \$fixture\.ConnectionCount -ne 1 -or/);
  assert.match(post, /Test-Path -LiteralPath \$path -PathType Leaf/);
  assert.match(post, /throw "Recovered app replayed an uncertain provider\/tool effect/);
}

function assertNativeAndSourceIdentity(source) {
  assert.match(source, /Assert-ConstellationHiddenDesktop \$HiddenDesktopWorkerName/);
  assert.match(source, /Assert-ConstellationHiddenWindow \$hwnd \$HiddenDesktopWorkerName/);
  assert.match(source, /Assert-PinnedConstellationSources/);
  assert.match(source, /Assert-PinnedFullSourceAndAssets/);
  assert.match(source, /\$process\.Kill\(\)/);
  assert.match(source, /--resume-isolated-data-dir/);
  assert.match(source, /\$fixture\.ReplyCount -ne 0/);
  assert.match(source, /\$fixture\.Release\(\)/);
}

test('crash E2E script counts all TCP attempts, even malformed retries', () => {
  assertAllConnectionsCounted(harness);
  assertPostRestartNoReplay(harness);
  assertNativeAndSourceIdentity(harness);
});

test('negative mutation: ignoring malformed connections after restart is detected', () => {
  const mutated = harness.replace(
    '$fixture.RequestCount -ne 1 -or $fixture.ConnectionCount -ne 1 -or\n      (Test-Path',
    '$fixture.RequestCount -ne 1 -or\n      (Test-Path'
  );
  assert.notEqual(mutated, harness, 'mutation must actually change the script');
  assert.throws(() => assertPostRestartNoReplay(mutated), assert.AssertionError);
});

test('negative mutation: missing at-accept TCP accounting is detected', () => {
  const mutated = harness.replace('Interlocked.Increment(ref connectionCount); ', '');
  assert.notEqual(mutated, harness);
  assert.throws(() => assertAllConnectionsCounted(mutated), assert.AssertionError);
});

test('negative mutation: no bounded post-restart observation is detected', () => {
  const mutated = harness.replace('Start-Sleep -Milliseconds 1500', 'Start-Sleep -Milliseconds 0');
  assert.notEqual(mutated, harness);
  assert.throws(() => assertPostRestartNoReplay(mutated), assert.AssertionError);
});
