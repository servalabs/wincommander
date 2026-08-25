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
!define WC_SERVICE_BACKUP "${WC_INSTALL_DIR}\wincommander-svc.exe.wc-backup"
!define WC_SERVICE_CONFIG_BACKUP "$PLUGINSDIR\WinCommanderSvc-before.reg"
!define WC_LIFECYCLE_DIAGNOSTIC_LOG "${WC_INSTALL_DIR}\installer-lifecycle.log"
; The encryption engine is installed separately below ProgramData.  Its
; driver service is nevertheless owned by this installer, so repair only this
; fixed name and only when the fixed driver payload is actually present.
!define WC_ENCVOL_DRIVER "$PROGRAMDATA\WinCommander\bin\engine\EncVolKm.sys"
!define WC_ENCVOL_CONFIG_BACKUP "$PLUGINSDIR\WinCommanderEncVol-before.reg"

; An MSI build briefly shipped before the per-machine NSIS installer.  Its
; registration can survive an NSIS upgrade even though both installers use the
; same Program Files directory, which makes Apps & Features show two products.
; Never execute the MSI uninstaller here: it owns the shared directory and can
; remove files from the upgrade currently in progress.  Instead, retire only a
; positively identified stale display registration.  Program files and user
; data are left for this installer and the normal update path to preserve.
!macro WC_RETIRE_LEGACY_MSI_REGISTRATIONS
  Call WcRetireLegacyMsiRegistrations
!macroend

; Tauri's main process and the downloaded Pro sidecar both keep their binaries
; open while running.  The service is stopped separately before this macro, so
; its broker cannot immediately start another sidecar during an update.  Exit
; code 128 means the process did not exist; every other failure is actionable.
!macro WC_CLOSE_RUNNING_APPS
  nsExec::ExecToStack 'taskkill.exe /F /T /IM ${MAINBINARYNAME}.exe'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
  ${AndIf} $R8 != 128
    Abort "WinCommander could not be closed. Close it and run the installer again."
  ${EndIf}

  nsExec::ExecToStack 'taskkill.exe /F /T /IM wincommander-pro.exe'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
  ${AndIf} $R8 != 128
    Abort "WinCommander Pro could not be closed. Close it and run the installer again."
  ${EndIf}
!macroend

Function WcRetireLegacyMsiRegistrations
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5

  SetRegView 64
  Call WcRetireLegacyMsiRegistrationsInCurrentView
  SetRegView 32
  Call WcRetireLegacyMsiRegistrationsInCurrentView
  SetRegView lastused

  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

Function WcRetireLegacyMsiRegistrationsInCurrentView
  StrCpy $0 0

  wc_legacy_msi_next:
    EnumRegKey $1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" wc_legacy_msi_done
    IntOp $0 $0 + 1

    ; The current NSIS record is named WinCommander and has no WindowsInstaller
    ; flag.  A GUID alone is insufficient: remove only the old MSI record that
    ; identifies the same product and shared install root.
    ClearErrors
    ReadRegDWORD $2 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "WindowsInstaller"
    ${If} ${Errors}
      Goto wc_legacy_msi_next
    ${EndIf}
    StrCmp $2 1 0 wc_legacy_msi_next
    ClearErrors
    ReadRegStr $3 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    ${If} ${Errors}
      Goto wc_legacy_msi_next
    ${EndIf}
    StrCmp $3 "WinCommander" wc_legacy_msi_name_ok
    StrCmp $3 "WinCommander Pro" wc_legacy_msi_name_ok wc_legacy_msi_next

  wc_legacy_msi_name_ok:
    ClearErrors
    ReadRegStr $4 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "Publisher"
    ${If} ${Errors}
      Goto wc_legacy_msi_next
    ${EndIf}
    StrCmp $4 "Secure Health" wc_legacy_msi_publisher_ok
    StrCmp $4 "ServaLabs" wc_legacy_msi_publisher_ok wc_legacy_msi_next

  wc_legacy_msi_publisher_ok:
    ClearErrors
    ReadRegStr $5 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "InstallLocation"
    ${If} ${Errors}
      Goto wc_legacy_msi_next
    ${EndIf}
    StrCmp $5 "${WC_INSTALL_DIR}" wc_legacy_msi_remove
    StrCmp $5 "${WC_INSTALL_DIR}\" wc_legacy_msi_remove
    StrCmp $5 "$\"${WC_INSTALL_DIR}$\"" wc_legacy_msi_remove wc_legacy_msi_next
    StrCmp $5 "$\"${WC_INSTALL_DIR}\$\"" wc_legacy_msi_remove wc_legacy_msi_next

  wc_legacy_msi_remove:
    DeleteRegKey HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1"
    Goto wc_legacy_msi_next

  wc_legacy_msi_done:
