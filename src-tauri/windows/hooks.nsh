; Tauri NSIS installer hooks (bundle > windows > nsis > installerHooks).
;
; Installs the app DLL beside accshift-gui.exe and removes it on uninstall,
; and adds the install directory to the user PATH (path.nsh).

!include "${__FILEDIR__}\path.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ; The app itself is accshift_gui_lib.dll, which accshift-gui.exe loads at
  ; startup (src-tauri/src/launcher.rs). Tauri bundles the main binary only,
  ; and cargo leaves the DLL in deps/ beside it. Written here, once the
  ; installer has closed any running Accshift, so the old DLL is not locked.
  ; A missing DLL fails makensis, never the installed app.
  !searchreplace ACCSHIFT_APP_DLL "${MAINBINARYSRCPATH}" "\${MAINBINARYNAME}.exe" "\deps\accshift_gui_lib.dll"
  SetOutPath $INSTDIR
  File "${ACCSHIFT_APP_DLL}"
  ; The size shown in Apps & features only counts what Tauri bundled.
  ReadRegDWORD $1 SHCTX "${UNINSTKEY}" "EstimatedSize"
  ${GetSize} "$INSTDIR" "/M=accshift_gui_lib.dll /S=0K /G=0" $0 $2 $3
  IntOp $0 $0 + $1
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" "$0"

  !insertmacro ACCSHIFT_PATH_ADD
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Tauri's own cleanup does not know the DLL, and left it behind its RMDir.
  Delete "$INSTDIR\accshift_gui_lib.dll"
  RMDir "$INSTDIR"

  !insertmacro ACCSHIFT_PATH_REMOVE
!macroend
