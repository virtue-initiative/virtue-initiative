Unicode true

!define APP_NAME "Virtue"
!define SERVICE_NAME "VirtueCaptureService"
!define LEGACY_SERVICE_NAME "BePureCaptureService"
!define CAPTURE_TASK_NAME "VirtueCaptureUser"
!define CAPTURE_LAUNCHER_VBS "virtue-capture.vbs"
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

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  WriteRegStr HKLM "Software\\Virtue" "InstallDir" "$INSTDIR"

  ; Ensure old tray instances are gone before replacing binaries/restarting tray.
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-tray.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-tray.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-service.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-service.exe"'
  Sleep 1000

  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\virtue-service.exe"
  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\virtue-tray.exe"

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

  ; Capture must run in an interactive user session (not Session 0 service).
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /Delete /TN "${CAPTURE_TASK_NAME}" /F'
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /Create /TN "${CAPTURE_TASK_NAME}" /SC ONLOGON /RL LIMITED /TR "\"$SYSDIR\\wscript.exe\" //B \"$INSTDIR\\${CAPTURE_LAUNCHER_VBS}\"" /F'
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /Run /TN "${CAPTURE_TASK_NAME}"'

  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"
  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray" '"$INSTDIR\\virtue-tray.exe"'

  WriteUninstaller "$INSTDIR\\Uninstall.exe"
  Exec '"$INSTDIR\\virtue-tray.exe"'
SectionEnd

Section "Uninstall"
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /End /TN "${CAPTURE_TASK_NAME}"'
  nsExec::ExecToLog '"$SYSDIR\\schtasks.exe" /Delete /TN "${CAPTURE_TASK_NAME}" /F'

  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${LEGACY_SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${LEGACY_SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-tray.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-tray.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-service.exe"'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-service.exe"'

  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"
  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray"

  Delete "$INSTDIR\\bepure-service.exe"
  Delete "$INSTDIR\\bepure-tray.exe"
  Delete "$INSTDIR\\virtue-service.exe"
  Delete "$INSTDIR\\virtue-tray.exe"
  Delete "$INSTDIR\\${CAPTURE_LAUNCHER_VBS}"
  Delete "$INSTDIR\\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "Software\\BePure"
  DeleteRegKey HKLM "Software\\Virtue"
SectionEnd
