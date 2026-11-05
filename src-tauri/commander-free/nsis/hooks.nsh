; Machine-wide WinCommander service lifecycle.
; The encrypted-volume driver is deliberately NOT installed or removed here.
; Its signed payload arrives with the entitled Pro sidecar and is started by
; WinCommanderSvc only when a Vault operation needs it.  This prevents an
; ordinary app update or uninstall from leaving a kernel driver unloading.

!ifdef WC_EXPERIMENTAL_SEARCH_FENCE
!ifndef WC_EXPERIMENTAL_USER_STAGE_ONLY
!error "Search maintenance fencing requires the gated current-user staging installer"
!endif
!endif
!include "TextFunc.nsh"
!include "StrFunc.nsh"
${Using:StrFunc} StrStr
${Using:StrFunc} UnStrStr

!define WC_SERVICE_NAME "WinCommanderSvc"
!define WC_SERVICE_PAYLOAD "$INSTDIR\resources\wincommander-svc.exe"
!define WC_SERVICE_EXE "$INSTDIR\wincommander-svc.exe"
!define WC_CALCULATOR_PAYLOAD "$INSTDIR\resources\Calculator.exe"
!define WC_CALCULATOR_EXE "$INSTDIR\Calculator.exe"
!define WC_LIFECYCLE_DIAGNOSTIC_LOG "$INSTDIR\installer-lifecycle.log"
!define WC_LEGACY_LAUNCH_MIGRATION "${__FILEDIR__}\migrate-legacy-user-launches.ps1"
!define WC_SCRUB_MENU_MIGRATION "${__FILEDIR__}\migrate-scrub-context-menus.ps1"
!define WC_CLOSE_INSTALLED_APP "${__FILEDIR__}\close-installed-app.ps1"
!define WC_CONFIGURE_ELEVATED_LAUNCHERS "${__FILEDIR__}\configure-elevated-launchers.ps1"
!define WC_MIGRATE_WIPE_TASKS "${__FILEDIR__}\migrate-wipe-tasks.ps1"
!define WC_MIGRATE_SYSTEM_MAINTENANCE_TASKS "${__FILEDIR__}\migrate-system-maintenance-tasks.ps1"
!define WC_WIPE_TASK_MODULE "${__FILEDIR__}\..\..\wincmd-shared\scripts\auto-erase.ps1"
!ifdef WC_EXPERIMENTAL_USER_STAGE_ONLY
!if "${INSTALLMODE}" != "currentUser"
!error "Original-user staging requires currentUser installer mode"
!endif
!define WC_STAGE_ORIGINAL_USER_SETTINGS "${__FILEDIR__}\stage-original-user-settings.ps1"
!define WC_SETTINGS_STAGE_SERVICE "${__FILEDIR__}\settings-stage-service.ps1"
!ifdef WC_EXPERIMENTAL_SEARCH_FENCE
!define WC_HOLD_SEARCH_MAINTENANCE "${__FILEDIR__}\hold-search-maintenance.ps1"
!define WC_SEARCH_MIGRATION_PHASE "${__FILEDIR__}\search-migration-phase.ps1"
Var WCSearchMoved
Var WCSearchFenceHandle
!endif
Var WCUserStageServiceRunning
!endif
!define WC_REPAIR_SHARED_SETTINGS "${__FILEDIR__}\repair-shared-settings.ps1"
!define WC_REPAIR_VAULT_DRIVER_ACCESS "${__FILEDIR__}\..\..\..\tools\repair-vault-driver-access.ps1"
!define WC_UPGRADE_LICENSE_BACKUP "$R5\WinCommander-license_cache.upgrade-backup.json"
; The service advertises this same bounded cleanup interval to SCM while it
; dismounts an active Vault.  The installer must not replace its EXE sooner.
!define WC_SERVICE_STOP_TIMEOUT_SECONDS 135
; `sc delete` only marks a service for deletion.  Wait for SCM to remove that
; record before the update tries to create a service with the same name.
!define WC_SERVICE_DELETE_TIMEOUT_SECONDS 30

!macro WC_SELECT_NATIVE_POWERSHELL target
  ; NSIS is 32-bit even for x64 payloads. Bare powershell.exe consequently
  ; sees only the redirected registry view, leaving native HKLM Run entries.
  ; Use the native host so the helpers inspect both normal and WOW6432Node
  ; paths without changing process-wide filesystem redirection.
  StrCpy ${target} "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  ${If} ${FileExists} "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
    StrCpy ${target} "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
  ${EndIf}
!macroend

!macro WC_WRITE_LIFECYCLE_DIAGNOSTIC stage exit detail
  ; `sc query` contains SCM's STATE, CHECKPOINT and WAIT_HINT on separate
  ; lines. Retain that exact output for post-failure diagnosis.
  FileOpen $R9 "${WC_LIFECYCLE_DIAGNOSTIC_LOG}" a
  FileSeek $R9 0 END
  FileWrite $R9 "stage=${stage} exit=${exit} detail=${detail}$\r$\n"
  FileClose $R9
!macroend

!macro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC stage attempt
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-query" "$0" "attempt=${attempt} output=$1"
!macroend

