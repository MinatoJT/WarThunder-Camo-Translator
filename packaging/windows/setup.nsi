Unicode true
RequestExecutionLevel user
ManifestSupportedOS Win10
SetCompressor /SOLID lzma
CRCCheck force

!include "MUI2.nsh"
!include "WinVer.nsh"
!include "x64.nsh"

!define PRODUCT "Camo Translator"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\CamoTranslator"

Name "${PRODUCT}"
BrandingText "${PRODUCT} ${VERSION}"
OutFile "${SETUP_EXE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT} Setup"
VIAddVersionKey "FileVersion" "${VERSION}.0"
VIAddVersionKey "LegalCopyright" "Camo Translator contributors"

; Compress the NSIS loader before the compiler appends the payload and its CRC.
!tempfile InstallerHeader
!packhdr "${InstallerHeader}" '"${UPX_EXE}" -9 --compress-icons=0 "${InstallerHeader}"'

!define MUI_ABORTWARNING
!define MUI_ICON "${PRODUCT_ICON}"
!define MUI_UNICON "${PRODUCT_ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\camo-translator.exe"
!define MUI_FINISHPAGE_RUN_NOTCHECKED

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

LangString UnsupportedWindows ${LANG_ENGLISH} "This installer requires Windows 10 or 11 on an x64 PC."
LangString UnsupportedWindows ${LANG_SIMPCHINESE} "此安装包需要 64 位 Windows 10 或 Windows 11。"

Function .onInit
    ${IfNot} ${AtLeastWin10}
    ${OrIfNot} ${IsNativeAMD64}
        MessageBox MB_OK|MB_ICONSTOP "$(UnsupportedWindows)" /SD IDOK
        SetErrorLevel 1
        Abort
    ${EndIf}
FunctionEnd

Section "${PRODUCT}"
    SetShellVarContext current
    SetOutPath "$INSTDIR\fonts"
    File "${FONT_DIR}\NotoSansCJKsc-Regular.otf"
    File "${FONT_DIR}\OFL.txt"

    SetOutPath "$INSTDIR"
    File "${RELEASE_DIR}\camo-translator.exe"
    File "${RELEASE_DIR}\camo-translator-cli.exe"
    File "/oname=Camo Translator.txt" "${USAGE_FILE}"
    File "/oname=LICENSE.txt" "${PRODUCT_LICENSE}"
    File "${NOTICE_FILE}"
    WriteUninstaller "$INSTDIR\Uninstall.exe"

    CreateDirectory "$SMPROGRAMS\${PRODUCT}"
    CreateShortCut "$SMPROGRAMS\${PRODUCT}\${PRODUCT}.lnk" "$INSTDIR\camo-translator.exe"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\camo-translator.exe"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
    WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
    SetShellVarContext current
    Delete "$SMPROGRAMS\${PRODUCT}\${PRODUCT}.lnk"
    RMDir "$SMPROGRAMS\${PRODUCT}"
    Delete "$INSTDIR\fonts\NotoSansCJKsc-Regular.otf"
    Delete "$INSTDIR\fonts\OFL.txt"
    RMDir "$INSTDIR\fonts"
    Delete "$INSTDIR\camo-translator.exe"
    Delete "$INSTDIR\camo-translator-cli.exe"
    Delete "$INSTDIR\Camo Translator.txt"
    Delete "$INSTDIR\LICENSE.txt"
    Delete "$INSTDIR\Third-party notices.txt"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
