import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const inventoryScript = "src-tauri/commander-free/scripts/modules/apps/winget.ps1";
const inventorySource = readFileSync(inventoryScript, "utf8");
const installerPanel = readFileSync("src/panels/apps/components/AppInstallerPanel.tsx", "utf8");

describe("Packages & Apps installed-state inventory", () => {
  test.skipIf(process.platform !== "win32")("matches normal ARP names without confusing two products from one publisher", () => {
    const script = `
$ErrorActionPreference='Stop'
$tokens=$null; $parseErrors=$null
$scriptPath=Join-Path (Get-Location) '${inventoryScript.replaceAll("\\", "/")}'
$ast=[Management.Automation.Language.Parser]::ParseFile($scriptPath,[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'Invalid Winget inventory script' }
foreach ($functionName in @('ConvertTo-AppMatchText','Get-ManifestDisplayNames','Test-ManifestDisplayNameMatch','Find-ManifestInstallationMatch')) {
  $finder={ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $functionName }.GetNewClosure()
  $definition=$ast.Find($finder,$true)
  if ($null -eq $definition) { throw "Missing inventory matcher: $functionName" }
  Invoke-Expression $definition.Extent.Text
}
$fdm=@{ id='SoftDeluxe.FreeDownloadManager'; name='Free Download Manager' }
$quickShare=@{ id='Google.QuickShare'; name='Google Quick Share' }
$git=@{ id='Git.Git'; name='Git' }
$fdmArp=[pscustomobject]@{ Id='ARP\\Registry\\FDM'; Name='Free Download Manager 6.24'; Version='6.24'; Source='registry' }
$quickShareArp=[pscustomobject]@{ Id='ARP\\Registry\\QuickShare'; Name='Quick Share'; Version='1.0.2697.0'; Source='registry' }
$gitArp=[pscustomobject]@{ Id='ARP\\Registry\\Git'; Name='Git version 2.47.1'; Version='2.47.1'; Source='registry' }
$googleDrive=[pscustomobject]@{ Id='ARP\\Registry\\GoogleDrive'; Name='Google Drive'; Version='100.0'; Source='registry' }
if (-not (Find-ManifestInstallationMatch -App $fdm -RegistryItems @($fdmArp))) { throw 'Free Download Manager ARP record did not match.' }
if (-not (Find-ManifestInstallationMatch -App $quickShare -RegistryItems @($quickShareArp))) { throw 'Quick Share ARP record did not match.' }
if (-not (Find-ManifestInstallationMatch -App $git -RegistryItems @($gitArp))) { throw 'Git ARP version record did not match.' }
if (Find-ManifestInstallationMatch -App $quickShare -RegistryItems @($googleDrive)) { throw 'Google Drive falsely matched Google Quick Share.' }
Write-Output 'PASS'
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
    expect(result.stderr.trim()).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe("PASS");
  });

  test("uses a direct read-only ARP fallback and removes publisher-only matching", () => {
    expect(inventorySource).toContain("function Get-WindowsUninstallInventory");
    expect(inventorySource).toContain("$registryInstalled = Get-WindowsUninstallInventory");
    expect(inventorySource).toContain("Find-ManifestInstallationMatch");
    expect(inventorySource).not.toContain("[regex]::Escape($publisher)");
  });

  test("rechecks inventory when the Packages & Apps panel is opened", () => {
    const panelInventoryEffect = installerPanel.slice(
      installerPanel.indexOf("Entering Packages & Apps is an explicit request"),
      installerPanel.indexOf("const openUpdates", installerPanel.indexOf("Entering Packages & Apps is an explicit request")),
    );
    expect(panelInventoryEffect).toContain("await runAppInventoryScan(true);");
    expect(panelInventoryEffect).toContain("await waitForAppInventoryScan();");
    expect(panelInventoryEffect).not.toContain("catalogIsFresh");
  });
});
