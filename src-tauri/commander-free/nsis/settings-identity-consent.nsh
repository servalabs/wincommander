; Only an explicit Yes authorizes a new identity. Silent setup always says No.
!macro WC_REQUEST_SETTINGS_IDENTITY_CONSENT
  StrCpy $WCSettingsIdentityConsent 0
  MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "WinCommander could not verify whether an older device signing identity exists. A new identity cannot restore the old one; previously provisioned wipe or recovery devices may stop working. Existing files will be kept, but automatic restoration is not available.$\r$\n$\r$\nCreate a new identity and continue setup? Choose No if the previous identity is needed." /SD IDNO IDYES +2
  Goto +2
  StrCpy $WCSettingsIdentityConsent 1
!macroend
