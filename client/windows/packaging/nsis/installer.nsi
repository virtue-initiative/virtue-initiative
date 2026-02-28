Unicode true

!define APP_NAME "BePure"
!define SERVICE_NAME "BePureCaptureService"
!ifndef PRODUCT_VERSION
!define PRODUCT_VERSION "0.1.0"
!endif
!ifndef BUILD_TARGET_DIR
!define BUILD_TARGET_DIR "..\\..\\target"
!endif
!ifndef OUTFILE
!define OUTFILE "bepure-windows-installer-${PRODUCT_VERSION}.exe"
!endif

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\\BePure"
InstallDirRegKey HKLM "Software\\BePure" "InstallDir"
RequestExecutionLevel admin

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  WriteRegStr HKLM "Software\\BePure" "InstallDir" "$INSTDIR"

  ; Ensure old tray instances are gone before replacing binaries/restarting tray.
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-tray.exe"'
  Sleep 1000

  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\bepure-service.exe"
  File "${BUILD_TARGET_DIR}\\x86_64-pc-windows-msvc\\release\\bepure-tray.exe"

  ExpandEnvStrings $0 "%ProgramData%"
  CreateDirectory "$0\\BePure"
  CreateDirectory "$0\\BePure\\config"
  CreateDirectory "$0\\BePure\\data"

  nsExec::ExecToLog '"$SYSDIR\\cmd.exe" /C icacls "$0\\BePure" /grant *S-1-5-32-545:(OI)(CI)M /T /C'

  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" create ${SERVICE_NAME} binPath= "\"$INSTDIR\\bepure-service.exe\"" start= auto DisplayName= "BePure Capture Service"'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" description ${SERVICE_NAME} "Captures screenshots and uploads via BePure core"'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" start ${SERVICE_NAME}'

  WriteRegStr HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray" '"$INSTDIR\\bepure-tray.exe"'

  WriteUninstaller "$INSTDIR\\Uninstall.exe"
  Exec '"$INSTDIR\\bepure-tray.exe"'
SectionEnd

Section "Uninstall"
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" stop ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\sc.exe" delete ${SERVICE_NAME}'
  nsExec::ExecToLog '"$SYSDIR\\taskkill.exe" /F /T /IM "bepure-tray.exe"'

  DeleteRegValue HKLM "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "BePureTray"

  Delete "$INSTDIR\\bepure-service.exe"
  Delete "$INSTDIR\\bepure-tray.exe"
  Delete "$INSTDIR\\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "Software\\BePure"
SectionEnd
