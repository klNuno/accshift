; User PATH integration of the Windows installer, shared by the hooks of the
; Tauri build (src-tauri/windows/hooks.nsh) and of the egui build
; (crates/accshift-egui/bundle/hooks.nsh).
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
; The scripts run from files (path-add.ps1, path-remove.ps1): the same limit
; cuts any command line longer than 1024 chars, and the inline commands they
; replace never ran whole.

!include "WinMessages.nsh"
!include "LogicLib.nsh"

; This file's directory, fixed here: a macro body sees the caller's.
!define ACCSHIFT_PATH_DIR "${__FILEDIR__}"

!macro ACCSHIFT_PATH_ADD
  ; Pass the path as process state, never as PowerShell source code. Install
  ; directories may legally contain apostrophes or other shell metacharacters.
  System::Call 'Kernel32::SetEnvironmentVariable(t "ACCSHIFT_INSTALL_DIR", t "$INSTDIR") i .r1'
  ${If} $1 = 0
    DetailPrint "Could not prepare Accshift PATH integration"
    SetErrors
  ${Else}
    InitPluginsDir
    File "/oname=$PLUGINSDIR\accshift-path-add.ps1" "${ACCSHIFT_PATH_DIR}\path-add.ps1"
    nsExec::ExecToLog 'powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\accshift-path-add.ps1"'
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

!macro ACCSHIFT_PATH_REMOVE
  System::Call 'Kernel32::SetEnvironmentVariable(t "ACCSHIFT_INSTALL_DIR", t "$INSTDIR") i .r1'
  ${If} $1 = 0
    DetailPrint "Could not prepare Accshift PATH cleanup"
    SetErrors
  ${Else}
    ; Remove only the exact entry this installer recorded as its own. A PATH
    ; entry that existed before installation is deliberately left untouched.
    InitPluginsDir
    File "/oname=$PLUGINSDIR\accshift-path-remove.ps1" "${ACCSHIFT_PATH_DIR}\path-remove.ps1"
    nsExec::ExecToLog 'powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\accshift-path-remove.ps1"'
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
