; Tauri NSIS installer hooks (bundle > windows > nsis > installerHooks).
;
; Adds the install directory to the user PATH so the bundled `accshift` CLI
; (shipped via externalBin) is callable from any shell, and removes it again
; on uninstall. The installer runs with installMode "currentUser", so the
; user-scoped HKCU\Environment key is the right target (no admin rights
; needed). If installMode ever changes to perMachine, this must move to
; HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment.
;
; The PATH read-modify-write is delegated to PowerShell. NSIS MUST NOT touch
; the value itself: ReadRegStr returns an empty string for values longer than
; the NSIS build-time string limit (1024 chars on the standard build), so a
; naive read-append-write silently WIPES a long PATH. PowerShell has no such
; limit and lets us do an exact entry-wise match instead of substring games.
; ($$ is the NSIS escape for a literal $, i.e. PowerShell's variable sigil.)

!include "WinMessages.nsh"

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$$true);$$p=[string]$$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);if(($$p -split ';') -notcontains '$INSTDIR'){$$k.SetValue('Path',($$p.TrimEnd(';')+';$INSTDIR').TrimStart(';'),[Microsoft.Win32.RegistryValueKind]::ExpandString)};$$k.Close()"`
  Pop $0
  ; Tell running apps (Explorer included) the environment changed, so new
  ; shells pick up the PATH without a logout.
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Entry-wise removal; only rewrites the value when the entry was present.
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$$true);$$p=[string]$$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);$$e=$$p -split ';';if($$e -contains '$INSTDIR'){$$k.SetValue('Path',(($$e|Where-Object{$$_ -and $$_ -ne '$INSTDIR'}) -join ';'),[Microsoft.Win32.RegistryValueKind]::ExpandString)};$$k.Close()"`
  Pop $0
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
