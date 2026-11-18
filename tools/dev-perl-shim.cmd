@echo off
rem Native Perl launcher for the vendored OpenSSL build (openssl-src).
rem
rem Cargo points OPENSSL_SRC_PERL at this file through src-tauri/.cargo/config.toml
rem so a plain `bun x tauri dev` (or any direct cargo build) finds a native
rem MSWin32 Perl without the caller exporting anything. Git's bundled MSYS Perl
rem is deliberately not used: it configures OpenSSL for the wrong platform.
rem
rem Tauri prepares Perl before Cargo starts. Direct Cargo invocations can
rem resolve or provision it with the same PowerShell helper.
setlocal
set "WC_PERL="
call :find
if defined WC_PERL goto run

set "WC_PERL_TOOLS=%~dp0"
for /f "usebackq delims=" %%P in (`powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$ErrorActionPreference = 'Stop'; . (Join-Path $env:WC_PERL_TOOLS 'dev-perl.ps1'); Ensure-DevPerl 6>$null"`) do set "WC_PERL=%%P"
if defined WC_PERL goto run
echo dev-perl-shim: no native Perl found. Run "bun run setup:dev" or install Strawberry Perl. 1>&2
exit /b 1

:run
set "LC_ALL=C"
set "LC_CTYPE=C"
"%WC_PERL%" %*
exit /b %ERRORLEVEL%

:find
if exist "%LOCALAPPDATA%\WinCommander\dev-tools\strawberry-perl-5.40.5.1\perl\bin\perl.exe" set "WC_PERL=%LOCALAPPDATA%\WinCommander\dev-tools\strawberry-perl-5.40.5.1\perl\bin\perl.exe"
if not defined WC_PERL if exist "C:\Strawberry\perl\bin\perl.exe" set "WC_PERL=C:\Strawberry\perl\bin\perl.exe"
goto :eof
