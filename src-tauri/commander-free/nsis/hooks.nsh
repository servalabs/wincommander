; ──────────────────────────────────────────────────────────────────────────────
; NSIS_HOOK_PREINSTALL
;   Runs at the start of the Install section, before any files are written.
;   1. Erases the stale per-user (HKCU) install entry left by pre-v2.9.2 builds
;      that used the Tauri default currentUser mode. Without this, a reinstall
;      after upgrading from an old build would cause RestorePreviousInstallLocation
;      to restore the AppData path instead of defaulting to Program Files.
;   2. Safety redirect: if $INSTDIR still resolved to AppData (e.g. a stale
;      HKLM entry written before the perMachine switch), redirect both $INSTDIR
;      and the active output path to %ProgramFiles%\WinCommander before any
;      files are written.
; ──────────────────────────────────────────────────────────────────────────────
!define WC_INSTALL_DIR "$PROGRAMFILES64\WinCommander"
!define WC_SERVICE_EXE "${WC_INSTALL_DIR}\wincommander-svc.exe"
!define WC_BUNDLED_SERVICE "$INSTDIR\resources\wincommander-svc.exe"

!macro NSIS_HOOK_PREINSTALL
  ; Releases use one non-customizable machine location.  The service's SCM
  ; ImagePath is therefore stable across fresh install, repair, and update.
  StrCpy $INSTDIR "${WC_INSTALL_DIR}"
  SetOutPath "$INSTDIR"

  ; An in-use service executable cannot be replaced safely. Stop the prior
  ; instance and wait for SCM to confirm SERVICE_STOPPED; a timeout aborts the
  ; update instead of leaving an old process with new files beside it.
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -Command "$svc = Get-Service -Name ''WinCommanderSvc'' -ErrorAction SilentlyContinue; if ($null -ne $svc -and $svc.Status -ne ''Stopped'') { Stop-Service -Name ''WinCommanderSvc'' -ErrorAction Stop; $svc.WaitForStatus([System.ServiceProcess.ServiceControllerStatus]::Stopped, [TimeSpan]::FromSeconds(30)) }"'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
    Abort "WinCommander service could not stop. Close dependent sessions and run the installer again."
  ${EndIf}
  ; --- 1. Remove stale per-user install location from old currentUser builds --
  DeleteRegValue HKCU "Software\servalabs.com\WinCommander" ""
  DeleteRegKey /ifempty HKCU "Software\servalabs.com\WinCommander"
  DeleteRegKey /ifempty HKCU "Software\servalabs.com"

  ; --- 2. Migrate users from a prior per-user (AppData) install ----------------
  ; Pre-perMachine builds installed to %LOCALAPPDATA%\WinCommander, leaving a
  ; duplicate Apps & Features entry, a per-user Start-Menu shortcut, a per-user
  ; Run-key autostart, and the old binaries behind once we move to Program
  ; Files. Clean those up so the machine ends with a single, machine-wide
  ; install. User DATA in that folder (settings.json, logs, icon-cache) is
  ; preserved — only the old program binaries + uninstaller are removed.
  ${If} ${FileExists} "$LOCALAPPDATA\WinCommander\${MAINBINARYNAME}.exe"
    ; Old per-user Apps & Features (uninstall) entry
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander"
    ; Old per-user autostart pointing at the AppData exe (the app re-adds a
    ; Program Files Run entry on its next launch)
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "WinCommander"
    ; Old per-user Start-Menu shortcut(s)
    Delete "$SMPROGRAMS\WinCommander.lnk"
    Delete "$APPDATA\Microsoft\Windows\Start Menu\Programs\WinCommander.lnk"
    ; Old program binaries + uninstaller (NOT settings.json / logs / icon-cache)
    Delete "$LOCALAPPDATA\WinCommander\${MAINBINARYNAME}.exe"
    Delete "$LOCALAPPDATA\WinCommander\*.dll"
    Delete "$LOCALAPPDATA\WinCommander\uninstall.exe"
    Delete "$LOCALAPPDATA\WinCommander\Uninstall WinCommander.exe"
    RMDir /r "$LOCALAPPDATA\WinCommander\resources"
  ${EndIf}

  ; --- 3. Record pre-update desktop-shortcut state ----------------------------
  ; Tauri recreates the desktop shortcut on every install/update. Record whether
  ; it existed BEFORE so POSTINSTALL can remove it when absent — prevents the
  ; app being revealed on the desktop in hidden or decoy mode after an update.
  ; Only applies to updates (binary already present); fresh installs are exempt.
  ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 "0"
    ${If} ${FileExists} "$COMMONDESKTOP\WinCommander.lnk"
      StrCpy $R0 "1"
    ${EndIf}
    ${If} ${FileExists} "$COMMONDESKTOP\WinCommander Free.lnk"
      StrCpy $R0 "1"
    ${EndIf}
    ${If} ${FileExists} "$COMMONDESKTOP\WinCommander Pro.lnk"
      StrCpy $R0 "1"
    ${EndIf}
    ${If} ${FileExists} "$DESKTOP\WinCommander.lnk"
      StrCpy $R0 "1"
    ${EndIf}
    ${If} ${FileExists} "$DESKTOP\WinCommander Free.lnk"
      StrCpy $R0 "1"
    ${EndIf}
    ${If} ${FileExists} "$DESKTOP\WinCommander Pro.lnk"
      StrCpy $R0 "1"
    ${EndIf}
    WriteRegDWORD HKLM "Software\servalabs.com\WinCommander\Installer" "HadDesktopShortcut" $R0
  ${EndIf}
