Unicode true

!define APP_NAME "Virtue"
!define SERVICE_NAME "VirtueCaptureService"
!define LEGACY_SERVICE_NAME "BePureCaptureService"
!define CAPTURE_TASK_NAME "VirtueCaptureUser"
!define CAPTURE_LAUNCHER_VBS "virtue-capture.vbs"
!define CAPTURE_STARTUP_SHORTCUT "Virtue Capture.lnk"
!define TRAY_STARTUP_SHORTCUT "Virtue Tray.lnk"
!ifndef PRODUCT_VERSION
!define PRODUCT_VERSION "0.1.0"
!endif
!ifndef BUILD_TARGET_DIR
!define BUILD_TARGET_DIR "..\\..\\target"
!endif
!ifndef OUTFILE
!define OUTFILE "virtue-windows-installer-${PRODUCT_VERSION}.exe"
!endif

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\\Virtue"
InstallDirRegKey HKLM "Software\\Virtue" "InstallDir"
RequestExecutionLevel admin
Icon "..\\..\\assets\\app-icon.ico"
UninstallIcon "..\\..\\assets\\app-icon.ico"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetRegView 64
  SetOutPath "$INSTDIR"
  WriteRegStr HKLM "Software\\Virtue" "InstallDir" "$INSTDIR"

  ; Ensure old tray instances are gone before replacing binaries/restarting tray.
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-tray.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-auth-ui.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-tray.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-service.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-service.exe"'
  Sleep 1000

  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\virtue-service.exe"
  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\virtue-tray.exe"
  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\virtue-auth-ui.exe"
  File "..\\..\\assets\\app-icon.ico"

  ; Launch helper to run the capture agent hidden (no console window).
  FileOpen $1 "$INSTDIR\\${CAPTURE_LAUNCHER_VBS}" w
  FileWrite $1 'Dim shell$\r$\n'
  FileWrite $1 'Set shell = CreateObject("WScript.Shell")$\r$\n'
  FileWrite $1 'shell.Run Chr(34) & "$INSTDIR\\virtue-service.exe" & Chr(34) & " --console", 0, False$\r$\n'
  FileWrite $1 'Set shell = Nothing$\r$\n'
  FileClose $1

  ExpandEnvStrings $0 "%ProgramData%"
  CreateDirectory "$0\\Virtue"
  CreateDirectory "$0\\Virtue\\config"
  CreateDirectory "$0\\Virtue\\data"

  nsExec::ExecToLog '"$SYSDIR\\cmd.exe" /C icacls "$0\\Virtue" /grant *S-1-5-32-545:(OI)(CI)M /T /C'

  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${LEGACY_SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${LEGACY_SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${SERVICE_NAME}'
  Sleep 500

  ; Remove legacy task-based launcher (kept for migration compatibility).
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /End /TN "${CAPTURE_TASK_NAME}"'
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /Delete /TN "${CAPTURE_TASK_NAME}" /F'
  Sleep 500

  ; Upgrade migration: preserve legacy state/dev overrides from %ProgramData%\\BePure
  ; when the new %ProgramData%\\Virtue files do not exist yet.
  IfFileExists "$0\\BePure\\config\\client_state.json" 0 +3
  IfFileExists "$0\\Virtue\\config\\client_state.json" +2 0
  CopyFiles /SILENT "$0\\BePure\\config\\client_state.json" "$0\\Virtue\\config"

  IfFileExists "$0\\BePure\\config\\token_store.json" 0 +3
  IfFileExists "$0\\Virtue\\config\\token_store.json" +2 0
  CopyFiles /SILENT "$0\\BePure\\config\\token_store.json" "$0\\Virtue\\config"

  IfFileExists "$0\\BePure\\config\\service.dev.env" 0 +3
  IfFileExists "$0\\Virtue\\config\\service.dev.env" +2 0
  CopyFiles /SILENT "$0\\BePure\\config\\service.dev.env" "$0\\Virtue\\config"

  IfFileExists "$0\\BePure\\data\\batch_buffer.json" 0 +3
  IfFileExists "$0\\Virtue\\data\\batch_buffer.json" +2 0
  CopyFiles /SILENT "$0\\BePure\\data\\batch_buffer.json" "$0\\Virtue\\data"

  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureCapture"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueCapture"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray"
  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueCapture" "$\"$SYSDIR\\wscript.exe$\" //B $\"$INSTDIR\\${CAPTURE_LAUNCHER_VBS}$\""
  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray" "$\"$INSTDIR\\virtue-tray.exe$\""
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureCapture"
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueCapture"
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray"

  ; Fallback launch path for logon reliability across Windows startup states.
  SetShellVarContext all
  Delete "$SMSTARTUP\${CAPTURE_STARTUP_SHORTCUT}"
  Delete "$SMSTARTUP\${TRAY_STARTUP_SHORTCUT}"
  CreateShortCut "$SMSTARTUP\${CAPTURE_STARTUP_SHORTCUT}" "$SYSDIR\\wscript.exe" "//B $\"$INSTDIR\\${CAPTURE_LAUNCHER_VBS}$\"" "$SYSDIR\\wscript.exe" 0
  CreateShortCut "$SMSTARTUP\${TRAY_STARTUP_SHORTCUT}" "$INSTDIR\\virtue-tray.exe" "" "$INSTDIR\\virtue-tray.exe" 0
  SetShellVarContext current

  ; Also clear stale 32-bit Run entries from older installer versions.
  SetRegView 32
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureCapture"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueCapture"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray"
  SetRegView 64

  WriteUninstaller "$INSTDIR\\Uninstall.exe"
  Exec '"$SYSDIR\\wscript.exe" //B "$INSTDIR\\${CAPTURE_LAUNCHER_VBS}"'
  Exec '"$INSTDIR\\virtue-tray.exe"'
SectionEnd

Section "Uninstall"
  SetRegView 64
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /End /TN "${CAPTURE_TASK_NAME}"'
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /Delete /TN "${CAPTURE_TASK_NAME}" /F'

  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${LEGACY_SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${LEGACY_SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-tray.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-auth-ui.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-tray.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-service.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-service.exe"'

  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureCapture"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueCapture"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray"
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureCapture"
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueCapture"
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray"

  SetRegView 32
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureCapture"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueCapture"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray"
  SetRegView 64
  SetShellVarContext all
  Delete "$SMSTARTUP\${CAPTURE_STARTUP_SHORTCUT}"
  Delete "$SMSTARTUP\${TRAY_STARTUP_SHORTCUT}"
  SetShellVarContext current

  Delete "$INSTDIR\\bepure-service.exe"
  Delete "$INSTDIR\\bepure-tray.exe"
  Delete "$INSTDIR\\virtue-service.exe"
  Delete "$INSTDIR\\virtue-tray.exe"
  Delete "$INSTDIR\\virtue-auth-ui.exe"
  Delete "$INSTDIR\\${CAPTURE_LAUNCHER_VBS}"
  Delete "$INSTDIR\\app-icon.ico"
  Delete "$INSTDIR\\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "Software\\BePure"
  DeleteRegKey HKLM "Software\\Virtue"
SectionEnd
