; Machine-wide WinCommander service lifecycle.
; The encrypted-volume driver is deliberately NOT installed or removed here.
; Its signed payload arrives with the entitled Pro sidecar and is started by
; WinCommanderSvc only when a Vault operation needs it.  This prevents an
; ordinary app update or uninstall from leaving a kernel driver unloading.

!define WC_SERVICE_NAME "WinCommanderSvc"
!define WC_SERVICE_PAYLOAD "$INSTDIR\resources\wincommander-svc.exe"
!define WC_SERVICE_EXE "$INSTDIR\wincommander-svc.exe"

!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "${WC_SERVICE_PAYLOAD}" wc_service_payload_ok 0
    Abort "The WinCommander service payload is missing; the installation was not completed."
  wc_service_payload_ok:
  ; Stop the existing service before replacing its owned executable. The
  ; separately entitled Pro helper is installed on demand and is never bundled
  ; into, replaced by, or required for a Free installer update.
  nsExec::ExecToStack 'sc.exe stop ${WC_SERVICE_NAME}'
  Pop $0
  Pop $1
  ; Keep the service beside the protected app executable. The service's peer
  ; authorization derives this exact install root, so do not run it from a
  ; mutable user profile or ProgramData download directory.
  CopyFiles /SILENT "${WC_SERVICE_PAYLOAD}" "${WC_SERVICE_EXE}"
  nsExec::ExecToStack 'sc.exe create ${WC_SERVICE_NAME} binPath= $\"${WC_SERVICE_EXE}$\" start= auto obj= LocalSystem'
  Pop $0
  Pop $1
  ${If} $0 != 0
  ${AndIf} $0 != 1073
    Abort "WinCommander could not register its machine service."
  ${EndIf}
  nsExec::ExecToStack 'sc.exe config ${WC_SERVICE_NAME} binPath= $\"${WC_SERVICE_EXE}$\" start= auto obj= LocalSystem'
  Pop $0
  Pop $1
  ${If} $0 != 0
    Abort "WinCommander could not configure its machine service."
  ${EndIf}
  nsExec::ExecToStack 'sc.exe start ${WC_SERVICE_NAME}'
  Pop $0
  Pop $1
  ${If} $0 != 0
  ${AndIf} $0 != 1056
    Abort "WinCommander could not start its machine service."
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
    Abort "WinCommander could not create the Vault Policy Administrators group."
  ${EndIf}
  nsExec::ExecToStack 'net.exe localgroup "WinCommander Vault Policy Administrators" "$USERNAME" /add'
  Pop $0
  Pop $1
  ${If} $0 != 0
  ${AndIf} $0 != 1378
    Abort "WinCommander could not grant Vault policy administration to the installing account."
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
  ; The paid sidecar and its metadata are separately entitlement-installed
  ; runtime assets in ProgramData. Do not remove them during a Free uninstall.
!macroend
