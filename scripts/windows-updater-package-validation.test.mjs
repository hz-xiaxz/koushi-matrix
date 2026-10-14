import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const script = fileURLToPath(new URL('./windows-updater-package-validation.ps1', import.meta.url));
const pwsh = process.env.KOUSHI_TEST_PWSH_PATH ?? 'pwsh';
const available = spawnSync(pwsh, ['-NoProfile', '-Command', '$PSVersionTable.PSVersion.ToString()']).status === 0;

for (const scenario of ['success', 'build-fails', 'valid-rejected', 'tampered-accepted', 'wrong-key-accepted']) {
  test(`Windows packaging process exit and cleanup: ${scenario}`, { skip: !available }, () => {
    const root = mkdtempSync(path.join(tmpdir(), 'koushi-updater-exit-'));
    const runner = path.join(root, 'runner');
    mkdirSync(runner);
    const fixture = path.join(root, 'fixture.ps1');
    writeFileSync(fixture, `
$ErrorActionPreference = 'Stop'
function npm {
    if ($args -contains 'generate') {
        $key = $args[-1]
        Set-Content $key 'synthetic-private-placeholder'
        Set-Content "$key.pub" ([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('synthetic-public')))
        $global:LASTEXITCODE = 0
    } else {
        New-Item -ItemType Directory -Force 'target/x86_64-pc-windows-msvc/release/bundle/nsis' | Out-Null
        Set-Content 'target/x86_64-pc-windows-msvc/release/bundle/nsis/test.exe' 'synthetic-installer'
        Set-Content 'target/x86_64-pc-windows-msvc/release/bundle/nsis/test.exe.sig' ([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('synthetic-signature')))
        $global:LASTEXITCODE = if ($env:KOUSHI_EXIT_SCENARIO -eq 'build-fails') { 2 } else { 0 }
    }
}
function Invoke-WebRequest { }
function Get-FileHash { @{ Hash = '37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479' } }
function Expand-Archive {
    param($Archive, $DestinationPath)
    $verifier = Join-Path $DestinationPath 'minisign-win64/x86_64/minisign.exe'
    Set-Item -Path "Function:global:$verifier" -Value {
        $tampered = ($args -join ' ') -match 'tampered.exe'
        $wrong = ($args -join ' ') -match 'wrong-public.txt'
        $code = if ($tampered -or $wrong) { 1 } else { 0 }
        if ($env:KOUSHI_EXIT_SCENARIO -eq 'valid-rejected' -and -not ($tampered -or $wrong)) { $code = 1 }
        if ($env:KOUSHI_EXIT_SCENARIO -eq 'tampered-accepted' -and $tampered) { $code = 0 }
        if ($env:KOUSHI_EXIT_SCENARIO -eq 'wrong-key-accepted' -and $wrong) { $code = 0 }
        & $env:KOUSHI_EXIT_NODE -e "process.exit($code)"
    }
}
# Match GitHub Actions' pwsh wrapper, including its native exit propagation.
. $env:KOUSHI_EXIT_SCRIPT
if (Test-Path variable:\\LASTEXITCODE) { exit $LASTEXITCODE }
`);
    try {
      const result = spawnSync(pwsh, ['-NoProfile', '-NonInteractive', '-Command', `. '${fixture.replaceAll("'", "''")}'`], {
        cwd: root, encoding: 'utf8', timeout: 30_000,
        env: { ...process.env, RUNNER_TEMP: runner, KOUSHI_EXIT_SCRIPT: script,
          KOUSHI_EXIT_NODE: process.execPath, KOUSHI_EXIT_SCENARIO: scenario },
      });
      assert.ifError(result.error);
      if (scenario === 'success') {
        assert.equal(result.status, 0, result.stdout + result.stderr);
        for (const token of ['windows_updater_package_signature=ok', 'windows_updater_tampered_package=reject',
          'windows_updater_wrong_key=reject', 'windows_updater_validation=ok']) {
          assert.ok(result.stdout.includes(token), result.stdout + result.stderr);
        }
      } else {
        assert.notEqual(result.status, 0, result.stdout + result.stderr);
        assert.ok(!result.stdout.includes('windows_updater_validation=ok'));
      }
      assert.deepEqual(readdirSync(runner), [], 'Private keys and logs must be cleaned on every outcome');
      assert.ok(!(result.stdout + result.stderr).includes('synthetic-private-placeholder'));
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