FunctionEnd

!macro WC_WRITE_LIFECYCLE_DIAGNOSTIC
  FileOpen $R0 "${WC_LIFECYCLE_DIAGNOSTIC_LOG}" a
  FileWrite $R0 "stage=$R3 exit=$R8 reason=$R4$\r$\n"
  FileClose $R0
!macroend

!macro NSIS_HOOK_PREINSTALL
  Delete "${WC_LIFECYCLE_DIAGNOSTIC_LOG}"
  ; Releases use one non-customizable machine location.  The service's SCM
  ; ImagePath is therefore stable across fresh install, repair, and update.
  StrCpy $INSTDIR "${WC_INSTALL_DIR}"
  SetOutPath "$INSTDIR"

  ; Preserve the release-managed service configuration before an update.  The
  ; post-install hook imports this exact registry backup if replacement fails,
  ; so a failed update does not strand the previous service configuration.
  InitPluginsDir
  StrCpy $R6 "0"
  Delete "${WC_SERVICE_CONFIG_BACKUP}"
  nsExec::ExecToStack 'sc query WinCommanderSvc'
  Pop $R8
  Pop $R9
  ${If} $R8 == 0
    StrCpy $R6 "1"
    nsExec::ExecToStack 'reg.exe export "HKLM\SYSTEM\CurrentControlSet\Services\WinCommanderSvc" "${WC_SERVICE_CONFIG_BACKUP}" /y'
    Pop $R8
    Pop $R9
    ${If} $R8 != 0
      Abort "WinCommander service configuration could not be backed up."
    ${EndIf}
  ${EndIf}

  ; Stop only when the service already exists. A PowerShell stop used NSIS `$`
  ; variables, so a fresh install aborted before any files were written.
  ${If} $R6 == 1
    nsExec::ExecToStack 'sc stop WinCommanderSvc'
    Pop $R8
    Pop $R9
    ${If} $R8 != 0
    ${AndIf} $R8 != 1062
      Abort "WinCommander service could not stop. Close dependent sessions and run the installer again."
    ${EndIf}
    StrCpy $R5 "0"
    wc_wait_svc_stop:
      nsExec::ExecToStack 'cmd /c sc query WinCommanderSvc | findstr /C:": 1  STOPPED"'
      Pop $R8
      Pop $R9
      ${If} $R8 == 0
        Goto wc_svc_stopped
      ${EndIf}
      IntOp $R5 $R5 + 1
      ${If} $R5 >= 30
        Abort "WinCommander service could not stop. Close dependent sessions and run the installer again."
      ${EndIf}
      Sleep 1000
      Goto wc_wait_svc_stop
    wc_svc_stopped:
  ${EndIf}

  !insertmacro WC_CLOSE_RUNNING_APPS
  !insertmacro WC_RETIRE_LEGACY_MSI_REGISTRATIONS

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
  ; The entry can outlive the binary after a broken prior uninstall, so retire
  ; its exact former key independently of the file check.
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander"
  ${If} ${FileExists} "$LOCALAPPDATA\WinCommander\${MAINBINARYNAME}.exe"
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
  IfFileExists "${WC_BUNDLED_SERVICE}" wc_service_payload_ready 0
    StrCpy $R3 "service-payload"
    StrCpy $R4 "WinCommander service payload is missing; the installation was not completed."
    Goto wc_service_rollback

  wc_service_payload_ready:

  ; Keep a rollback copy of the previous service executable until the new
  ; configuration has started.  Copy rather than rename keeps the working
  ; image in place until the replacement write succeeds.
  StrCpy $R7 "0"
  Delete "${WC_SERVICE_BACKUP}"
  ${If} $R6 == 1
    StrCpy $R3 "service-backup"
    IfFileExists "${WC_SERVICE_EXE}" 0 wc_service_missing_previous_exe
    System::Call 'kernel32::CopyFileW(w "${WC_SERVICE_EXE}", w "${WC_SERVICE_BACKUP}", i 0) i .R8'
    ${If} $R8 == 0
      StrCpy $R4 "WinCommander service executable could not be backed up."
      Goto wc_service_rollback
    ${EndIf}
    StrCpy $R7 "1"
  ${EndIf}
  Goto wc_service_copy_new_exe

  wc_service_missing_previous_exe:
    Abort "WinCommander service configuration exists but its executable is missing."

  wc_service_copy_new_exe:
  StrCpy $R3 "service-copy"
  System::Call 'kernel32::CopyFileW(w "${WC_BUNDLED_SERVICE}", w "${WC_SERVICE_EXE}", i 0) i .R8'
  ${If} $R8 == 0
    StrCpy $R4 "WinCommander service executable could not be installed."
    Goto wc_service_rollback
  ${EndIf}

  ; No certificate is required: LocalSystem is the SCM identity and the
  ; service independently authenticates every named-pipe peer/broker request.
  ; ERROR_SERVICE_EXISTS (1073) is expected during an update; every other SCM
  ; failure is shown to the installer user rather than silently ignored.
  ; `sc.exe` must receive one ImagePath argument containing the executable's
  ; inner quotes, so Program Files remains a single executable path in SCM.
  StrCpy $R3 "service-create"
  nsExec::ExecToStack 'sc.exe create WinCommanderSvc binPath= "\$\"${WC_SERVICE_EXE}\$\"" start= auto obj= LocalSystem'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
  ${AndIf} $R8 != 1073
    StrCpy $R4 "WinCommander service could not be created."
    Goto wc_service_rollback
  ${EndIf}
  ${If} $R8 == 1073
  ${AndIf} $R6 == 0
    Abort "WinCommander service appeared during installation; run the installer again."
  ${EndIf}
  StrCpy $R3 "service-config"
  nsExec::ExecToStack 'sc.exe config WinCommanderSvc binPath= "\$\"${WC_SERVICE_EXE}\$\"" start= auto obj= LocalSystem'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
    StrCpy $R4 "WinCommander service could not be configured."
    Goto wc_service_rollback
  ${EndIf}
  StrCpy $R3 "service-recovery-config"
  nsExec::ExecToStack 'sc failure WinCommanderSvc reset= 86400 actions= restart/5000/restart/5000/none/0'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
    StrCpy $R4 "WinCommander service recovery could not be configured."
    Goto wc_service_rollback
  ${EndIf}
  StrCpy $R3 "service-start"
  nsExec::ExecToStack 'sc start WinCommanderSvc'
  Pop $R8
  Pop $R9
  ${If} $R8 != 0
  ${AndIf} $R8 != 1056
    StrCpy $R4 "WinCommander service could not be started."
    Goto wc_service_rollback
  ${EndIf}
  Delete "${WC_SERVICE_BACKUP}"
  Delete "${WC_SERVICE_CONFIG_BACKUP}"
  Goto wc_service_ready

  ; A service update is all-or-restore: replace the previous executable and
  ; import the exact service registry configuration captured before stopping
  ; it.  A fresh-install failure removes the newly-created service instead.
  wc_service_rollback:
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC
    ${If} $R6 == 1
      ${If} $R7 == 1
        System::Call 'kernel32::CopyFileW(w "${WC_SERVICE_BACKUP}", w "${WC_SERVICE_EXE}", i 0) i .R8'
        ${If} $R8 == 0
          Abort "$R4 The previous WinCommander service executable could not be restored."
        ${EndIf}
      ${EndIf}
      nsExec::ExecToStack 'reg.exe import "${WC_SERVICE_CONFIG_BACKUP}"'
      Pop $R8
      Pop $R9
      ${If} $R8 != 0
        Abort "$R4 The previous WinCommander service configuration could not be restored."
      ${EndIf}
      nsExec::ExecToStack 'sc start WinCommanderSvc'
      Pop $R8
      Pop $R9
      ${If} $R8 != 0
      ${AndIf} $R8 != 1056
        Abort "$R4 The previous WinCommander service could not be restarted."
      ${EndIf}
      Delete "${WC_SERVICE_BACKUP}"
      Delete "${WC_SERVICE_CONFIG_BACKUP}"
      Abort "$R4 The previous WinCommander service was restored."
    ${Else}
      nsExec::ExecToStack 'sc delete WinCommanderSvc'
      Pop $R8
      Pop $R9
      ${If} $R8 != 0
      ${AndIf} $R8 != 1060
        Abort "$R4 The incomplete WinCommander service could not be removed."
      ${EndIf}
      Delete "${WC_SERVICE_EXE}"
      Abort "$R4 The incomplete WinCommander service was removed."
    ${EndIf}

  wc_service_ready:
  ; A prior uninstall removes WinCommanderEncVol but deliberately leaves the
  ; engine installer responsible for the driver file itself.  A subsequent
  ; Free install/update must recreate the *owned* system-start kernel service
  ; when that exact payload is present.  Do not probe, alter, or remove any
  ; third-party driver (including VeraCrypt).
  IfFileExists "${WC_ENCVOL_DRIVER}" wc_encvol_driver_present wc_encvol_driver_ready

  wc_encvol_driver_present:
    StrCpy $R3 "encvol-inspect"
    InitPluginsDir
    Delete "${WC_ENCVOL_CONFIG_BACKUP}"
    StrCpy $R2 "0"
    nsExec::ExecToStack 'sc query WinCommanderEncVol'
    Pop $R8
    Pop $R9
    ${If} $R8 == 0
      StrCpy $R2 "1"
      ; A colliding service name is not proof of ownership.  Refuse to
      ; reconfigure it unless SCM already points at our fixed driver payload.
      nsExec::ExecToStack 'cmd /c sc qc WinCommanderEncVol | findstr /I /C:"${WC_ENCVOL_DRIVER}"'
      Pop $R8
      Pop $R9
      ${If} $R8 != 0
        Abort "WinCommander encryption driver service does not reference the owned driver payload."
      ${EndIf}
      nsExec::ExecToStack 'reg.exe export "HKLM\SYSTEM\CurrentControlSet\Services\WinCommanderEncVol" "${WC_ENCVOL_CONFIG_BACKUP}" /y'
      Pop $R8
      Pop $R9
      ${If} $R8 != 0
        Abort "WinCommander encryption driver configuration could not be backed up."
      ${EndIf}
    ${ElseIf} $R8 != 1060
      Abort "WinCommander encryption driver could not be inspected."
    ${EndIf}

  ; Pass the kernel image as one quoted SCM argument so its fixed path cannot
  ; be interpreted as multiple tokens.
    StrCpy $R3 "encvol-create"
    nsExec::ExecToStack 'sc.exe create WinCommanderEncVol type= kernel binPath= "\$\"${WC_ENCVOL_DRIVER}\$\"" start= system'
    Pop $R8
    Pop $R9
    ${If} $R8 != 0
    ${AndIf} $R8 != 1073
      StrCpy $R4 "WinCommander encryption driver could not be created."
      Goto wc_encvol_driver_rollback
    ${EndIf}
    StrCpy $R3 "encvol-config"
    nsExec::ExecToStack 'sc.exe config WinCommanderEncVol type= kernel binPath= "\$\"${WC_ENCVOL_DRIVER}\$\"" start= system'
    Pop $R8
    Pop $R9
    ${If} $R8 != 0
      StrCpy $R4 "WinCommander encryption driver could not be configured."
      Goto wc_encvol_driver_rollback
    ${EndIf}
    StrCpy $R3 "encvol-start"
    nsExec::ExecToStack 'sc start WinCommanderEncVol'
    Pop $R8
    Pop $R9
    ${If} $R8 != 0
    ${AndIf} $R8 != 1056
      StrCpy $R4 "WinCommander encryption driver could not be started."
      Goto wc_encvol_driver_rollback
    ${EndIf}
    Delete "${WC_ENCVOL_CONFIG_BACKUP}"
    Goto wc_encvol_driver_ready

  ; Restore a pre-existing owned-service configuration on repair failure.  A
  ; newly-created service is deleted instead, leaving no partial driver entry.
  wc_encvol_driver_rollback:
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC
    ${If} $R2 == 1
      nsExec::ExecToStack 'reg.exe import "${WC_ENCVOL_CONFIG_BACKUP}"'
      Pop $R8
      Pop $R9
      ${If} $R8 != 0
        Abort "$R4 The previous encryption driver configuration could not be restored."
      ${EndIf}
      Delete "${WC_ENCVOL_CONFIG_BACKUP}"
      Abort "$R4 The previous encryption driver configuration was restored."
    ${Else}
      nsExec::ExecToStack 'sc delete WinCommanderEncVol'
      Pop $R8
      Pop $R9
      ${If} $R8 != 0
      ${AndIf} $R8 != 1060
        Abort "$R4 The incomplete encryption driver service could not be removed."
      ${EndIf}
      Abort "$R4 The incomplete encryption driver service was removed."
    ${EndIf}

  wc_encvol_driver_ready:
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
  ; Stop first and wait for STATE 1 STOPPED before deletion, otherwise SCM can
  ; retain a live process or a marked-for-delete record after uninstall.
  nsExec::ExecToStack 'sc query WinCommanderSvc'
  Pop $R8
  Pop $R9
  ${If} $R8 == 0
    nsExec::ExecToStack 'sc stop WinCommanderSvc'
    Pop $R8
    Pop $R9
    ${If} $R8 != 0
    ${AndIf} $R8 != 1062
      Abort "WinCommander service could not stop. Close dependent sessions and run the uninstaller again."
    ${EndIf}
    StrCpy $R5 "0"
    wc_un_wait_svc_stop:
      nsExec::ExecToStack 'cmd /c sc query WinCommanderSvc | findstr /C:": 1  STOPPED"'
      Pop $R8
      Pop $R9
      ${If} $R8 == 0
        Goto wc_un_svc_stopped
      ${EndIf}
      IntOp $R5 $R5 + 1
      ${If} $R5 >= 30
        Abort "WinCommander service could not stop. Close dependent sessions and run the uninstaller again."
      ${EndIf}
      Sleep 1000
      Goto wc_un_wait_svc_stop
    wc_un_svc_stopped:
  ${EndIf}
  !insertmacro WC_CLOSE_RUNNING_APPS
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
  ; touches them.  Only touch WinCommander's own driver service: `veracrypt`
  ; may belong to an official VeraCrypt installation.  Windows can retain a
  ; kernel-driver service as STOP_PENDING after a stop request; deleting its
  ; image then strands the driver record.  Wait for a confirmed stop before
  ; deleting either the service or its containing directory.
  nsExec::ExecToStack 'sc query WinCommanderEncVol'
  Pop $R8
  Pop $R9
  ${If} $R8 == 0
    nsExec::ExecToStack 'sc stop WinCommanderEncVol'
    Pop $R8
    Pop $R9
    ${If} $R8 != 0
    ${AndIf} $R8 != 1062
      Abort "WinCommander encryption driver could not stop. Restart Windows, then run the uninstaller again."
    ${EndIf}
    StrCpy $R5 "0"
    wc_un_wait_encvol_stop:
      nsExec::ExecToStack 'cmd /c sc query WinCommanderEncVol | findstr /C:": 1  STOPPED"'
      Pop $R8
      Pop $R9
      ${If} $R8 == 0
        Goto wc_un_encvol_stopped
      ${EndIf}
      IntOp $R5 $R5 + 1
      ${If} $R5 >= 30
        Abort "WinCommander encryption driver is still unloading. Restart Windows, then run the uninstaller again."
      ${EndIf}
      Sleep 1000
      Goto wc_un_wait_encvol_stop
    wc_un_encvol_stopped:
      nsExec::ExecToStack 'sc delete WinCommanderEncVol'
      Pop $R8
      Pop $R9
      ${If} $R8 != 0
      ${AndIf} $R8 != 1060
        Abort "WinCommander encryption driver could not be removed. Restart Windows, then run the uninstaller again."
      ${EndIf}
  ${EndIf}
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
