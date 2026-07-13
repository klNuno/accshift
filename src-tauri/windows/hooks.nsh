; Tauri NSIS installer hooks (bundle > windows > nsis > installerHooks).
;
; Adds the install directory to the user PATH so the bundled accshift CLI
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
!include "LogicLib.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ; Pass the path as process state, never as PowerShell source code. Install
  ; directories may legally contain apostrophes or other shell metacharacters.
  System::Call 'Kernel32::SetEnvironmentVariable(t "ACCSHIFT_INSTALL_DIR", t "$INSTDIR") i .r1'
  ${If} $1 = 0
    DetailPrint "Could not prepare Accshift PATH integration"
    SetErrors
  ${Else}
    nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$$ErrorActionPreference='Stop';try{$$dir=[Environment]::GetEnvironmentVariable('ACCSHIFT_INSTALL_DIR','Process');if([string]::IsNullOrWhiteSpace($$dir) -or $$dir.Contains(';')){throw 'Invalid install directory'};$$sep=[char]92;$$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$$true);if($$null -eq $$k){throw 'Could not open user environment key'};$$p=[string]$$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);$$n=$$dir.TrimEnd($$sep);$$exists=@($$p -split ';' | Where-Object { $$_ -and [string]::Equals($$_.TrimEnd($$sep),$$n,[StringComparison]::OrdinalIgnoreCase) }).Count -gt 0;$$markerPath='Software'+$$sep+'Accshift'+$$sep+'Installer';$$m=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($$markerPath);$$owned=[string]$$m.GetValue('PathEntry','');if(-not $$exists){$$kind=if($$k.GetValueNames() -contains 'Path'){$$k.GetValueKind('Path')}else{[Microsoft.Win32.RegistryValueKind]::ExpandString};$$k.SetValue('Path',($$p.TrimEnd(';')+';'+$$dir).TrimStart(';'),$$kind);$$m.SetValue('PathEntry',$$dir,[Microsoft.Win32.RegistryValueKind]::String)}elseif(-not [string]::Equals($$owned,$$dir,[StringComparison]::OrdinalIgnoreCase)){$$m.DeleteValue('PathEntry',$$false)};$$m.Close();$$k.Close()}catch{Write-Error $$_;exit 1}"`
    Pop $0
    System::Call 'Kernel32::SetEnvironmentVariable(t "ACCSHIFT_INSTALL_DIR", t 0)'
    ${If} $0 = 0
      ; Tell running apps (Explorer included) the environment changed, so new
      ; shells pick up the PATH without a logout.
      SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
    ${Else}
      DetailPrint "Could not add Accshift to the user PATH (PowerShell exit code $0)"
      SetErrors
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  System::Call 'Kernel32::SetEnvironmentVariable(t "ACCSHIFT_INSTALL_DIR", t "$INSTDIR") i .r1'
  ${If} $1 = 0
    DetailPrint "Could not prepare Accshift PATH cleanup"
    SetErrors
  ${Else}
    ; Remove only the exact entry this installer recorded as its own. A PATH
    ; entry that existed before installation is deliberately left untouched.
    nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$$ErrorActionPreference='Stop';try{$$dir=[Environment]::GetEnvironmentVariable('ACCSHIFT_INSTALL_DIR','Process');if([string]::IsNullOrWhiteSpace($$dir)){throw 'Missing install directory'};$$sep=[char]92;$$markerPath='Software'+$$sep+'Accshift'+$$sep+'Installer';$$m=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($$markerPath,$$true);if($$null -ne $$m){$$owned=[string]$$m.GetValue('PathEntry','');if([string]::Equals($$owned,$$dir,[StringComparison]::OrdinalIgnoreCase)){$$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$$true);if($$null -eq $$k){throw 'Could not open user environment key'};$$p=[string]$$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);$$n=$$dir.TrimEnd($$sep);$$all=@($$p -split ';' | Where-Object { $$_ });$$e=@($$all | Where-Object { -not [string]::Equals($$_.TrimEnd($$sep),$$n,[StringComparison]::OrdinalIgnoreCase) });if($$e.Count -ne $$all.Count){$$kind=if($$k.GetValueNames() -contains 'Path'){$$k.GetValueKind('Path')}else{[Microsoft.Win32.RegistryValueKind]::ExpandString};$$k.SetValue('Path',($$e -join ';'),$$kind)};$$k.Close();$$m.DeleteValue('PathEntry',$$false)};$$m.Close()}}catch{Write-Error $$_;exit 1}"`
    Pop $0
    System::Call 'Kernel32::SetEnvironmentVariable(t "ACCSHIFT_INSTALL_DIR", t 0)'
    ${If} $0 = 0
      SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
    ${Else}
      DetailPrint "Could not remove Accshift from the user PATH (PowerShell exit code $0)"
      SetErrors
    ${EndIf}
  ${EndIf}
!macroend
