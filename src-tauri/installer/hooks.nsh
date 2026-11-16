; Castaway installer hooks (Tauri NSIS bundle, perMachine => runs elevated).
; The rule name MUST match FIREWALL_RULE_NAME in src/health.rs.
; Private profile only: networks Windows classes as Public still block discovery.
; netsh is invoked by full path ($SYSDIR) so a planted netsh.exe next to the
; installer or in the cwd can never run elevated.

!macro NSIS_HOOK_POSTINSTALL
  ; Delete any existing rule first so upgrades/reinstalls stay idempotent.
  nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="Castaway (UxPlay)"'
  Pop $0
  nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall add rule name="Castaway (UxPlay)" dir=in action=allow program="$INSTDIR\resources\uxplay\uxplay.exe" enable=yes profile=private'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="Castaway (UxPlay)"'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; "Start with Windows" is stored as a Run value; remove it so no broken
  ; startup entry is left behind. Skipped during updates, which keep it.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  ${EndIf}
!macroend