; NSIS does not provide a built-in $PROGRAMDATA/$COMMONAPPDATA variable. Read
; the Windows-owned ProgramData path instead of accidentally treating it as an
; empty string and operating at the root of the current drive.
!macro WC_LOAD_PROGRAMDATA_OR_ABORT stage
  ClearErrors
  ReadEnvStr $R5 "ProgramData"
  ${If} ${Errors}
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-programdata" "missing" "ProgramData environment variable was unavailable"
    Abort "WinCommander could not locate the machine ProgramData directory."
  ${EndIf}
  ${If} $R5 == ""
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-programdata" "missing" "ProgramData environment variable was empty"
    Abort "WinCommander could not locate the machine ProgramData directory."
  ${EndIf}
!macroend

; A live desktop app holds wincommander-free.exe open, so Windows correctly
; refuses to replace or delete it. Ask that exact Program Files image to close
; first, then force only that exact image after five seconds. The script uses
; Win32_Process.ExecutablePath matching rather than a broad `/IM` kill.
!macro WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT stage
  InitPluginsDir
  File /oname=$PLUGINSDIR\wincommander-close-installed-app.ps1 "${WC_CLOSE_INSTALLED_APP}"
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-close-installed-app.ps1" -ExecutablePath "$INSTDIR\wincommander-free.exe" -GraceSeconds 5 -ForceWaitSeconds 5'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-desktop-close" "$0" "$1"
  ${If} $0 != 0
    Abort "WinCommander could not close its installed desktop application. Close it manually and try the update again."
  ${EndIf}
!macroend

; UAC must not be bypassed by a self-elevating executable. The installer runs
; elevated and therefore can register the narrow, Administrators-only Task
; Scheduler launchers that let an Administrator open this exact installed app
; at high integrity without a repeat consent dialog. A standard user remains
; on the normal UAC consent/credential path.
!macro WC_CONFIGURE_ELEVATED_LAUNCHERS
  InitPluginsDir
  File /oname=$PLUGINSDIR\wincommander-configure-elevated-launchers.ps1 "${WC_CONFIGURE_ELEVATED_LAUNCHERS}"
  ; A manually opened setup is also an upgrade when older routes exist.
  ; Preserve legacy OFF in both modes; a fresh machine still defaults to ON.
  !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
  nsExec::ExecToStack '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-configure-elevated-launchers.ps1" -ExecutablePath "$INSTDIR\wincommander-free.exe" -PreserveAutostartPreference -DeferAutostartConflict'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "elevated-launchers-configure" "$0" "$1"
  ${If} $0 == 3
    DetailPrint "Automatic startup could not be configured because a task name is already in use. Existing tasks and preferences were kept. Launch WinCommander manually."
  ${ElseIf} $0 != 0
    Abort "WinCommander could not configure its required automatic startup route. See installer-lifecycle.log."
  ${EndIf}
!macroend

; Full uninstall must remove automatic routes before deleting the installed
; payload. The helper verifies action targets before it deletes any task or
; registry value, and reports a conflict rather than removing another app.
!macro WC_REMOVE_AUTOSTART_ROUTES_OR_ABORT stage
  InitPluginsDir
  File /oname=$PLUGINSDIR\wincommander-configure-elevated-launchers.ps1 "${WC_CONFIGURE_ELEVATED_LAUNCHERS}"
  !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
  nsExec::ExecToStack '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-configure-elevated-launchers.ps1" -ExecutablePath "$INSTDIR\wincommander-free.exe" -RemoveAutostartRoutes -RemoveManualLauncher -RemoveAutostartPreference'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-autostart-cleanup" "$0" "$1"
  ${If} $0 != 0
    Abort "WinCommander could not remove all of its automatic startup routes. See installer-lifecycle.log."
  ${EndIf}
!macroend

; Repair only desktop settings paths. Do not recursively take ownership of
; private service state or replace any existing encrypted contents.
!macro WC_REPAIR_SHARED_MACHINE_DATA_ACL_OR_ABORT stage
  InitPluginsDir
  File /oname=$PLUGINSDIR\wincommander-repair-shared-settings.ps1 "${WC_REPAIR_SHARED_SETTINGS}"
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-repair-shared-settings.ps1" -DataRoot "$R5\WinCommander"'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-shared-settings-repair" "$0" "$1"
  ${If} $0 != 0
    Abort "WinCommander could not verify shared settings permissions. See installer-lifecycle.log."
  ${EndIf}
!macroend

!ifdef WC_EXPERIMENTAL_USER_STAGE_ONLY
Function WC_RestoreUserStageService
  !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
  nsExec::ExecToStack /TIMEOUT=150000 '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-settings-stage-service.ps1" -Action Restore'
  Pop $0
  Pop $1
  StrCpy $R6 0
  ${If} $0 == 0
    StrCpy $R6 1
  ${EndIf}
FunctionEnd
!endif

