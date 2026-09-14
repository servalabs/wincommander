<#
  Prepare and start one WinCommander Free tag release without the GitHub
  prepare-release workflow.

  This command creates the version commit on origin/main, then creates the
  matching vX.Y.Z tag. The tag push starts .github/workflows/release.yml.

  Examples:
    .\tools\start-free-release.ps1 -Version 3.5.7
    .\tools\start-free-release.ps1 -Version 3.5.6 -ReplaceUnpublishedTag
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$')]
    [string]$Version,

    # Only use when a previous attempt created this tag but no GitHub Release.
    [switch]$ReplaceUnpublishedTag
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Stop-Release([string]$Message) {
    throw "[release] $Message"
}

function Invoke-Git([string[]]$Arguments) {
    & git @Arguments
    if ($LASTEXITCODE -ne 0) {
        Stop-Release "git $($Arguments -join ' ') failed."
    }
}

$root = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$tag = "v$Version"

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Stop-Release 'GitHub CLI (gh) is required to verify release state.'
}
Invoke-Git @('-C', $root, 'fetch', '--quiet', 'origin', 'main', '--tags')
$origin = (Invoke-Git @('-C', $root, 'remote', 'get-url', 'origin') | Out-String).Trim()
if ($origin -notmatch 'github\.com[/:](?<owner>[^/]+)/(?<name>[^/.]+)(?:\.git)?$') {
    Stop-Release "Could not determine the GitHub repository from origin: $origin"
}
$repository = "$($Matches.owner)/$($Matches.name)"

$priorErrorActionPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$published = & gh release view $tag --repo $repository 2>$null
$publishedExitCode = $LASTEXITCODE
$ErrorActionPreference = $priorErrorActionPreference
if ($publishedExitCode -eq 0) {
    Stop-Release "$tag is already published and cannot be replaced. Choose a newer version."
}
$publishedTags = (& gh api "repos/$repository/releases" --paginate --jq '.[] | select(.draft == false) | .tag_name' | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($publishedTags)) {
    Stop-Release 'Could not establish the published release baseline.'
}
$env:WINCOMMANDER_RELEASE_VERSION = $Version
$env:WINCOMMANDER_PUBLISHED_TAGS = $publishedTags
@'
const parse = (value) => {
  const match = /^(?:v)?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(value);
  if (!match) throw new Error(`Invalid semantic version: ${value}`);
  return { core: match.slice(1, 4).map(Number), pre: match[4]?.split('.') || [] };
};
const compare = (left, right) => {
  const a = parse(left), b = parse(right);
  for (let i = 0; i < 3; i++) if (a.core[i] !== b.core[i]) return a.core[i] - b.core[i];
  if (!a.pre.length || !b.pre.length) return a.pre.length ? -1 : b.pre.length ? 1 : 0;
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    if (a.pre[i] === undefined) return -1;
    if (b.pre[i] === undefined) return 1;
    if (a.pre[i] === b.pre[i]) continue;
    const an = /^\d+$/.test(a.pre[i]), bn = /^\d+$/.test(b.pre[i]);
    if (an && bn) return Number(a.pre[i]) - Number(b.pre[i]);
    if (an !== bn) return an ? -1 : 1;
    return a.pre[i].localeCompare(b.pre[i]);
  }
  return 0;
};
const target = process.env.WINCOMMANDER_RELEASE_VERSION;
const published = process.env.WINCOMMANDER_PUBLISHED_TAGS.split(/\r?\n/).map((tag) => tag.trim()).filter(Boolean);
if (published.some((tag) => compare(target, tag) <= 0)) {
  throw new Error(`Release ${target} must be newer than every published release.`);
}
'@ | node --input-type=commonjs -
Remove-Item Env:WINCOMMANDER_RELEASE_VERSION
Remove-Item Env:WINCOMMANDER_PUBLISHED_TAGS
if ($LASTEXITCODE -ne 0) { Stop-Release 'Requested version is not newer than the published release baseline.' }

$remoteTag = (& git -C $root ls-remote --tags origin "refs/tags/$tag" | Out-String).Trim()
if ($remoteTag -and -not $ReplaceUnpublishedTag) {
    Stop-Release "$tag already exists without a published release. Re-run with -ReplaceUnpublishedTag only after confirming it is the failed, unpublished tag."
}

