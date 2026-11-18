@echo off
rem Native Perl launcher for the vendored OpenSSL build (openssl-src).
rem
rem Cargo points OPENSSL_SRC_PERL at this file through src-tauri/.cargo/config.toml
rem so a plain `bun x tauri dev` (or any direct cargo build) finds a native
rem MSWin32 Perl without the caller exporting anything. Git's bundled MSYS Perl
rem is deliberately not used: it configures OpenSSL for the wrong platform.
rem
rem Perl is provisioned by tools/dev-perl.ps1 (Ensure-DevPerl), which Tauri's
rem beforeDevCommand runs in parallel with the first Rust build. Wait for it
rem for up to five minutes instead of failing the build, then run the same
rem provisioning here as a last resort.
setlocal
set "WC_PERL="
set /a WC_TRIES=0

:wait
call :find
if defined WC_PERL goto run
set /a WC_TRIES+=1
if %WC_TRIES% geq 60 goto provision
rem `timeout` rejects redirected stdin, which build scripts always have.
ping -n 6 127.0.0.1 >nul
goto wait

:provision
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command ". '%~dp0dev-perl.ps1'; Ensure-DevPerl | Out-Null" 1>&2
call :find
if defined WC_PERL goto run
echo dev-perl-shim: no native Perl found. Run "bun run setup:dev" or install Strawberry Perl. 1>&2
exit /b 1

:run
"%WC_PERL%" %*
exit /b %ERRORLEVEL%

:find
if exist "%LOCALAPPDATA%\WinCommander\dev-tools\strawberry-perl-5.40.5.1\perl\bin\perl.exe" set "WC_PERL=%LOCALAPPDATA%\WinCommander\dev-tools\strawberry-perl-5.40.5.1\perl\bin\perl.exe"
if not defined WC_PERL if exist "C:\Strawberry\perl\bin\perl.exe" set "WC_PERL=C:\Strawberry\perl\bin\perl.exe"
goto :eof
