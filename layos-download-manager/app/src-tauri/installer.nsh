; Extra installer steps for Layó's Download Manager (Tauri NSIS hooks).

!macro NSIS_HOOK_POSTINSTALL
  ; Let Chrome, Edge, Firefox, Opera, Brave and Vivaldi talk to the app.
  nsExec::ExecToLog '"$INSTDIR\ldm-native-host.exe" --register'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Leave nothing behind: browser bridge, magnet/.torrent handlers, start-with-Windows entry.
  nsExec::ExecToLog '"$INSTDIR\ldm-native-host.exe" --unregister'
  nsExec::ExecToLog '"$INSTDIR\LayosDownloadManager.exe" --unregister-associations'
!macroend
