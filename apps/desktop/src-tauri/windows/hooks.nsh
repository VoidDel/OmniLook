; Omni Look installer hooks (Tauri NSIS). Installing turns on starting with Windows (the default;
; the settings window can turn it off) and starts the agent, which registers the keys OmniZip
; finds it by (docs/development-plan.md §6.1). Uninstalling stops the agent and removes all of it.

!macro NSIS_HOOK_PREINSTALL
  ; An update replaces the running agent.
  IfFileExists "$INSTDIR\omnilook.exe" 0 +3
    nsExec::Exec '"$INSTDIR\omnilook.exe" --quit'
    Sleep 500
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Omni Look" '"$INSTDIR\omnilook.exe" --agent'
  Exec '"$INSTDIR\omnilook.exe" --agent'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec '"$INSTDIR\omnilook.exe" --quit'
  Sleep 500
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Omni Look"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Omni Look"
  DeleteRegKey HKCU "Software\OmniLook"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\omnilook.exe"
!macroend
