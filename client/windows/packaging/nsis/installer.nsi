Unicode true

!define APP_NAME "Virtue"
!define SERVICE_NAME "VirtueCaptureService"
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
  Sleep 1000

  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\virtue-service.exe"
  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\virtue-tray.exe"

  ExpandEnvStrings $0 "%ProgramData%"
  CreateDirectory "$0\\Virtue"
  CreateDirectory "$0\\Virtue\\config"
  CreateDirectory "$0\\Virtue\\data"

  nsExec::ExecToLog '"$SYSDIR\\cmd.exe" /C icacls "$0\\Virtue" /grant *S-1-5-32-545:(OI)(CI)M /T /C'

  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" create ${SERVICE_NAME} binPath= "\"$INSTDIR\\virtue-service.exe\"" start= auto DisplayName= "Virtue Capture Service"'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" description ${SERVICE_NAME} "Captures screenshots and uploads via Virtue core"'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" start ${SERVICE_NAME}'

  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray" '"$INSTDIR\\virtue-tray.exe"'

  WriteUninstaller "$INSTDIR\\Uninstall.exe"
  Exec '"$INSTDIR\\virtue-tray.exe"'
SectionEnd

Section "Uninstall"
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "virtue-tray.exe"'

  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "VirtueTray"

  Delete "$INSTDIR\\virtue-service.exe"
  Delete "$INSTDIR\\virtue-tray.exe"
  Delete "$INSTDIR\\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "Software\\Virtue"
SectionEnd