!ifdef WC_EXPERIMENTAL_SEARCH_FENCE
; Restore only a backup that this installer invocation confirmed it moved.
; R6 is 1 on success/no action and 0 if manual recovery is required.
Function WC_RestoreMovedSearch
  StrCpy $R6 1
  ${If} $WCSearchMoved == 1
    ${If} $WCSearchFenceHandle != 0
      System::Call 'kernel32::CloseHandle(p $WCSearchFenceHandle) i .r0'
      StrCpy $WCSearchFenceHandle 0
      ${If} $0 == 0
        StrCpy $R6 0
        Return
      ${EndIf}
    ${EndIf}
    !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
    nsExec::ExecToStack /TIMEOUT=600000 '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-search-migration-phase.ps1" -Phase restore -ExecutablePath "$PLUGINSDIR\wincommander-search-fence.exe" -InstallDirectory "$INSTDIR"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      StrCpy $R6 0
    ${Else}
      StrCpy $WCSearchMoved 0
    ${EndIf}
  ${EndIf}
FunctionEnd
!endif

!macro WC_ABORT_STOP_FAILURE stage message
  !ifdef WC_EXPERIMENTAL_USER_STAGE_ONLY
    !if "${stage}" == "preinstall"
      ${If} $WCUserStageServiceRunning == 1
        Call WC_RestoreUserStageService
        ${If} $R6 != 1
          Abort "The service did not stop safely and could not be restored. Existing app and settings remain in place; restart the service before retrying."
        ${EndIf}
      ${EndIf}
    !endif
  !endif
  Abort "${message}"
!macroend

; `sc stop` returns before SCM has necessarily released the service process.
; Replacing its executable early can fail silently, then leave the next start
; using a stale image. Wait until SCM confirms STOPPED before copying.
!macro WC_STOP_OWNED_SERVICE_OR_ABORT stage scope
  nsExec::ExecToStack 'sc.exe query ${WC_SERVICE_NAME}'
  Pop $0
  Pop $1
  ${If} $0 == 0
    !insertmacro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC "${stage}" "0/${WC_SERVICE_STOP_TIMEOUT_SECONDS}"
    nsExec::ExecToStack 'sc.exe stop ${WC_SERVICE_NAME}'
    Pop $0
    Pop $1
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-service-stop-request" "$0" "$1"
    ${If} $0 != 0
    ${AndIf} $0 != 1062
    ${AndIf} $0 != 1052
      !insertmacro WC_ABORT_STOP_FAILURE "${stage}" "WinCommander could not stop its existing machine service."
    ${EndIf}
    StrCpy $R8 0
    wc_wait_for_service_stop_${stage}:
      ; Do not shell a pipe through cmd.exe here.  The prior escaped-pipe form
      ; was passed to sc.exe as an option on affected NSIS builds (exit 1639),
      ; making a stopped service look permanently busy and aborting every update.
      ; `sc query` also reports SCM's STATE, CHECKPOINT and WAIT_HINT so the
      ; lifecycle log records the exact shutdown state if support is needed.
      nsExec::ExecToStack 'sc.exe query ${WC_SERVICE_NAME}'
      Pop $0
      Pop $1
      ; StrFunc creates a separate un.StrStr function for an uninstaller. Use
      ; the matching helper so the same safe stop path compiles in both hooks.
      !if "${scope}" == "un"
        ${UnStrStr} $R7 $1 "STOPPED"
      !else
        ${StrStr} $R7 $1 "STOPPED"
      !endif
      ${If} $0 == 0
      ${AndIf} $R7 != ""
        !insertmacro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC "${stage}" "$R8/${WC_SERVICE_STOP_TIMEOUT_SECONDS}"
        Goto wc_service_stopped_${stage}
      ${ElseIf} $0 == 1060
        ; Another authorized installer/uninstaller may already have removed
        ; the record. There is no executable-owning service left to wait for.
        !insertmacro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC "${stage}" "$R8/${WC_SERVICE_STOP_TIMEOUT_SECONDS}"
        Goto wc_service_stopped_${stage}
      ${EndIf}
      !insertmacro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC "${stage}" "$R8/${WC_SERVICE_STOP_TIMEOUT_SECONDS}"
      ${If} $R8 >= ${WC_SERVICE_STOP_TIMEOUT_SECONDS}
        !insertmacro WC_ABORT_STOP_FAILURE "${stage}" "WinCommander service did not stop within ${WC_SERVICE_STOP_TIMEOUT_SECONDS} seconds."
      ${EndIf}
      DetailPrint "Waiting for WinCommander service to stop ($R8/${WC_SERVICE_STOP_TIMEOUT_SECONDS} seconds; SCM state/checkpoint recorded in installer-lifecycle.log)."
      Sleep 1000
      IntOp $R8 $R8 + 1
      Goto wc_wait_for_service_stop_${stage}
    wc_service_stopped_${stage}:
  ${ElseIf} $0 != 1060
    !insertmacro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC "${stage}" "initial"
    !insertmacro WC_ABORT_STOP_FAILURE "${stage}" "WinCommander could not inspect its existing machine service."
  ${EndIf}
!macroend