$worktree = Join-Path ([IO.Path]::GetTempPath()) ("wincommander-free-release-" + [guid]::NewGuid().ToString('N'))
try {
    Invoke-Git @('-C', $root, 'worktree', 'add', '--detach', $worktree, 'origin/main')
    Push-Location $worktree

    $current = (& node -e "const fs=require('fs'); process.stdout.write(JSON.parse(fs.readFileSync('package.json','utf8')).version)" --input-type=commonjs 2>$null).Trim()
    if ($LASTEXITCODE -ne 0) { Stop-Release 'Could not read package.json version.' }
    if ($current -eq $Version) {
        Stop-Release "origin/main already declares $Version. This script only prepares a new version."
    }

    $update = @'
const fs = require('fs');
const version = process.env.WINCOMMANDER_RELEASE_VERSION;
const packagePath = 'package.json';
const tauriPath = 'src-tauri/commander-free/tauri.conf.json';
const cargoPath = 'src-tauri/commander-free/Cargo.toml';
const lockPath = 'src-tauri/Cargo.lock';
const packageJson = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
packageJson.version = version;
fs.writeFileSync(packagePath, `${JSON.stringify(packageJson, null, 2)}\n`);
const tauri = JSON.parse(fs.readFileSync(tauriPath, 'utf8'));
tauri.version = version;
fs.writeFileSync(tauriPath, `${JSON.stringify(tauri, null, 2)}\n`);
const cargo = fs.readFileSync(cargoPath, 'utf8');
const updatedCargo = cargo.replace(/^version = "[^"]+"$/m, `version = "${version}"`);
if (updatedCargo === cargo) throw new Error('Free Cargo package version was not found.');
fs.writeFileSync(cargoPath, updatedCargo);
const lock = fs.readFileSync(lockPath, 'utf8');
const updatedLock = lock.replace(/(\[\[package\]\]\r?\nname = "commander-free"\r?\nversion = ")[^"]+(")/, `$1${version}$2`);
if (updatedLock === lock) throw new Error('Free Cargo.lock package version was not found.');
fs.writeFileSync(lockPath, updatedLock);
'@
    $env:WINCOMMANDER_RELEASE_VERSION = $Version
    $update | node --input-type=commonjs -
    if ($LASTEXITCODE -ne 0) { Stop-Release 'Could not update all four release version files.' }
    Remove-Item Env:WINCOMMANDER_RELEASE_VERSION

    $versionCheck = @'
const fs = require('fs');
const version = process.env.WINCOMMANDER_RELEASE_VERSION;
const packageVersion = JSON.parse(fs.readFileSync('package.json', 'utf8')).version;
const tauriVersion = JSON.parse(fs.readFileSync('src-tauri/commander-free/tauri.conf.json', 'utf8')).version;
const cargo = fs.readFileSync('src-tauri/commander-free/Cargo.toml', 'utf8');
const lock = fs.readFileSync('src-tauri/Cargo.lock', 'utf8');
const cargoVersion = (cargo.match(/^version = "([^"]+)"/m) || [])[1];
const lockVersion = (lock.match(/\[\[package\]\]\r?\nname = "commander-free"\r?\nversion = "([^"]+)"/) || [])[1];
if (![packageVersion, tauriVersion, cargoVersion, lockVersion].every((value) => value === version)) {
  throw new Error(`Release version fields are not all ${version}.`);
}
'@
    $env:WINCOMMANDER_RELEASE_VERSION = $Version
    $versionCheck | node --input-type=commonjs -
    Remove-Item Env:WINCOMMANDER_RELEASE_VERSION
    if ($LASTEXITCODE -ne 0) { Stop-Release 'Release version verification failed.' }

    Invoke-Git @('-C', $worktree, 'add', 'package.json', 'src-tauri/commander-free/tauri.conf.json', 'src-tauri/commander-free/Cargo.toml', 'src-tauri/Cargo.lock')
    Invoke-Git @('-C', $worktree, '-c', 'user.name=WinCommander release operator', '-c', 'user.email=release@users.noreply.github.com', 'commit', '-m', "release: v$Version")
    Invoke-Git @('-C', $worktree, 'push', 'origin', 'HEAD:main')

    if ($remoteTag) {
        Invoke-Git @('-C', $worktree, 'push', 'origin', ":refs/tags/$tag")
    }
    $localTag = (& git -C $worktree rev-parse -q --verify "refs/tags/$tag" | Out-String).Trim()
    if ($localTag) {
        Invoke-Git @('-C', $worktree, 'tag', '-d', $tag)
    }
    Invoke-Git @('-C', $worktree, 'tag', '-a', $tag, '-m', "release: $tag")
    Invoke-Git @('-C', $worktree, 'push', 'origin', "refs/tags/$tag")
    Write-Output "Started Free release $tag. The tag push triggered the release workflow."
}
finally {
    if ((Get-Location).Path -eq $worktree) { Pop-Location }
    Remove-Item Env:WINCOMMANDER_RELEASE_VERSION -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $worktree) {
        & git -C $root worktree remove --force $worktree 2>$null
    }
}
