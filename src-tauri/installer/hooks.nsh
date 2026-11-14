; Castaway installer hooks (Tauri NSIS bundle, perMachine => runs elevated).
; The rule name MUST match FIREWALL_RULE_NAME in src/health.rs.
; Private profile only: networks Windows classes as Public still block discovery.

!macro NSIS_HOOK_POSTINSTALL
  ; Delete any existing rule first so upgrades/reinstalls stay idempotent.
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Castaway (UxPlay)"'
  Pop $0
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Castaway (UxPlay)" dir=in action=allow program="$INSTDIR\resources\uxplay\uxplay.exe" enable=yes profile=private'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Castaway (UxPlay)"'
  Pop $0
!macroend