; The owned NSIS template upgrades existing NSIS installs in place. A WiX
; migration can run its old uninstaller before this hook, so keep the licence
; fallback for other update paths without relying on it for Pro preservation.
!macro NSIS_HOOK_PREINSTALL
  ; Start this install's evidence fresh, then retain it through POSTINSTALL so
  ; an update report includes the desktop close and the old-service shutdown.
  Delete "${WC_LIFECYCLE_DIAGNOSTIC_LOG}"
  ; In-place NSIS updates reach this hook before replacing the desktop app,
  ; including releases whose own uninstall hook did not close the app.
  !insertmacro WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT "preinstall"
  ; Stop with the service's full advertised budget before replacing its
  ; executable. Older service binaries may take longer to dismount a Vault.
  !ifdef WC_EXPERIMENTAL_USER_STAGE_ONLY
    ; Query numeric SCM state using ServiceController, independent of locale.
    InitPluginsDir
    File /oname=$PLUGINSDIR\wincommander-settings-stage-service.ps1 "${WC_SETTINGS_STAGE_SERVICE}"
    !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
    nsExec::ExecToStack /TIMEOUT=30000 '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-settings-stage-service.ps1" -Action Status'
    Pop $0
    Pop $1
    StrCpy $WCUserStageServiceRunning 0
    ${If} $0 == 4
      StrCpy $WCUserStageServiceRunning 1
    ${ElseIf} $0 != 1
    ${AndIf} $0 != 0
      Abort "WinCommander could not safely inspect its service before staging."
    ${EndIf}
    nsExec::ExecToStack /TIMEOUT=225000 '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-settings-stage-service.ps1" -Action Stop'
    Pop $0
    Pop $1
    ${If} $0 != 0
      !insertmacro WC_ABORT_STOP_FAILURE "preinstall" "WinCommander could not stop its service for settings staging."
    ${EndIf}
  !else
    !insertmacro WC_STOP_OWNED_SERVICE_OR_ABORT "preinstall" ""
  !endif
  !ifdef WC_EXPERIMENTAL_SEARCH_FENCE
    ; The helper duplicates verified file and directory handles into this
    ; installer. Only this process acquires and owns the exclusive lock.
    InitPluginsDir
    StrCpy $WCSearchMoved 0
    StrCpy $WCSearchFenceHandle 0
    File /oname=$PLUGINSDIR\wincommander-search-fence.exe "${MAINBINARYSRCPATH}"
    File /oname=$PLUGINSDIR\wincommander-hold-search-maintenance.ps1 "${WC_HOLD_SEARCH_MAINTENANCE}"
    File /oname=$PLUGINSDIR\wincommander-search-migration-phase.ps1 "${WC_SEARCH_MIGRATION_PHASE}"
    ; Quarantine holds its own exclusive lock. Release it, then take the
    ; installer lock and authenticate/recheck before replacing any binaries.
    !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
    nsExec::ExecToStack /TIMEOUT=600000 '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-search-migration-phase.ps1" -Phase quarantine -ExecutablePath "$PLUGINSDIR\wincommander-search-fence.exe" -InstallDirectory "$INSTDIR"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      !insertmacro WC_ABORT_STOP_FAILURE "preinstall" "WinCommander could not quarantine its existing search index."
    ${EndIf}
    ${StrStr} $R7 $1 "WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:moved"
    ${If} $R7 != ""
      StrCpy $WCSearchMoved 1
    ${EndIf}
    System::Call 'kernel32::GetCurrentProcessId() i .R6'
    !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
    nsExec::ExecToStack /TIMEOUT=90000 '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-hold-search-maintenance.ps1" -ExecutablePath "$PLUGINSDIR\wincommander-search-fence.exe" -InstallerProcessId $R6'
    Pop $0
    Pop $1
    ${If} $0 == 0
      ; nsExec returns a newline-terminated decimal handle owned by this process.
      ${TrimNewLines} "$1" $R5
      StrCpy $WCSearchFenceHandle $R5
      System::Call '*(i 0, i 0, i 0, i 0, i 0) p.r0'
      ${If} $0 != 0
        System::Call 'kernel32::LockFileEx(p $R5, i 3, i 0, i -1, i -1, p r0) i .r1'
        System::Free $0
        StrCpy $0 $1
      ${EndIf}
    ${Else}
      StrCpy $0 0
    ${EndIf}
    ${If} $0 != 1
      Call WC_RestoreMovedSearch
      ${If} $R6 != 1
        Abort "Search migration failed and the prior index could not be restored. Existing data remains in its backup; recovery is required."
      ${EndIf}
      ${If} $WCUserStageServiceRunning == 1
        Call WC_RestoreUserStageService
        ${If} $R6 != 1
          Abort "Search maintenance failed and the service could not restart. Existing app and settings remain in place."
        ${EndIf}
      ${EndIf}
      Abort "WinCommander could not establish search maintenance. Existing app and settings remain in place."
    ${EndIf}
    !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
    nsExec::ExecToStack /TIMEOUT=600000 '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-search-migration-phase.ps1" -Phase verify -ExecutablePath "$PLUGINSDIR\wincommander-search-fence.exe" -InstallDirectory "$INSTDIR"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      Call WC_RestoreMovedSearch
      ${If} $R6 != 1
        Abort "Search verification failed and the prior index could not be restored. Existing data remains in its backup; recovery is required."
      ${EndIf}
      !insertmacro WC_ABORT_STOP_FAILURE "preinstall" "WinCommander could not verify the protected search index."
    ${EndIf}
  !endif
  !ifdef WC_EXPERIMENTAL_USER_STAGE_ONLY
    ; The helper handles fresh installs and fails closed if an existing
    ; profile has lost its app binary. It never selects the new store.
      InitPluginsDir
      File /oname=$PLUGINSDIR\wincommander-settings-stage.exe "${MAINBINARYSRCPATH}"
      File /oname=$PLUGINSDIR\wincommander-stage-original-user-settings.ps1 "${WC_STAGE_ORIGINAL_USER_SETTINGS}"
      !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
      nsExec::ExecToStack /TIMEOUT=600000 '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-stage-original-user-settings.ps1" -ExecutablePath "$PLUGINSDIR\wincommander-settings-stage.exe" -InstallDirectory "$INSTDIR"'
      Pop $0
      Pop $1
      !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "user-settings-stage" "$0" "$1"
      ${If} $0 != 0
        !ifdef WC_EXPERIMENTAL_SEARCH_FENCE
          Call WC_RestoreMovedSearch
          ${If} $R6 != 1
            Abort "Settings staging failed and the prior search index could not be restored. Existing data remains in its backup; recovery is required."
          ${EndIf}
        !endif
        ${If} $WCUserStageServiceRunning == 1
          Call WC_RestoreUserStageService
          ${If} $R6 != 1
            Abort "User settings staging failed and the WinCommander service could not restart. Existing app and settings remain in place."
          ${EndIf}
        ${EndIf}
        Abort "WinCommander could not stage user settings. Existing app and settings remain in place."
      ${EndIf}
  !endif
  !insertmacro WC_LOAD_PROGRAMDATA_OR_ABORT "preinstall"
  IfFileExists "$R5\WinCommander\license_cache.json" 0 wc_no_upgrade_license_to_backup
    ClearErrors
    CopyFiles /SILENT "$R5\WinCommander\license_cache.json" "${WC_UPGRADE_LICENSE_BACKUP}"
    IfErrors 0 wc_no_upgrade_license_to_backup
      DetailPrint "Warning: WinCommander could not make the upgrade licence backup."
  wc_no_upgrade_license_to_backup:
