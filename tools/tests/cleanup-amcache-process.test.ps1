$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Invoke-WCAmcacheNativeCollector' }, $true)
if (-not $definition) { throw 'Native collector process wrapper is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))
$taskDir = Join-Path ([IO.Path]::GetTempPath()) ('wc-amcache-fixture-' + [guid]::NewGuid().ToString('N'))
$oldExe = $env:WINCMD_EXE_PATH; $oldExit = $env:WC_AMCACHE_FIXTURE_FAIL
try {
    [void](New-Item -ItemType Directory -Path $taskDir)
    $source = Join-Path $taskDir 'fixture.cs'; $exe = Join-Path $taskDir 'fixture.exe'
    @'
using System;
class Fixture {
    static int Main(string[] args) {
        if (args.Length != 1 || args[0] != "--internal-amcache-scan") return 2;
        Console.Error.Write("diagnostic text");
        Console.Out.Write("{\"status\":\"complete\"}");
        return Environment.GetEnvironmentVariable("WC_AMCACHE_FIXTURE_FAIL") == "1" ? 1 : 0;
    }
}
'@ | Set-Content -LiteralPath $source -Encoding UTF8
    $compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    & $compiler /nologo /target:winexe "/out:$exe" $source
    if ($LASTEXITCODE -ne 0) { throw 'Could not build the GUI-subsystem fixture.' }
    $env:WINCMD_EXE_PATH = $exe; $env:WC_AMCACHE_FIXTURE_FAIL = '0'
    $output = Invoke-WCAmcacheNativeCollector
    if ($output -ne '{"status":"complete"}') { throw 'Did not capture GUI executable stdout.' }
    $env:WC_AMCACHE_FIXTURE_FAIL = '1'
    if ($null -ne (Invoke-WCAmcacheNativeCollector)) { throw 'Nonzero collector exit was accepted.' }
} finally {
    $env:WINCMD_EXE_PATH = $oldExe; $env:WC_AMCACHE_FIXTURE_FAIL = $oldExit
    if (Test-Path -LiteralPath $taskDir) { Remove-Item -LiteralPath $taskDir -Recurse -Force }
}
Write-Output 'Passed: GUI executable stdout, stderr drain, exit and failure handling.'
