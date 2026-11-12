; Only an explicit Yes authorizes a new identity. Silent setup always says No.
!macro WC_REQUEST_SETTINGS_IDENTITY_CONSENT
  StrCpy $WCSettingsIdentityConsent 0
  MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "WinCommander could not verify whether an older device signing identity exists. A new identity cannot restore the old one; previously provisioned wipe or recovery devices may stop working. Existing files will be kept, but automatic restoration is not available.$\r$\n$\r$\nCreate a new identity and continue setup? Choose No if the previous identity is needed." /SD IDNO IDYES +2
  Goto +2
  StrCpy $WCSettingsIdentityConsent 2
!macroend

; The machine key already exists and will not be replaced. An older user
; profile may nevertheless have used a different identity.
!macro WC_REQUEST_EXISTING_MACHINE_IDENTITY_CONSENT
  StrCpy $WCSettingsIdentityConsent 0
  MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "Setup will keep and use the existing machine-wide device signing identity; it will not replace that key. An older per-user installation may have used a different identity, and its previously provisioned wipe or recovery devices may stop working. Existing files will be kept, but automatic recovery of an older identity is not available.$\r$\n$\r$\nContinue with the existing machine identity? Choose No if you need to preserve use of the per-user identity." /SD IDNO IDYES +2
  Goto +2
  StrCpy $WCSettingsIdentityConsent 3
!macroend

; Approval to use an existing machine key must never authorize creating one.
!macro WC_CHECK_NEW_IDENTITY_CONSENT result
  StrCpy ${result} 0
  StrCmp $WCSettingsIdentityConsent 2 0 +2
  StrCpy ${result} 1
!macroend