!macroend

!macro WC_DELETE_OWNED_SERVICE_OR_ABORT stage
  nsExec::ExecToStack 'sc.exe delete ${WC_SERVICE_NAME}'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-service-delete-request" "$0" "$1"
  ${If} $0 != 0
  ${AndIf} $0 != 1060
    Abort "WinCommander could not remove its machine service."
  ${EndIf}
  ${If} $0 == 1060
    Goto wc_service_deleted_${stage}
  ${EndIf}
  StrCpy $R8 0
  wc_wait_for_service_delete_${stage}:
    nsExec::ExecToStack 'sc.exe query ${WC_SERVICE_NAME}'
    Pop $0
    Pop $1
    ${If} $0 == 1060
      !insertmacro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC "${stage}-delete" "$R8/${WC_SERVICE_DELETE_TIMEOUT_SECONDS}"
      Goto wc_service_deleted_${stage}
    ${EndIf}
    !insertmacro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC "${stage}-delete" "$R8/${WC_SERVICE_DELETE_TIMEOUT_SECONDS}"
    ${If} $R8 >= ${WC_SERVICE_DELETE_TIMEOUT_SECONDS}
      Abort "WinCommander service deletion did not finish within ${WC_SERVICE_DELETE_TIMEOUT_SECONDS} seconds."
    ${EndIf}
    Sleep 1000
    IntOp $R8 $R8 + 1
    Goto wc_wait_for_service_delete_${stage}
  wc_service_deleted_${stage}:
!macroend

