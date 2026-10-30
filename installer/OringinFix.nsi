; OringinFix — NSIS 3 (Unicode + MUI2)
; 构建：cargo build --release 后 makensis /DVERSION=0.1.1 installer\OringinFix.nsi
; 输出：dist\OringinFix-Setup-x.y.z-windows-x64.exe

Unicode true
CRCCheck on
SetCompressor /SOLID lzma
ManifestDPIAware true

!ifndef VERSION
  !define VERSION "0.1.1"
!endif

!define APP_NAME "OringinFix"
!define APP_PUBLISHER "su"
!define APP_URL "https://github.com/luckysus/OringinFix"
!define APP_EXE "OringinFix.exe"
!define REG_UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
!define REG_RUN "Software\Microsoft\Windows\CurrentVersion\Run"

; 标题不带版本号，更清爽；版本只写进文件属性 / 卸载项
Name "${APP_NAME}"
Caption "安装 ${APP_NAME}"
UninstallCaption "卸载 ${APP_NAME}"
OutFile "..\dist\${APP_NAME}-Setup-${VERSION}-windows-x64.exe"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "${REG_UNINST}" "InstallLocation"
RequestExecutionLevel user
BrandingText " "

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define MUI_ABORTWARNING
!define MUI_ICON "..\assets\app.ico"
!define MUI_UNICON "..\assets\app.ico"

Var CreateDesktop
Var TasksDialog
Var DesktopCheck
Var ClearUserData
Var UnConfirmDialog
Var ClearDataCheck

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
Page custom TasksPageCreate TasksPageLeave
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!insertmacro MUI_PAGE_FINISH

; 卸载确认页自带「删除用户数据」勾选，不用事后弹窗
UninstPage custom un.ConfirmPageCreate un.ConfirmPageLeave
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "ProductName" "${APP_NAME}"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "FileDescription" "${APP_NAME} 安装程序"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "CompanyName" "${APP_PUBLISHER}"
VIAddVersionKey /LANG=${LANG_SIMPCHINESE} "LegalCopyright" "${APP_PUBLISHER}"

Function .onInit
  StrCpy $CreateDesktop "0"
FunctionEnd

Function TasksPageCreate
  !insertmacro MUI_HEADER_TEXT "选择附加任务" "您想要安装程序执行哪些附加任务？"
  nsDialogs::Create 1018
  Pop $TasksDialog
  ${If} $TasksDialog == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 24u "请选择要执行的附加任务，然后点击「下一步」。"
  Pop $0

  ${NSD_CreateGroupBox} 0 36u 100% 40u "附加快捷方式"
  Pop $0

  ${NSD_CreateCheckbox} 12u 52u 90% 12u "创建桌面快捷方式"
  Pop $DesktopCheck
  ${If} $CreateDesktop == "1"
    ${NSD_Check} $DesktopCheck
  ${EndIf}

  nsDialogs::Show
FunctionEnd

Function TasksPageLeave
  ${NSD_GetState} $DesktopCheck $0
  ${If} $0 == ${BST_CHECKED}
    StrCpy $CreateDesktop "1"
  ${Else}
    StrCpy $CreateDesktop "0"
  ${EndIf}
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  File "..\target\release\${APP_EXE}"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\卸载 ${APP_NAME}.lnk" "$INSTDIR\Uninstall.exe"

  ${If} $CreateDesktop == "1"
    CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  ${EndIf}

  WriteRegStr HKCU "${REG_UNINST}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${REG_UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${REG_UNINST}" "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKCU "${REG_UNINST}" "URLInfoAbout" "${APP_URL}"
  WriteRegStr HKCU "${REG_UNINST}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKCU "${REG_UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${REG_UNINST}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${REG_UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${REG_UNINST}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${REG_UNINST}" "EstimatedSize" "$0"
SectionEnd

Function un.onInit
  StrCpy $ClearUserData "0"
  nsExec::ExecToLog 'taskkill /F /IM ${APP_EXE} /T'
  Pop $0
  Sleep 600
FunctionEnd

Function un.ConfirmPageCreate
  ; 常见做法：页头短文案 + 路径单独一行 + 勾选短句，路径放勾选下方说明
  !insertmacro MUI_HEADER_TEXT "确认卸载" "从计算机中移除 ${APP_NAME}"
  nsDialogs::Create 1018
  Pop $UnConfirmDialog
  ${If} $UnConfirmDialog == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0u 100% 12u "安装位置："
  Pop $0

  ${NSD_CreateLabel} 0 16u 100% 12u "$INSTDIR"
  Pop $0

  ${NSD_CreateCheckbox} 0 48u 100% 12u "删除用户数据（配置、日志）"
  Pop $ClearDataCheck
  ${If} $ClearUserData == "1"
    ${NSD_Check} $ClearDataCheck
  ${Else}
    ${NSD_Uncheck} $ClearDataCheck
  ${EndIf}

  ${NSD_CreateLabel} 16u 66u 100% 12u "%AppData%\${APP_NAME}"
  Pop $0

  nsDialogs::Show
FunctionEnd

Function un.ConfirmPageLeave
  ${NSD_GetState} $ClearDataCheck $0
  ${If} $0 == ${BST_CHECKED}
    StrCpy $ClearUserData "1"
  ${Else}
    StrCpy $ClearUserData "0"
  ${EndIf}
FunctionEnd

Section "Uninstall"
  DeleteRegValue HKCU "${REG_RUN}" "${APP_NAME}"

  nsExec::ExecToLog 'taskkill /F /IM ${APP_EXE} /T'
  Pop $0
  Sleep 400

  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR"

  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\卸载 ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  DeleteRegKey HKCU "${REG_UNINST}"

  ${If} $ClearUserData == "1"
    RMDir /r "$APPDATA\${APP_NAME}"
  ${EndIf}
SectionEnd