!macroend

; ──────────────────────────────────────────────────────────────────────────────
; NSIS_HOOK_POSTINSTALL
;   Runs after all files have been written to $INSTDIR (elevated context).
;   1. Unblocks the main EXE (removes Zone.Identifier ADS / Mark of the Web)
;      so SmartScreen does not block launch on first run.
;   2. (removed) Defender ExclusionPath was removed (DN-01): a name-keyed
;      exclusion tied to the install directory was a tell that AV tooling
;      could enumerate and that survived uninstall — removed in P3.
;   3. Writes an HKLM RunOnce entry as a safety net for the finish-page
;      "Launch WinCommander" checkbox. The Tauri template calls
;      `nsis_tauri_utils::RunAsUser` from `RunMainBinary`, which relies on
;      WMIC for its privilege-drop COM dance. WMIC is removed by default on
;      Windows 11 24H2+ (build 26000+), so the launch silently no-ops on
;      those systems — install completes, checkbox stays ticked, no app
;      appears. The RunOnce here ensures the app starts at the next logon
;      regardless; after that first launch, the app's React effect writes
;      HKCU\...\Run (with --minimized) and normal silent autostart takes
;      over. RunOnce auto-deletes itself after firing, so it never
;      duplicates HKCU\Run on subsequent logons.
;   4. Schedules a one-shot Task Scheduler task (WinCommanderLaunchOnce) that
;      fires 5 seconds after the installer exits, running in the interactive
;      user's context (-LogonType Interactive -RunLevel Limited). This gives
;      an IMMEDIATE launch on Windows 11 24H2+ where WMIC-based RunAsUser
;      silently no-ops. The app cleans up this task on its own first run.
;      RunOnce (step 3) remains as a belt-and-suspenders fallback.
; ──────────────────────────────────────────────────────────────────────────────
!macro NSIS_HOOK_POSTINSTALL
  ; Tauri stages resources below $INSTDIR\resources. Copy the service to the
  ; fixed root before configuring SCM, so ImagePath is always the quoted
  ; Program Files executable rather than a bundler implementation detail.
  IfFileExists "${WC_BUNDLED_SERVICE}" +2 0
    DetailPrint "The bundled WinCommander service executable is missing."
    Abort "WinCommander service payload is missing; the installation was not completed."
  ClearErrors
  CopyFiles /SILENT "${WC_BUNDLED_SERVICE}" "${WC_INSTALL_DIR}"
  ${If} ${Errors}
    Abort "WinCommander service executable could not be installed."
  ${EndIf}

  ; No certificate is required: LocalSystem is the SCM identity and the
  ; service independently authenticates every named-pipe peer/broker request.
  ; ERROR_SERVICE_EXISTS (1073) is expected during an update; every other SCM
  ; failure is shown to the installer user rather than silently ignored.
  nsExec::ExecToStack 'sc create WinCommanderSvc binPath= ""${WC_SERVICE_EXE}"" start= auto obj= LocalSystem'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
  ${AndIf} $R8 != 1073
    Abort "WinCommander service could not be created."
  ${EndIf}
  nsExec::ExecToStack 'sc config WinCommanderSvc binPath= ""${WC_SERVICE_EXE}"" start= auto obj= LocalSystem'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
    Abort "WinCommander service could not be configured."
  ${EndIf}
  nsExec::ExecToStack 'sc failure WinCommanderSvc reset= 86400 actions= restart/5000/restart/5000/none/0'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
    Abort "WinCommander service recovery could not be configured."
  ${EndIf}
  nsExec::ExecToStack 'sc start WinCommanderSvc'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
  ${AndIf} $R8 != 1056
    Abort "WinCommander service could not be started."
  ${EndIf}
  ; --- 1. Unblock EXE (remove Zone.Identifier alternate data stream) ----------
  nsExec::ExecToLog 'powershell.exe -NonInteractive -NoProfile -WindowStyle Hidden \
    -Command "Get-ChildItem -Path ''$INSTDIR'' -Recurse -Include *.exe,*.dll | \
    Unblock-File -ErrorAction SilentlyContinue"'

  ; --- 3. RunOnce safety net for the finish-page launch -----------------------
  ; Value is the quoted EXE path so RunOnce treats it as a single token even
  ; when $INSTDIR contains spaces. ${MAINBINARYNAME} expands to the actual
  ; binary name set by the Tauri bundler (currently wincommander-free).
  ; No --minimized here: first launch after install should open the window.
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\RunOnce" \
    "WinCommanderFirstLaunch" '"$INSTDIR\${MAINBINARYNAME}.exe"'

  ; --- 3b. Persistent all-users silent autostart (perMachine) -----------------
  ; HKLM\Run fires at EVERY user's interactive logon — unlike HKCU\Run, which
  ; the app writes only for the user who ran it. This is what makes autostart
  ; apply to all accounts on a per-machine install. --minimized = tray-only,
  ; no window, no popup. If the app's own HKCU autostart also fires in the same
  ; session, the single-instance guard (per logon session) dedups it.
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Run" \
    "WinCommander" '"$INSTDIR\${MAINBINARYNAME}.exe" --minimized'

  ; --- 4. Immediate launch via one-shot scheduled task (Windows 24H2+ fix) ----
  ; Runs in the interactive user's session 5 seconds after the installer exits.
  ; The app deletes this task on startup (see lib.rs). Gracefully no-ops if no
  ; interactive session is present (headless/server installs).
  ; Use the built-in Users group as the task principal, matching the app's
  ; machine-wide autostart task. Resolving a username here can select the
  ; separate administrator account used for UAC or fail in RDP contexts, which
  ; leaves a GUI process outside the desktop user's interactive session. Pass
  ; the bare exe path to -Execute (no embedded quotes; Task Scheduler handles
  ; spaces) — embedding quotes made the task target a non-existent quoted path.
  ; Keep this task Limited. The highestAvailable app manifest lets a standard
  ; Partner retain their own Windows token; forcing Highest here would either
  ; require an administrator or make the service authorize the wrong session.
  nsExec::ExecToLog 'powershell.exe -NonInteractive -NoProfile -WindowStyle Hidden \
    -Command "$a = New-ScheduledTaskAction -Execute ''$INSTDIR\${MAINBINARYNAME}.exe''; \
    $t = New-ScheduledTaskTrigger -Once -At ((Get-Date).AddSeconds(5)); \
    $p = New-ScheduledTaskPrincipal -GroupId ''S-1-5-32-545'' -RunLevel Limited; \
    $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero); \
    Register-ScheduledTask -TaskName ''WinCommanderLaunchOnce'' \
      -Action $a -Trigger $t -Principal $p -Settings $s -Force -ErrorAction SilentlyContinue | Out-Null; \
    Start-ScheduledTask -TaskName ''WinCommanderLaunchOnce'' -ErrorAction SilentlyContinue"'

  ; --- 5. Honour pre-update shortcut state ------------------------------------
  ; The app writes HKLM\SOFTWARE\WinCommander!HiddenMode (REG_DWORD, 1 = active)
  ; via runtime_visibility when hide-mode is engaged.  Because the app uses the
  ; 64-bit hive and NSIS runs as a 32-bit process, SetRegView 64 is required to
  ; reach the non-redirected view; we restore it afterwards.
  ; When HiddenMode == 1 every shortcut variant is removed unconditionally —
  ; Start-menu AND desktop — so an update cannot resurrect a hidden app.
  ; When HiddenMode == 0 / absent, the existing HadDesktopShortcut path applies
  ; (only desktop shortcuts are removed if they were absent before the update).
  SetRegView 64
  ClearErrors
  ReadRegDWORD $R1 HKLM "SOFTWARE\WinCommander" "HiddenMode"
  SetRegView lastused  ; KT: restore default view; avoids polluting later reg ops

  ${IfNot} ${Errors}
  ${AndIf} $R1 == 1
    ; Hidden mode is active — wipe all shortcut variants (desktop + Start-menu)
    ; so the update does not expose the app's presence anywhere in the shell.
    Delete "$COMMONDESKTOP\WinCommander.lnk"
    Delete "$COMMONDESKTOP\WinCommander Free.lnk"
    Delete "$COMMONDESKTOP\WinCommander Pro.lnk"
    Delete "$DESKTOP\WinCommander.lnk"
    Delete "$DESKTOP\WinCommander Free.lnk"
    Delete "$DESKTOP\WinCommander Pro.lnk"
    Delete "$SMPROGRAMS\WinCommander.lnk"
    Delete "$SMPROGRAMS\WinCommander Free.lnk"
    Delete "$SMPROGRAMS\WinCommander Pro.lnk"
    Delete "$COMMONPROGRAMS\WinCommander.lnk"
    Delete "$COMMONPROGRAMS\WinCommander Free.lnk"
    Delete "$COMMONPROGRAMS\WinCommander Pro.lnk"
    ; Clean up the installer temp key (no longer needed when hidden)
    DeleteRegKey HKLM "Software\servalabs.com\WinCommander\Installer"
  ${Else}
    ; Not hidden — honour the pre-update desktop-shortcut state only.
    ; If the desktop shortcut was absent before this update (PREINSTALL step 4
    ; recorded 0), delete the shortcut Tauri just recreated so a hidden/decoy-mode
    ; install is not exposed by a stray desktop icon. The temp registry key is
    ; cleaned up regardless. No-ops on fresh installs (key was never written).
    ClearErrors
    ReadRegDWORD $R0 HKLM "Software\servalabs.com\WinCommander\Installer" "HadDesktopShortcut"
    ${IfNot} ${Errors}
      ${If} $R0 == 0
        Delete "$COMMONDESKTOP\WinCommander.lnk"
        Delete "$COMMONDESKTOP\WinCommander Free.lnk"
        Delete "$COMMONDESKTOP\WinCommander Pro.lnk"
        Delete "$DESKTOP\WinCommander.lnk"
        Delete "$DESKTOP\WinCommander Free.lnk"
        Delete "$DESKTOP\WinCommander Pro.lnk"
      ${EndIf}
      DeleteRegKey HKLM "Software\servalabs.com\WinCommander\Installer"
    ${EndIf}
  ${EndIf}
