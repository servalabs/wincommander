; Machine-wide WinCommander service lifecycle.
; The encrypted-volume driver is deliberately NOT installed or removed here.
; Its signed payload arrives with the entitled Pro sidecar and is started by
; WinCommanderSvc only when a Vault operation needs it.  This prevents an
; ordinary app update or uninstall from leaving a kernel driver unloading.

!include "StrFunc.nsh"
${Using:StrFunc} StrStr
${Using:StrFunc} UnStrStr

!define WC_SERVICE_NAME "WinCommanderSvc"
!define WC_SERVICE_PAYLOAD "$INSTDIR\resources\wincommander-svc.exe"
!define WC_SERVICE_EXE "$INSTDIR\wincommander-svc.exe"
!define WC_LIFECYCLE_DIAGNOSTIC_LOG "$INSTDIR\installer-lifecycle.log"
!define WC_LEGACY_LAUNCH_MIGRATION "${__FILEDIR__}\migrate-legacy-user-launches.ps1"
!define WC_CLOSE_INSTALLED_APP "${__FILEDIR__}\close-installed-app.ps1"
!define WC_CONFIGURE_ELEVATED_LAUNCHERS "${__FILEDIR__}\configure-elevated-launchers.ps1"
!define WC_UPGRADE_LICENSE_BACKUP "$R5\WinCommander-license_cache.upgrade-backup.json"
; The service advertises this same bounded cleanup interval to SCM while it
; dismounts an active Vault.  The installer must not replace its EXE sooner.
!define WC_SERVICE_STOP_TIMEOUT_SECONDS 135
; `sc delete` only marks a service for deletion.  Wait for SCM to remove that
; record before the update tries to create a service with the same name.
!define WC_SERVICE_DELETE_TIMEOUT_SECONDS 30

!macro WC_WRITE_LIFECYCLE_DIAGNOSTIC stage exit detail
  ; `sc query` contains SCM's STATE, CHECKPOINT and WAIT_HINT on separate
  ; lines. Retain that exact output for post-failure diagnosis.
  FileOpen $R9 "${WC_LIFECYCLE_DIAGNOSTIC_LOG}" a
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
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-configure-elevated-launchers.ps1" -ExecutablePath "$INSTDIR\wincommander-free.exe"'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "elevated-launchers-configure" "$0" "$1"
  ${If} $0 != 0
    DetailPrint "Warning: WinCommander could not configure automatic elevated launch for Administrators; normal UAC fallback remains available."
  ${EndIf}
!macroend

; The Pro binary, its verified version record, and the licence cache are
; machine-owned artifacts. Every local user needs read/execute access so a
; completed administrator update is visible in every session; only SYSTEM and
; Administrators may replace them. Per-user preferences remain under each
; profile's LocalAppData and are not part of this ACL change.
!macro WC_ENSURE_SHARED_MACHINE_DATA_ACL_OR_ABORT stage
  nsExec::ExecToStack 'icacls.exe "$R5\WinCommander" /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX" /T /C'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-shared-machine-data-acl" "$0" "$1"
  ${If} $0 != 0
    Abort "WinCommander could not secure its shared machine data for all local users."
  ${EndIf}
!macroend

; Older builds could leave an individual ProgramData file (most importantly
; `store\settings.dat` or `.install.material`) with a protected ACL owned by
; SYSTEM.  `icacls /T /C` then continues past that file and reports success for
; the directory, while every desktop account later fails before it can read
; settings.  The elevated installer owns this product root, so first reclaim
; ownership of its machine-owned files and then apply the deliberate shared
; read-only ACL.  Per-user preferences are in LocalAppData and are untouched.
; A clean installation has no product ProgramData folder yet.  `takeown`
; correctly returns ERROR_FILE_NOT_FOUND in that case, so create the narrow
; product root before attempting the legacy-owner repair.  CreateDirectory is
; idempotent and does not modify an existing store.
!macro WC_ENSURE_SHARED_MACHINE_DATA_DIRECTORY_OR_ABORT stage
  ClearErrors
  CreateDirectory "$R5\WinCommander"
  ${If} ${Errors}
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-shared-machine-data-directory" "failed" "Could not create the product ProgramData directory"
    Abort "WinCommander could not prepare its shared machine data directory."
  ${EndIf}
!macroend