; A true product uninstall must release the server-side device seat before it
; removes the local signed token. `/UPDATE` deliberately skips this macro so an
; in-place upgrade never consumes/requires another activation. If the network
; release cannot be confirmed, abort while the application and retryable token
; are still present rather than silently stranding a licence seat.
!macro WC_RELEASE_LICENSE_SEAT_OR_ABORT
  IfFileExists "$INSTDIR\wincommander-free.exe" 0 wc_license_release_missing
    DetailPrint "Releasing this device's licence seat..."
    nsExec::ExecToStack '"$INSTDIR\wincommander-free.exe" --release-license-seat'
    Pop $0
    Pop $1
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-license-release" "$0" "attempted"
    ${If} $0 != 0
      Abort "WinCommander could not confirm release of this device's licence seat. Connect to the internet and retry the uninstall, or use Deactivate device from WinCommander first."
    ${EndIf}
  wc_license_release_missing:
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro WC_LOAD_PROGRAMDATA_OR_ABORT "postinstall"
  !insertmacro WC_REPAIR_SHARED_MACHINE_DATA_ACL_OR_ABORT "postinstall"
  IfFileExists "${WC_CALCULATOR_PAYLOAD}" wc_calculator_payload_ok 0
    Abort "The Calculator launcher is missing from this setup."
  wc_calculator_payload_ok:
  System::Call 'kernel32::CopyFileW(w "${WC_CALCULATOR_PAYLOAD}", w "${WC_CALCULATOR_EXE}", i 0) i .R8'
  ${If} $R8 == 0
    Abort "The Calculator launcher could not be installed."
  ${EndIf}
  ; An existing Pro driver may have a protected, empty DACL. Run the pinned
  ; verifier only when its engine directory exists; a file-level existence
  ; check can fail under that DACL. This never downloads or starts a driver.
  IfFileExists "$R5\WinCommander\bin\engine" 0 wc_no_driver_access_repair
    InitPluginsDir
    File /oname=$PLUGINSDIR\wincommander-repair-vault-driver-access.ps1 "${WC_REPAIR_VAULT_DRIVER_ACCESS}"
    nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-repair-vault-driver-access.ps1"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "vault-driver-access-repair" "$0" "$1"
      DetailPrint "Warning: WinCommander could not verify the existing Vault driver access."
    ${EndIf}
  wc_no_driver_access_repair:
  ; Prefer the normal preserved token. Only restore this fallback if an older
  ; uninstaller lost it; never overwrite a freshly activated/repaired token.
  IfFileExists "$R5\WinCommander\license_cache.json" wc_remove_upgrade_license_backup 0
    IfFileExists "${WC_UPGRADE_LICENSE_BACKUP}" 0 wc_remove_upgrade_license_backup
      ClearErrors
      CopyFiles /SILENT "${WC_UPGRADE_LICENSE_BACKUP}" "$R5\WinCommander\license_cache.json"
      IfErrors 0 wc_remove_upgrade_license_backup
        !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "upgrade-license-restore" "failed" "${WC_UPGRADE_LICENSE_BACKUP}"
  wc_remove_upgrade_license_backup:
    Delete "${WC_UPGRADE_LICENSE_BACKUP}"
  IfFileExists "${WC_SERVICE_PAYLOAD}" wc_service_payload_ok 0
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-payload" "missing" "${WC_SERVICE_PAYLOAD}"
    Abort "The WinCommander service payload is missing; the installation was not completed."
  wc_service_payload_ok:
  ; Stop the existing service before replacing its owned executable. The
  ; separately entitled Pro helper is installed on demand and is never bundled
  ; into, replaced by, or required for a Free installer update.
  !insertmacro WC_STOP_OWNED_SERVICE_OR_ABORT "install" ""
  ; Keep the service beside the protected app executable. The service's peer
  ; authorization derives this exact install root, so do not run it from a
  ; mutable user profile or ProgramData download directory.
  System::Call 'kernel32::CopyFileW(w "${WC_SERVICE_PAYLOAD}", w "${WC_SERVICE_EXE}", i 0) i .R8'
  ${If} $R8 == 0
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-copy" "failed" "${WC_SERVICE_PAYLOAD}"
    Abort "WinCommander could not install its machine-service executable."
  ${EndIf}
  nsExec::ExecToStack 'sc.exe create ${WC_SERVICE_NAME} binPath= "\$\"${WC_SERVICE_EXE}\$\"" start= auto obj= LocalSystem'
  Pop $0
  Pop $1
  ${If} $0 != 0
  ${AndIf} $0 != 1073
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-create" "$0" "$1"
    Abort "WinCommander could not register its machine service."
  ${EndIf}
  nsExec::ExecToStack 'sc.exe config ${WC_SERVICE_NAME} binPath= "\$\"${WC_SERVICE_EXE}\$\"" start= auto obj= LocalSystem'
  Pop $0
  Pop $1
  ${If} $0 != 0
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-config" "$0" "$1"
    Abort "WinCommander could not configure its machine service."
  ${EndIf}
  nsExec::ExecToStack 'sc.exe start ${WC_SERVICE_NAME}'
  Pop $0
  Pop $1
  ${If} $0 != 0
  ${AndIf} $0 != 1056
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-start" "$0" "$1"
    Abort "WinCommander could not start its machine service."
  ${EndIf}

  !insertmacro WC_CONFIGURE_ELEVATED_LAUNCHERS

  ; Rename only verified, owned wipe tasks; never run the wipe actions.
  File /oname=$PLUGINSDIR\migrate-wipe-tasks.ps1 "${WC_MIGRATE_WIPE_TASKS}"
  File /oname=$PLUGINSDIR\auto-erase.ps1 "${WC_WIPE_TASK_MODULE}"
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\migrate-wipe-tasks.ps1" -ModulePath "$PLUGINSDIR\auto-erase.ps1"'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "wipe-tasks-migration" "$0" "$1"
  ; Task-name migration is best effort. A locked/running old task must never
  ; abort setup; it remains functional and WinCommander retries later.
  ${If} $0 != 0
    DetailPrint "Scheduled-cleanup task migration deferred; setup will continue."
  ${EndIf}

  File /oname=$PLUGINSDIR\wincommander-migrate-system-maintenance-tasks.ps1 "${WC_MIGRATE_SYSTEM_MAINTENANCE_TASKS}"
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-migrate-system-maintenance-tasks.ps1"'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "system-maintenance-task-migration" "$0" "$1"
  ${If} $0 != 0
    DetailPrint "System Maintenance task migration deferred; setup will continue."
  ${EndIf}

  ; Move every existing local profile away from the obsolete per-user binary.
  ; This is an elevated, machine-wide migration: it changes only shortcuts and
  ; old executable payloads, and never removes profile-owned settings/caches.
  InitPluginsDir
  File /oname=$PLUGINSDIR\wincommander-migrate-legacy-user-launches.ps1 "${WC_LEGACY_LAUNCH_MIGRATION}"
  !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
  nsExec::ExecToStack '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-migrate-legacy-user-launches.ps1" -SharedExecutable "$INSTDIR\wincommander-free.exe"'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "legacy-launch-migration" "$0" "$1"
  ${If} $0 != 0
    Abort "WinCommander could not remove legacy automatic startup routes. See installer-lifecycle.log."
  ${EndIf}

  File /oname=$PLUGINSDIR\wincommander-migrate-scrub-context-menus.ps1 "${WC_SCRUB_MENU_MIGRATION}"
  !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
  nsExec::ExecToStack '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-migrate-scrub-context-menus.ps1" -SharedExecutable "$INSTDIR\wincommander-free.exe"'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "scrub-context-menu-migration" "$0" "$1"
  ${If} $0 != 0
    DetailPrint "Some Scrub menus were preserved or deferred. Start the installed WinCommander in the affected account to refresh its menu."
  ${EndIf}

  ; Vault-policy changes are authorized by a dedicated local group, not by
  ; whether the desktop process happened to be elevated. Give the installing
  ; account a direct membership: a nested Administrators group is marked
  ; deny-only in a non-elevated UAC token and would send the app back to an
  ; unnecessary credential prompt. Other users remain unable to alter policy
  ; until a device administrator explicitly grants them this group.
  nsExec::ExecToStack 'net.exe localgroup "WinCommander Vault Policy Administrators" /add'
  Pop $0
  Pop $1
  ${If} $0 != 0
  ${AndIf} $0 != 1379
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "vault-policy-group-create" "$0" "$1"
    ; Vault policy delegation is optional. A domain/local policy may forbid
    ; local-group creation, but that must never cancel a normal app update.
    DetailPrint "Warning: Vault policy administrator group was not created."
  ${EndIf}
  ; `$USERNAME` is not an NSIS variable. Let cmd.exe expand its own environment
  ; variable, otherwise `net localgroup` receives the literal text "$USERNAME"
  ; and silently-installed releases abort on a non-existent account.
  nsExec::ExecToStack 'cmd.exe /c net.exe localgroup "WinCommander Vault Policy Administrators" "%USERNAME%" /add'
  Pop $0
  Pop $1
  ${If} $0 != 0
  ${AndIf} $0 != 1378
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "vault-policy-group-grant" "$0" "$1"
    DetailPrint "Warning: Vault policy administration was not granted to this account."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Stop and remove only the user-mode WinCommander service. Do not issue an
  ; SCM stop/delete for WinCommanderEncVol or VeraCrypt: a loaded kernel driver
  ; is released by Windows at reboot and must never block app removal.
  ; This applies to /UPDATE as well as a full uninstall. In particular, an
  ; update must see SCM confirm STOPPED before it asks SCM to delete the old
  ; record, or the old service can keep the payload open during replacement.
  ; Standalone removal uses the same exact-path graceful-then-forced close.
  ; The update path has already done this in PREINSTALL, but repeating it is
  ; harmless and prevents an interactive uninstall from leaving the EXE locked.
  !insertmacro WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT "uninstall"
  Delete "${WC_CALCULATOR_EXE}"
  !insertmacro WC_STOP_OWNED_SERVICE_OR_ABORT "uninstall" "un"
  !insertmacro WC_DELETE_OWNED_SERVICE_OR_ABORT "uninstall"
  ; Updates retain the canonical task so POSTINSTALL can migrate the legacy
  ; disabled-task preference. A full uninstall removes verified owned routes.
  ClearErrors
  ${GetOptions} $CMDLINE "/UPDATE" $R7
  ${If} ${Errors}
    !insertmacro WC_LOAD_PROGRAMDATA_OR_ABORT "uninstall"
    !insertmacro WC_REMOVE_AUTOSTART_ROUTES_OR_ABORT "uninstall"
    InitPluginsDir
    File /oname=$PLUGINSDIR\wincommander-migrate-legacy-user-launches.ps1 "${WC_LEGACY_LAUNCH_MIGRATION}"
    !insertmacro WC_SELECT_NATIVE_POWERSHELL $R8
    nsExec::ExecToStack '"$R8" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-migrate-legacy-user-launches.ps1" -SharedExecutable "$INSTDIR\wincommander-free.exe" -Uninstall'
    Pop $0
    Pop $1
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-profile-autostart-cleanup" "$0" "$1"
    ${If} $0 != 0
      Abort "WinCommander could not remove per-user automatic startup routes. See installer-lifecycle.log."
    ${EndIf}
  ${EndIf}

  ; Tauri marks an in-place replacement with /UPDATE. An update must retain
  ; every user and machine data file (preferences, encrypted settings, licence,
  ; Pro runtime state, logs, and caches). Only an explicit uninstall may reach
  ; the cleanup below.
  ClearErrors
  ${GetOptions} $CMDLINE "/UPDATE" $R7
  ${IfNot} ${Errors}
    Goto wc_uninstall_cleanup_done
  ${EndIf}

  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "WinCommander Hide Recovery"
  StrCmp $0 '$"$INSTDIR\wincommander-free.exe$" --hide-recovery-listener' 0 +2
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "WinCommander Hide Recovery"

  !insertmacro WC_RELEASE_LICENSE_SEAT_OR_ABORT

  ; An explicit product uninstall removes machine-owned WinCommander components,
  ; including the separately delivered Pro runtime and device policy/state.
  ; The one exception is the device-bound licence token, which is moved out of
  ; the product root and restored after cleanup.  Encrypted Vault containers
  ; are user-chosen files outside this product root and are never touched.
  ; If the token cannot be moved, leave the root intact rather than risk a
  ; licence loss; the removed executable and service remain uninstalled.
  InitPluginsDir
  IfFileExists "$R5\WinCommander\license_cache.json" 0 wc_remove_machine_data
    ClearErrors
    Rename "$R5\WinCommander\license_cache.json" "$PLUGINSDIR\wincommander-license_cache.json"
    IfErrors wc_preserve_license_failed
  wc_remove_machine_data:
    ; Keep the cleanup journal if service shutdown could not close a driver slot.
    ; Permission policies are removed below; the journal grants no new mount access.
    FindFirst $R0 $R1 "$R5\WinCommander\*"
    wc_remove_machine_data_next:
      StrCmp $R1 "" wc_remove_machine_data_end
      StrCmp $R1 "." wc_remove_machine_data_skip
      StrCmp $R1 ".." wc_remove_machine_data_skip
      StrCmp $R1 "policy" wc_remove_machine_data_skip
      RMDir /r "$R5\WinCommander\$R1"
      Delete "$R5\WinCommander\$R1"
    wc_remove_machine_data_skip:
      FindNext $R0 $R1
      Goto wc_remove_machine_data_next
    wc_remove_machine_data_end:
      FindClose $R0
    FindFirst $R0 $R1 "$R5\WinCommander\policy\*"
    wc_remove_policy_data_next:
      StrCmp $R1 "" wc_remove_policy_data_end
      StrCmp $R1 "." wc_remove_policy_data_skip
      StrCmp $R1 ".." wc_remove_policy_data_skip
      StrCmp $R1 "vault-active-mounts-v1.json" wc_remove_policy_data_skip
      RMDir /r "$R5\WinCommander\policy\$R1"
      Delete "$R5\WinCommander\policy\$R1"
    wc_remove_policy_data_skip:
      FindNext $R0 $R1
      Goto wc_remove_policy_data_next
    wc_remove_policy_data_end:
      FindClose $R0
    RMDir "$R5\WinCommander\policy"
    CreateDirectory "$R5\WinCommander"
    ; Restore the device-data ACL before putting the retained entitlement back.
    ; Standard users may read it but cannot replace the shared token or state.
    nsExec::ExecToStack 'icacls.exe "$R5\WinCommander" /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-restore-machine-data-acl" "$0" "$1"
    ${EndIf}
    IfFileExists "$PLUGINSDIR\wincommander-license_cache.json" 0 wc_remove_legacy_current_user
      ClearErrors
      Rename "$PLUGINSDIR\wincommander-license_cache.json" "$R5\WinCommander\license_cache.json"
      IfErrors wc_restore_license_failed
  wc_remove_legacy_current_user:
    ; Per-user state remains intact. The verified migration above removes only
    ; obsolete wincommander-free.exe/uninstall.exe/resources/scripts payloads
    ; from every profile, never the complete LocalAppData product directory.
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander"
    Goto wc_uninstall_cleanup_done
  wc_preserve_license_failed:
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-preserve-license" "failed" "$R5\WinCommander\license_cache.json"
    ; Preserve the entitlement rather than deleting its parent directory, but
    ; never retain Vault authority after an explicit uninstall. These are the
    ; complete machine-owned Vault journals; Vault containers are user-chosen
    ; files outside ProgramData and are deliberately not touched.
    ClearErrors
    Delete "$R5\WinCommander\policy\vault-access-v1.json"
    IfErrors wc_remove_vault_policy_failed
    ; Retain the active cleanup journal even if licence preservation failed.
    Delete "$R5\WinCommander\policy\vault-personal-v1.json"
    IfErrors wc_remove_vault_policy_failed
    Goto wc_remove_legacy_current_user
  wc_remove_vault_policy_failed:
      !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-remove-vault-policy" "failed" "$R5\WinCommander\policy"
    Goto wc_remove_legacy_current_user
  wc_restore_license_failed:
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-restore-license" "failed" "$PLUGINSDIR\wincommander-license_cache.json"
  wc_uninstall_cleanup_done:
!macroend
