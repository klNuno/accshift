; NSIS installer hooks of the egui build (bundle/tauri.conf.json).
;
; The Tauri build's hooks without the app DLL: the egui accshift-gui.exe is
; the whole app. An install over v1.0.x removes the DLL that version left
; beside the exe; the user PATH integration is the same (path.nsh).

!include "${__FILEDIR__}\..\..\..\src-tauri\windows\path.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ; Runs once the installer has closed any running Accshift, so the old DLL
  ; is not locked.
  Delete "$INSTDIR\accshift_gui_lib.dll"

  !insertmacro ACCSHIFT_PATH_ADD
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; An install the post-install step could not clean.
  Delete "$INSTDIR\accshift_gui_lib.dll"
  RMDir "$INSTDIR"

  !insertmacro ACCSHIFT_PATH_REMOVE
!macroend
