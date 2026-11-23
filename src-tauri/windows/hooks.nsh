!macro NSIS_HOOK_PREINSTALL
  nsExec::Exec 'taskkill /F /IM kestrel.exe'
  Sleep 800
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'taskkill /F /IM kestrel.exe'
  nsExec::Exec 'schtasks /Delete /TN kestrel /F'
  nsExec::Exec 'powershell -NoProfile -NonInteractive -Command "Remove-NetFirewallRule -Group kestrel -ErrorAction SilentlyContinue"'
!macroend
