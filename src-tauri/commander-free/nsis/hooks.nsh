; Machine-wide WinCommander service lifecycle.
; The encrypted-volume driver is deliberately NOT installed or removed here.
; Its signed payload arrives with the entitled Pro sidecar and is started by
; WinCommanderSvc only when a Vault operation needs it.  This prevents an
; ordinary app update or uninstall from leaving a kernel driver unloading.

!include "StrFunc.nsh"
${Using:StrFunc} StrStr

!define WC_SERVICE_NAME "WinCommanderSvc"
!define WC_SERVICE_PAYLOAD "$INSTDIR\resources\wincommander-svc.exe"
!define WC_SERVICE_EXE "$INSTDIR\wincommander-svc.exe"
!define WC_LIFECYCLE_DIAGNOSTIC_LOG "$INSTDIR\installer-lifecycle.log"
!define WC_LEGACY_LAUNCH_MIGRATION "${__FILEDIR__}\migrate-legacy-user-launches.ps1"

!macro WC_WRITE_LIFECYCLE_DIAGNOSTIC stage exit detail
  FileOpen $R9 "${WC_LIFECYCLE_DIAGNOSTIC_LOG}" a
  FileWrite $R9 "stage=${stage} exit=${exit} detail=${detail}$\r$\n"
  FileClose $R9
!macroend

; `sc stop` returns before SCM has necessarily released the service process.
; Replacing its executable early can fail silently, then leave the next start
; using a stale image. Wait until SCM confirms STOPPED before copying.
!macro WC_STOP_OWNED_SERVICE_OR_ABORT
  nsExec::ExecToStack 'sc.exe query ${WC_SERVICE_NAME}'
  Pop $0
  Pop $1
  ${If} $0 == 0
    nsExec::ExecToStack 'sc.exe stop ${WC_SERVICE_NAME}'
    Pop $0
    Pop $1
    ${If} $0 != 0
    ${AndIf} $0 != 1062
      !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-stop" "$0" "$1"
      Abort "WinCommander could not stop its existing machine service."
    ${EndIf}
    StrCpy $R8 0
    wc_wait_for_service_stop:
      ; Do not shell a pipe through cmd.exe here.  The prior escaped-pipe form
      ; was passed to sc.exe as an option on affected NSIS builds (exit 1639),
      ; making a stopped service look permanently busy and aborting every update.
      nsExec::ExecToStack 'sc.exe query ${WC_SERVICE_NAME}'
      Pop $0
      Pop $1
      ${StrStr} $R7 $1 "STOPPED"
      ${If} $0 == 0
      ${AndIf} $R7 != ""
        Goto wc_service_stopped
      ${EndIf}
      IntOp $R8 $R8 + 1
      ${If} $R8 >= 30
        !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-stop-wait" "$0" "$1"
        Abort "WinCommander service did not stop within 30 seconds."
      ${EndIf}
      Sleep 1000
      Goto wc_wait_for_service_stop
    wc_service_stopped:
  ${ElseIf} $0 != 1060
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-query" "$0" "$1"
    Abort "WinCommander could not inspect its existing machine service."
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Delete "${WC_LIFECYCLE_DIAGNOSTIC_LOG}"
  IfFileExists "${WC_SERVICE_PAYLOAD}" wc_service_payload_ok 0
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "service-payload" "missing" "${WC_SERVICE_PAYLOAD}"
    Abort "The WinCommander service payload is missing; the installation was not completed."
  wc_service_payload_ok:
  ; Stop the existing service before replacing its owned executable. The
  ; separately entitled Pro helper is installed on demand and is never bundled
  ; into, replaced by, or required for a Free installer update.
  !insertmacro WC_STOP_OWNED_SERVICE_OR_ABORT
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
  nsExec::ExecToStack 'sc.exe stop ${WC_SERVICE_NAME}'
  Pop $0
  Pop $1
  nsExec::ExecToStack 'sc.exe delete ${WC_SERVICE_NAME}'
  Pop $0
  Pop $1

  ; A product uninstall removes every machine-owned WinCommander component,
  ; including the separately delivered Pro runtime and device policy/state.
  ; The one exception is the device-bound licence token, which is moved out of
  ; the product root and restored after cleanup.  Encrypted Vault containers
  ; are user-chosen files outside this product root and are never touched.
  ; If the token cannot be moved, leave the root intact rather than risk a
  ; licence loss; the removed executable and service remain uninstalled.
  InitPluginsDir
  IfFileExists "$PROGRAMDATA\WinCommander\license_cache.json" 0 wc_remove_machine_data
    ClearErrors
    Rename "$PROGRAMDATA\WinCommander\license_cache.json" "$PLUGINSDIR\wincommander-license_cache.json"
    IfErrors wc_preserve_license_failed
  wc_remove_machine_data:
    RMDir /r "$PROGRAMDATA\WinCommander"
    CreateDirectory "$PROGRAMDATA\WinCommander"
    ; Restore the device-data ACL before putting the retained entitlement back.
    ; Standard users may read it but cannot replace the shared token or state.
    nsExec::ExecToStack 'icacls.exe "$PROGRAMDATA\WinCommander" /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)F" "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-32-545:(OI)(CI)RX"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-restore-machine-data-acl" "$0" "$1"
    ${EndIf}
    IfFileExists "$PLUGINSDIR\wincommander-license_cache.json" 0 wc_remove_legacy_current_user
      ClearErrors
      Rename "$PLUGINSDIR\wincommander-license_cache.json" "$PROGRAMDATA\WinCommander\license_cache.json"
      IfErrors wc_restore_license_failed
  wc_remove_legacy_current_user:
    ; A legacy per-user installer used this path.  Remove it for the user
    ; running uninstall so its old executable cannot shadow the shared build.
    RMDir /r "$LOCALAPPDATA\WinCommander"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander"
    Goto wc_uninstall_cleanup_done
  wc_preserve_license_failed:
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-preserve-license" "failed" "$PROGRAMDATA\WinCommander\license_cache.json"
    Goto wc_remove_legacy_current_user
  wc_restore_license_failed:
    !insertmacro WC_WRITE_LIFECYCLE_DIAGNOSTIC "uninstall-restore-license" "failed" "$PLUGINSDIR\wincommander-license_cache.json"
  wc_uninstall_cleanup_done:
!macroend