!macroend

; ──────────────────────────────────────────────────────────────────────────────
; NSIS_HOOK_PREUNINSTALL
;   Runs at the start of the Uninstall section, before any files are deleted.
;   1. Clears the RunOnce safety net written in POSTINSTALL.
;   2. Removes the one-shot launch scheduled task.
;   3. Comprehensive shortcut + registry cleanup so the uninstaller leaves no
;      orphaned artefacts — including .wc-hidden files and __WC_Hidden registry
;      entries left behind when WinCommander was hidden at uninstall time.
; ──────────────────────────────────────────────────────────────────────────────
!macro NSIS_HOOK_PREUNINSTALL
  ; Stop first and wait for SERVICE_STOPPED before deletion, otherwise SCM can
  ; retain a live process or a marked-for-delete record after uninstall.
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -Command "$svc = Get-Service -Name ''WinCommanderSvc'' -ErrorAction SilentlyContinue; if ($null -ne $svc -and $svc.Status -ne ''Stopped'') { Stop-Service -Name ''WinCommanderSvc'' -ErrorAction Stop; $svc.WaitForStatus([System.ServiceProcess.ServiceControllerStatus]::Stopped, [TimeSpan]::FromSeconds(30)) }"'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
    Abort "WinCommander service could not stop. Close dependent sessions and run the uninstaller again."
  ${EndIf}
  nsExec::ExecToStack 'sc delete WinCommanderSvc'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
  ${AndIf} $R8 != 1060
    Abort "WinCommander service could not be removed."
  ${EndIf}
  Delete "${WC_SERVICE_EXE}"
  ; --- 1. Remove RunOnce safety-net entry -------------------------------------
  DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\RunOnce" \
    "WinCommanderFirstLaunch"

  ; --- 2. Remove one-shot launch task ----------------------------------------
  nsExec::ExecToLog 'powershell.exe -NonInteractive -NoProfile -WindowStyle Hidden \
    -Command "Unregister-ScheduledTask -TaskName ''WinCommanderLaunchOnce'' \
    -Confirm:$false -ErrorAction SilentlyContinue"'

  ; --- 2a. Remove the retired reopen helper task and config -----------------
  ; Older releases registered these names for the standalone relauncher.
  nsExec::ExecToLog 'powershell.exe -NonInteractive -NoProfile -WindowStyle Hidden \
    -Command "Unregister-ScheduledTask -TaskName ''Sys Health Checker'' -Confirm:$false -ErrorAction SilentlyContinue; \
    Unregister-ScheduledTask -TaskName ''WinCommander Input Service'' -Confirm:$false -ErrorAction SilentlyContinue; \
    Remove-Item -LiteralPath ''$env:ProgramData\\WinCommander\\reopen.cfg'' -Force -ErrorAction SilentlyContinue"'

  ; --- 2b. CL-03: Remove all auto-erase scheduled tasks -----------------------
  ; Purge every WinCommander_AutoErase_* task the app may have registered so
  ; they do not outlive the uninstall (the tasks embed self-contained
  ; scripts and would otherwise keep running post-removal). Also purge
  ; legacy System_AutoErase_* tasks for machines that never ran migration.
  nsExec::ExecToLog 'powershell.exe -NonInteractive -NoProfile -WindowStyle Hidden \
    -Command "Get-ScheduledTask -ErrorAction SilentlyContinue | \
    Where-Object { $_.TaskName -like ''WinCommander_AutoErase_*'' -or $_.TaskName -like ''System_AutoErase_*'' } | \
    Unregister-ScheduledTask -Confirm:$false -ErrorAction SilentlyContinue"'

  ; --- 2c. Remove Pro sidecar + bundled kernel driver/service -----------------
  ; The Pro sidecar (wincommander-pro.exe) and its bundled EncVol kernel driver
  ; are downloaded post-install into %ProgramData%\WinCommander\bin (see
  ; pro_install.rs) — outside $INSTDIR, so the base uninstall section never
  ; touches them. Left behind, an examiner finds a live kernel-mode driver
  ; service + signed Pro binary after "uninstall". Stop + delete the service
  ; before removing its driver file; best-effort, ignore errors if absent.
  nsExec::ExecToLog 'sc stop veracrypt'
  nsExec::ExecToLog 'sc delete veracrypt'
  RMDir /r "$PROGRAMDATA\WinCommander\bin"

  ; --- 3. Full shortcut + registry cleanup (handles hidden-mode artefacts) ----
  ; Write the cleanup script to a temp file to avoid inline-escaping nightmares.
  GetTempFileName $R0
  FileOpen $R1 $R0 w

  FileWrite $R1 "# WinCommander pre-uninstall cleanup$\n"
  FileWrite $R1 "# Deletes all shortcuts (including .wc-hidden variants), restores any$\n"
  FileWrite $R1 "# __WC_Hidden Run/App-Paths registry entries, and removes state files.$\n"
  FileWrite $R1 "$\n"

  ; Shortcut paths — Start Menu (machine-wide + per-user) and Desktop (all variants)
  FileWrite $R1 "`$names = @('WinCommander','WinCommander Free','WinCommander Pro')$\n"
  FileWrite $R1 "`$smRoots = @($\n"
  FileWrite $R1 "  'C:\ProgramData\Microsoft\Windows\Start Menu\Programs',$\n"
  FileWrite $R1 "  $\"`$env:APPDATA\Microsoft\Windows\Start Menu\Programs$\"$\n"
  FileWrite $R1 ")$\n"
  FileWrite $R1 "`$desktops = @($\n"
  FileWrite $R1 "  [Environment]::GetFolderPath('Desktop'),$\n"
  FileWrite $R1 "  [Environment]::GetFolderPath('CommonDesktopDirectory'),$\n"
  FileWrite $R1 "  $\"`$env:USERPROFILE\Desktop$\",$\n"
  FileWrite $R1 "  $\"`$env:PUBLIC\Desktop$\"$\n"
  FileWrite $R1 ")$\n"
  FileWrite $R1 "if (-not [string]::IsNullOrEmpty(`$env:OneDrive))      { `$desktops += $\"`$env:OneDrive\Desktop$\" }$\n"
  FileWrite $R1 "if (-not [string]::IsNullOrEmpty(`$env:OneDriveConsumer)) { `$desktops += $\"`$env:OneDriveConsumer\Desktop$\" }$\n"
  FileWrite $R1 "$\n"

  ; Build full list of paths and delete each plus its .wc-hidden variant
  FileWrite $R1 "`$paths = [Collections.Generic.List[string]]::new()$\n"
  FileWrite $R1 "foreach (`$sm in `$smRoots) {$\n"
  FileWrite $R1 "  foreach (`$n in `$names) { [void]`$paths.Add($\"`$sm\`$n.lnk$\") }$\n"
  FileWrite $R1 "  [void]`$paths.Add($\"`$sm\WinCommander$\")$\n"
  FileWrite $R1 "  [void]`$paths.Add($\"`$sm\Uninstall WinCommander.lnk$\")$\n"
  FileWrite $R1 "}$\n"
  FileWrite $R1 "foreach (`$dp in (`$desktops | Where-Object { -not [string]::IsNullOrEmpty(`$_) } | Select-Object -Unique)) {$\n"
  FileWrite $R1 "  foreach (`$n in `$names) { [void]`$paths.Add($\"`$dp\`$n.lnk$\") }$\n"
  FileWrite $R1 "}$\n"
  FileWrite $R1 "foreach (`$p in `$paths) {$\n"
  FileWrite $R1 "  Remove-Item `$p          -Recurse -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "  Remove-Item $\"`$p.wc-hidden$\" -Recurse -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "}$\n"
  FileWrite $R1 "$\n"

  ; Run / RunOnce — remove both the normal WC entry AND any __WC_Hidden renamed copy
  FileWrite $R1 "`$runKeys = @($\n"
  FileWrite $R1 "  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run',$\n"
  FileWrite $R1 "  'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce',$\n"
  FileWrite $R1 "  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run'$\n"
  FileWrite $R1 ")$\n"
  FileWrite $R1 "foreach (`$rk in `$runKeys) {$\n"
  FileWrite $R1 "  if (-not (Test-Path `$rk)) { continue }$\n"
  FileWrite $R1 "  (Get-ItemProperty `$rk -ErrorAction SilentlyContinue).PSObject.Properties |$\n"
  FileWrite $R1 "    Where-Object { `$_.Name -notmatch '^PS' -and$\n"
  FileWrite $R1 "      `$_.Name -match 'WinCommander|wincommander-free|commander-free' } |$\n"
  FileWrite $R1 "    ForEach-Object { Remove-ItemProperty `$rk -Name `$_.Name -ErrorAction SilentlyContinue }$\n"
  FileWrite $R1 "}$\n"
  FileWrite $R1 "$\n"

  ; App Paths — remove normal and __WC_Hidden_ prefixed keys
  FileWrite $R1 "`$apRoots = @($\n"
  FileWrite $R1 "  'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths',$\n"
  FileWrite $R1 "  'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths'$\n"
  FileWrite $R1 ")$\n"
  FileWrite $R1 "foreach (`$root in `$apRoots) {$\n"
  FileWrite $R1 "  if (-not (Test-Path `$root)) { continue }$\n"
  FileWrite $R1 "  Get-ChildItem `$root -ErrorAction SilentlyContinue |$\n"
  FileWrite $R1 "    Where-Object { `$_.PSChildName -match '^(WinCommander|wincommander-free|__WC_Hidden_WinCommander|__WC_Hidden_wincommander)' } |$\n"
  FileWrite $R1 "    ForEach-Object { Remove-Item `$_.PSPath -Recurse -Force -ErrorAction SilentlyContinue }$\n"
  FileWrite $R1 "}$\n"
  FileWrite $R1 "$\n"

  ; Remove SystemComponent=1 from all WinCommander uninstall keys so the uninstaller
  ; can clean them up normally (SystemComponent hides from Apps & Features / uninstall tools)
  FileWrite $R1 "`$uninstallRoots = @($\n"
  FileWrite $R1 "  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',$\n"
  FileWrite $R1 "  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',$\n"
  FileWrite $R1 "  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'$\n"
  FileWrite $R1 ")$\n"
  FileWrite $R1 "foreach (`$root in `$uninstallRoots) {$\n"
  FileWrite $R1 "  if (-not (Test-Path `$root)) { continue }$\n"
  FileWrite $R1 "  Get-ChildItem `$root -ErrorAction SilentlyContinue | ForEach-Object {$\n"
  FileWrite $R1 "    try {$\n"
  FileWrite $R1 "      `$p = Get-ItemProperty `$_.PSPath -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "      if (`$p.DisplayName -match 'WinCommander' -or `$p.Publisher -match 'ServaLabs') {$\n"
  FileWrite $R1 "        Remove-ItemProperty `$_.PSPath -Name 'SystemComponent' -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "      }$\n"
  FileWrite $R1 "    } catch {}$\n"
  FileWrite $R1 "  }$\n"
  FileWrite $R1 "}$\n"
  FileWrite $R1 "$\n"

  ; Delete hide-mode state and flag files
  FileWrite $R1 "Remove-Item $\"`$env:APPDATA\WinCommander\session_state.dat$\" -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "Remove-Item $\"`$env:APPDATA\WinCommander\hide_wincommander.flag$\" -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "Remove-Item $\"`$env:ProgramData\WinCommander\visibility_state.json$\" -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "$\n"

  ; Wipe encrypted app data on uninstall.
  ; Uninstall means the user chose to remove everything — an encrypted-but-
  ; named logs\ dir and a license_cache.json left behind after uninstall are
  ; forensic orphans (audit #14), not "support artefacts". No preservation.
  FileWrite $R1 "`$wcLocal = $\"`$env:LOCALAPPDATA\WinCommander$\"$\n"
  FileWrite $R1 "foreach (`$item in @('store', 'icon-cache', 'migration-v3.done', 'logs')) {$\n"
  FileWrite $R1 "  Remove-Item $\"`$wcLocal\`$item$\" -Recurse -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "}$\n"
  FileWrite $R1 "Remove-Item $\"`$wcLocal\.install.material$\" -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "Remove-Item $\"`$wcLocal\settings.json$\"     -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "Remove-Item $\"`$wcLocal\license_cache.json$\" -Force -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "$\n"

  ; Notify Windows Shell so Search and Start Menu update immediately
  FileWrite $R1 "try {$\n"
  FileWrite $R1 "  Add-Type -TypeDefinition @'$\n"
  FileWrite $R1 "using System; using System.Runtime.InteropServices;$\n"
  FileWrite $R1 "public static class WcShell {$\n"
  FileWrite $R1 "  [DllImport($\"shell32.dll$\")] public static extern void SHChangeNotify(int e, uint f, IntPtr a, IntPtr b);$\n"
  FileWrite $R1 "  public const int SHCNE_ASSOCCHANGED = 0x08000000;$\n"
  FileWrite $R1 "  public const uint SHCNF_FLUSH = 0x1000;$\n"
  FileWrite $R1 "}$\n"
  FileWrite $R1 "'@ -ErrorAction SilentlyContinue$\n"
  FileWrite $R1 "  [WcShell]::SHChangeNotify([WcShell]::SHCNE_ASSOCCHANGED, [WcShell]::SHCNF_FLUSH, [IntPtr]::Zero, [IntPtr]::Zero)$\n"
  FileWrite $R1 "} catch {}$\n"

  FileClose $R1

  nsExec::ExecToLog 'powershell.exe -NonInteractive -NoProfile -WindowStyle Hidden \
    -ExecutionPolicy Bypass -File "$R0"'

  Delete $R0
!macroend
