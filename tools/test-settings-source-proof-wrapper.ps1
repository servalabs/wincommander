[CmdletBinding()]
param([Parameter(Mandatory)][string]$ScratchRoot)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$base = (Resolve-Path -LiteralPath $ScratchRoot).Path
$env:TEMP = $base; $env:TMP = $base
$work = Join-Path $base ('settings-proof-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$source = Join-Path $work 'fake.rs'; $helper = Join-Path $work 'fake.exe'
@'
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.get(1).is_some_and(|value| value == "stage") {
        println!("{}", std::env::var("WINCMD_TEST_PROOF").unwrap_or_default());
    } else { println!("{}", args.join("|")); }
    std::process::exit(std::env::var("WINCMD_TEST_EXIT").ok().and_then(|s| s.parse().ok()).unwrap_or(0));
}
'@ | Set-Content -LiteralPath $source -Encoding ascii
& rustc.exe +1.99.1 --edition 2021 $source -o $helper
if ($LASTEXITCODE -ne 0) { throw 'Synthetic proof helper did not compile.' }
$wrapper = (Resolve-Path (Join-Path $PSScriptRoot '../src-tauri/commander-free/nsis/install-settings-generation.ps1')).Path
$stdout = Join-Path $work 'stdout.txt'; $stderr = Join-Path $work 'stderr.txt'
function Invoke-Fixture([string]$Action, [string]$Proof = '') {
    $args = @('-NoProfile', '-NonInteractive', '-File', ('"' + $wrapper + '"'), '-ExecutablePath', ('"' + $helper + '"'), '-Action', $Action)
    if ($Proof) { $args += @('-SourceProof', $Proof) }
    $p = Start-Process -FilePath 'powershell.exe' -ArgumentList $args -WindowStyle Hidden -PassThru -Wait -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    return @{ Exit = $p.ExitCode; Output = (Get-Content -Raw -LiteralPath $stdout) }
}
$env:WINCMD_TEST_EXIT='2'; $env:WINCMD_TEST_PROOF='F'+('0'*64)
$result = Invoke-Fixture 'stage'
if ($result.Exit -ne 2 -or $result.Output.Trim() -cne $env:WINCMD_TEST_PROOF) { throw 'Valid consent-required proof was not preserved.' }
$env:WINCMD_TEST_EXIT='3'; $env:WINCMD_TEST_PROOF='L'+('b'*64)
$result = Invoke-Fixture 'stage'
if ($result.Exit -ne 3 -or $result.Output.Trim() -cne $env:WINCMD_TEST_PROOF) { throw 'Existing-identity consent proof was not preserved.' }
foreach ($bad in @('Labc', ('F'+('1'*64)), ('A'+('A'*64)), ('X'+('a'*64)))) {
    $env:WINCMD_TEST_PROOF=$bad; $result = Invoke-Fixture 'stage'
    if ($result.Exit -eq 0 -or $result.Exit -eq 2 -or $result.Exit -eq 3) { throw 'Invalid proof was accepted.' }
}
$env:WINCMD_TEST_EXIT='0'; $proof='L'+('a'*64)
$result = Invoke-Fixture 'install' $proof
if ($result.Exit -ne 0 -or $result.Output.Trim() -cne "--settings-source-proof-lab|install|--offline-confirmed|$proof") { throw 'Install proof was not forwarded.' }
Write-Output 'Synthetic proof wrapper checks passed.'
