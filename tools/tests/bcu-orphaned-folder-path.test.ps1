$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\apps\bcu-uninstaller.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'BCU uninstaller source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Resolve-BcuOrphanedFolderPath' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))

$base = Join-Path ([IO.Path]::GetTempPath()) ("bcu-path-test-" + [guid]::NewGuid().ToString('N'))
$pf = Join-Path $base 'Program Files'
$evil = Join-Path $base 'Program Files Evil'
$win = Join-Path $base 'Windows'
New-Item -ItemType Directory -Force -Path (Join-Path $pf 'Vendor'), (Join-Path $evil 'App'), $win | Out-Null
$outside = Join-Path $base 'outside'
New-Item -ItemType Directory -Force -Path $outside | Out-Null
$junction = Join-Path $pf 'Link'
cmd /c mklink /J "$junction" "$outside" | Out-Null

function Expect-Rejected([string]$Name, [string]$Path) {
    try { Resolve-BcuOrphanedFolderPath -Path $Path -Roots @($pf) | Out-Null }
    catch { return }
    throw "Expected rejection: $Name ($Path)"
}
try {
    $ok = Resolve-BcuOrphanedFolderPath -Path (Join-Path $pf 'Vendor') -Roots @($pf)
    if ($ok -ine (Join-Path $pf 'Vendor')) { throw "Valid child was altered: $ok" }
    $missing = Resolve-BcuOrphanedFolderPath -Path (Join-Path $pf 'Gone\Sub') -Roots @($pf)
    if ($missing -ine (Join-Path $pf 'Gone\Sub')) { throw 'Missing valid child must still resolve.' }
    Expect-Rejected 'root itself' $pf
    Expect-Rejected 'root with trailing slash' ($pf + '\')
    Expect-Rejected 'dot-dot escape' (Join-Path $pf '..\Windows')
    Expect-Rejected 'sibling sharing prefix' (Join-Path $evil 'App')
    Expect-Rejected 'wildcard star' (Join-Path $pf '*')
    Expect-Rejected 'wildcard question' (Join-Path $pf 'Ven?or')
    Expect-Rejected 'bracket class' (Join-Path $pf '[V]endor')
    Expect-Rejected 'junction' $junction
    Expect-Rejected 'through junction' (Join-Path $junction 'child')
    Expect-Rejected 'empty' ' '
    Write-Output 'Passed: BCU orphaned-folder path validation (strict child, no wildcards, no traversal, no reparse points).'
}
finally {
    if (Test-Path -LiteralPath $junction) { cmd /c rmdir "$junction" | Out-Null }
    Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue
}
