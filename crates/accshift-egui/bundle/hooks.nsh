; NSIS installer hooks (tauri.conf.json).
;
; accshift-gui.exe is the whole app. An install over v1.0.x removes the DLL
; the Tauri versions left beside the exe; path.nsh adds the CLI to the user
; PATH.

!include "${__FILEDIR__}\path.nsh"

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
