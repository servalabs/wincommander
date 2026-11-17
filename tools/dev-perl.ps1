# Native Perl is required by the vendored OpenSSL used by SQLCipher.

function Test-NativeDevPerl {
    param([string]$PerlPath)
    if (-not $PerlPath -or -not (Test-Path -LiteralPath $PerlPath -PathType Leaf)) { return $false }
    try {
        $start = New-Object System.Diagnostics.ProcessStartInfo
        $start.FileName = $PerlPath
        $start.Arguments = '-MConfig -MFindBin -MIPC::Cmd -e "print $^O"'
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.EnvironmentVariables['LC_ALL'] = 'C'
        $start.EnvironmentVariables['LC_CTYPE'] = 'C'
        $process = [System.Diagnostics.Process]::Start($start)
        try {
            $platform = $process.StandardOutput.ReadToEnd()
            $null = $process.StandardError.ReadToEnd()
            $process.WaitForExit()
            return $process.ExitCode -eq 0 -and $platform -eq 'MSWin32'
        } finally { $process.Dispose() }
    } catch { return $false }
}

function Ensure-DevPerl {
    $version = '5.40.5.1'
    $toolRoot = Join-Path $env:LOCALAPPDATA "WinCommander\dev-tools\strawberry-perl-$version"
    $managedPerl = Join-Path $toolRoot 'perl\bin\perl.exe'
    $pathPerl = Get-Command perl -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $candidates = @($env:OPENSSL_SRC_PERL, $managedPerl, 'C:\Strawberry\perl\bin\perl.exe')
    if ($pathPerl) { $candidates += $pathPerl.Path }
    $selected = $candidates | Where-Object { Test-NativeDevPerl $_ } | Select-Object -First 1
    if (-not $selected) {
        # Package and SHA-256 are pinned from https://strawberryperl.com/releases.json.
        $url = 'https://github.com/StrawberryPerl/Perl-Dist-Strawberry/releases/download/SP_54051_64bit/strawberry-perl-5.40.5.1-64bit-portable.zip'
        $sha256 = '6619fe7eeef921ccddb4aac3972fb602a0c690a3074205b863ade998d7bc79a6'
        $archive = Join-Path ([IO.Path]::GetTempPath()) "wincommander-perl-$([guid]::NewGuid().ToString('N')).zip"
        Write-Host "Installing native Strawberry Perl $version for the OpenSSL build..."
        try {
            Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $archive
            if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $sha256) {
                throw 'Strawberry Perl download checksum does not match the pinned package.'
            }
            Expand-Archive -LiteralPath $archive -DestinationPath $toolRoot -Force
        } finally {
            Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
        }
        if (-not (Test-NativeDevPerl $managedPerl)) {
            throw 'Native Perl setup failed; the OpenSSL build requires MSWin32 Perl with FindBin and IPC::Cmd.'
        }
        $selected = $managedPerl
    }
    # Add only Perl, not Strawberry's GCC tools, to this MSVC build process.
    $perlBin = Split-Path -Parent $selected
    if (($env:Path -split ';') -notcontains $perlBin) { $env:Path = "$perlBin;$env:Path" }
    $env:OPENSSL_SRC_PERL = $selected
    Write-Host "Using native Perl: $selected"
    return $selected
}
