@echo off
rem Windows PowerShell keeps a parent PowerShell 7 module path and then cannot
rem load built-in cmdlets such as Get-FileHash. Start it on its own module path.
set "PSModulePath=%USERPROFILE%\Documents\WindowsPowerShell\Modules;%ProgramFiles%\WindowsPowerShell\Modules;%SystemRoot%\System32\WindowsPowerShell\v1.0\Modules"
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0protected-dev-runner.ps1" %*
exit /b %errorlevel%
