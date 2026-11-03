import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { readDevelopmentVaultState } from "./dev-service-startup";

test("development preparation distinguishes mounted state from an empty verified journal", () => {
  expect(readDevelopmentVaultState('{"state":"mounted","mountedCount":1}', 0)).toEqual({ state: "mounted", mountedCount: 1 });
  expect(readDevelopmentVaultState('{"state":"free","mountedCount":0}', 0)).toEqual({ state: "free", mountedCount: 0 });
});

test("failed, malformed and inconsistent observations preserve native processes", () => {
  for (const [output, code] of [["", 0], ['{"state":"free","mountedCount":1}', 0],
    ['{"state":"mounted","mountedCount":0}', 0], ['{"state":"free","mountedCount":0}', 1],
    ['{"state":"free","mountedCount":0}', null], ["unexpected diagnostic", 0]] as const) {
    expect(readDevelopmentVaultState(output, code).state).toBe("unknown");
  }
});

test.skipIf(process.platform !== "win32")("stopped service without a journal permits restart but uncertain Vault states do not", () => {
  const probe = `
. ./tools/dev-vault-state.ps1
$script:serviceStatus = 'Stopped'
$script:desktop = $null
$script:deny = $false
function Test-Path {
  if ($script:deny) { throw [UnauthorizedAccessException]::new('denied') }
  return $false
}
function Get-Service { return [pscustomobject]@{ Status = $script:serviceStatus } }
function Get-Process { return $script:desktop }
$results = @()
$results += (Get-WcDevelopmentVaultState).state
$script:serviceStatus = 'Running'
$results += (Get-WcDevelopmentVaultState).state
$script:serviceStatus = 'Stopped'
$script:desktop = [pscustomobject]@{ Id = 1 }
$results += (Get-WcDevelopmentVaultState).state
$script:desktop = $null
$script:deny = $true
$results += (Get-WcDevelopmentVaultState).state
ConvertTo-Json -Compress -InputObject @($results)
`;
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", probe],
    { cwd: process.cwd(), encoding: "utf8", timeout: 10_000 });
  expect(result.status).toBe(0);
  expect(JSON.parse(result.stdout.trim())).toEqual(["free", "unknown", "unknown", "unknown"]);
});
