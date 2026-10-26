; RCL 安装包的附加步骤（Tauri NSIS hooks）

!macro NSIS_HOOK_POSTINSTALL
  ; 从 1.0.x 升级：旧版本的产品名是「Red Craft」，安装在另一个目录。
  ; 静默卸载旧版本（不删除用户数据），并在旧位置留一个新程序的副本：
  ; 1.0.x 的更新程序装完后会重新打开旧路径，新版本发现自己在旧位置时，
  ; 会转去打开新安装的 RCL 并删除旧目录。
  StrCpy $R0 ""
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Red Craft" "InstallLocation"
  StrCpy $R1 $R0 1
  ${If} $R1 == '"'
    StrCpy $R0 $R0 "" 1
    StrCpy $R0 $R0 -1
  ${EndIf}
  ${If} $R0 == ""
    StrCpy $R0 "$LOCALAPPDATA\Red Craft"
  ${EndIf}
  ${If} $R0 != $INSTDIR
  ${AndIf} ${FileExists} "$R0\uninstall.exe"
    ExecWait '"$R0\uninstall.exe" /S _?=$R0'
    Delete "$R0\uninstall.exe"
    CreateDirectory "$R0"
    CopyFiles /SILENT "$INSTDIR\${MAINBINARYNAME}.exe" "$R0\${MAINBINARYNAME}.exe"
  ${EndIf}
!macroend