!macro WC_REPAIR_SHARED_MACHINE_DATA_ACL_OR_ABORT stage
  !insertmacro WC_ENSURE_SHARED_MACHINE_DATA_DIRECTORY_OR_ABORT "${stage}"
  nsExec::ExecToStack 'takeown.exe /F "$R5\WinCommander" /A /R /D Y'
  Pop $0
  Pop $1
  !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "${stage}-shared-machine-data-owner-repair" "$0" "$1"
  ${If} $0 != 0
    Abort "WinCommander could not repair ownership of its shared machine data."
  ${EndIf}
  !insertmacro WC_ENSURE_SHARED_MACHINE_DATA_ACL_OR_ABORT "${stage}"
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
      Abort "WinCommander could not stop its existing machine service."
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
        Abort "WinCommander service did not stop within ${WC_SERVICE_STOP_TIMEOUT_SECONDS} seconds."
      ${EndIf}
      DetailPrint "Waiting for WinCommander service to stop ($R8/${WC_SERVICE_STOP_TIMEOUT_SECONDS} seconds; SCM state/checkpoint recorded in installer-lifecycle.log)."
      Sleep 1000
      IntOp $R8 $R8 + 1
      Goto wc_wait_for_service_stop_${stage}
    wc_service_stopped_${stage}:
  ${ElseIf} $0 != 1060
    !insertmacro WC_WRITE_SERVICE_QUERY_DIAGNOSTIC "${stage}" "initial"
    Abort "WinCommander could not inspect its existing machine service."
  ${EndIf}
!macroend

; A setup upgrade runs the installed release's uninstaller between this hook
; and POSTINSTALL. Keep a short-lived, machine-owned fallback outside the
; product directory so an older uninstaller cannot remove the active licence
; token before its own preservation path restores it.
!macro NSIS_HOOK_PREINSTALL
  ; Start this install's evidence fresh, then retain it through POSTINSTALL so
  ; an update report includes the desktop close and the old-service shutdown.
  Delete "${WC_LIFECYCLE_DIAGNOSTIC_LOG}"
  ; This runs before an existing version's uninstaller during /UPDATE, so it
  ; makes the old executable writable even when that older uninstall hook did
  ; not know how to close a running desktop application.
  !insertmacro WC_CLOSE_OWNED_DESKTOP_APP_OR_ABORT "preinstall"
  ; Stop with the service's full advertised budget before the older
  ; uninstaller runs. It will then find a stopped service rather than aborting
  ; its own shorter legacy wait.
  !insertmacro WC_STOP_OWNED_SERVICE_OR_ABORT "preinstall" ""
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

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro WC_LOAD_PROGRAMDATA_OR_ABORT "postinstall"
  !insertmacro WC_REPAIR_SHARED_MACHINE_DATA_ACL_OR_ABORT "postinstall"
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

  ; Move every existing local profile away from the obsolete per-user binary.
  ; This is an elevated, machine-wide migration: it changes only shortcuts and
  ; old executable payloads, and never removes profile-owned settings/caches.
  InitPluginsDir
  File /oname=$PLUGINSDIR\wincommander-migrate-legacy-user-launches.ps1 "${WC_LEGACY_LAUNCH_MIGRATION}"
  nsExec::ExecToStack 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\wincommander-migrate-legacy-user-launches.ps1" -SharedExecutable "$INSTDIR\wincommander-free.exe"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "legacy-launch-migration" "$0" "$1"
    DetailPrint "Warning: WinCommander could not migrate every legacy user shortcut."
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
  !insertmacro WC_STOP_OWNED_SERVICE_OR_ABORT "uninstall" "un"
  !insertmacro WC_DELETE_OWNED_SERVICE_OR_ABORT "uninstall"
  ; Updates remove and immediately recreate these trusted launcher tasks in
  ; POSTINSTALL; a standalone uninstall leaves no privileged launch route.
  nsExec::ExecToStack 'schtasks.exe /Delete /TN "WinCommander Elevated Launcher" /F'
  Pop $0
  Pop $1
  nsExec::ExecToStack 'schtasks.exe /Delete /TN "WinCommander Elevated Autostart" /F'
  Pop $0
  Pop $1

  ; Tauri marks an in-place replacement with /UPDATE. An update must retain
  ; every user and machine data file (preferences, encrypted settings, licence,
  ; Pro runtime state, logs, and caches). Only an explicit uninstall may reach
  ; the cleanup below.
  ClearErrors
  ${GetOptions} $CMDLINE "/UPDATE" $R7
  ${IfNot} ${Errors}
    Goto wc_uninstall_cleanup_done
  ${EndIf}

  !insertmacro WC_LOAD_PROGRAMDATA_OR_ABORT "uninstall"

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
    RMDir /r "$R5\WinCommander"
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
    ; A legacy per-user installer used this path.  Remove it for the user
    ; running uninstall so its old executable cannot shadow the shared build.
    RMDir /r "$LOCALAPPDATA\WinCommander"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander"
    Goto wc_uninstall_cleanup_done
  wc_preserve_license_failed:
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-preserve-license" "failed" "$R5\WinCommander\license_cache.json"
    Goto wc_remove_legacy_current_user
  wc_restore_license_failed:
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-restore-license" "failed" "$PLUGINSDIR\wincommander-license_cache.json"
  wc_uninstall_cleanup_done:
!macroend
