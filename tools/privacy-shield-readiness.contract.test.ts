// SPDX-License-Identifier: AGPL-3.0-or-later
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

test("Shield receipt rejects stale, malformed, wrong-PID and prior-process evidence", () => {
    const source = resolve(import.meta.dir, "../src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1");
    // Parse only, then evaluate the single pure predicate definition. No launcher,
    // camera probe, user path, registry, process enumeration, or service executes.
    const command = `
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:WC_SHIELD_TEST_SOURCE, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'PowerShell parse errors' }
$predicate = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Test-PrivacyShieldReadyReceipt' }, $true)
if (-not $predicate) { throw 'Missing pure readiness predicate' }
. ([scriptblock]::Create($predicate.Extent.Text))
$good = @{ pid=44; startedAt=100; updatedAt=109; ready=$true }
if (-not (Test-PrivacyShieldReadyReceipt $good 44 99 110)) { throw 'Fresh inference rejected' }
$cases = @(
    $null,
    @{},
    @{ pid=45; startedAt=100; updatedAt=109; ready=$true },
    @{ pid=44; startedAt=98; updatedAt=109; ready=$true },
    @{ pid=44; startedAt=100; updatedAt=104; ready=$true },
    @{ pid=44; startedAt=100; updatedAt=112; ready=$true },
    @{ pid=44; startedAt=100; updatedAt=99; ready=$true },
    @{ pid=44; startedAt=100; updatedAt=109; ready=$false }
)
foreach ($receipt in $cases) {
    if (Test-PrivacyShieldReadyReceipt $receipt 44 99 110) { throw 'Invalid readiness accepted' }
}
Write-Output 'fresh=1 invalid=8 passed'
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], {
        encoding: "utf8", timeout: 15_000,
        env: { ...process.env, WC_SHIELD_TEST_SOURCE: source },
    });
    expect(result.error).toBeUndefined();
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("fresh=1 invalid=8 passed");
});
