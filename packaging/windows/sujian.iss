; 素笺写作 Windows 安装程序配置
; Inno Setup 6 脚本
;
; 编译示例：
;   ISCC /DAppVersion=0.1.0 /DFlavor=no-ai sujian.iss
;   ISCC /DAppVersion=0.1.0 /DFlavor=ai sujian.iss
;
; 部署目录需通过 /DDeployDir 指定，或默认为 "deploy"

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef Flavor
  #define Flavor "no-ai"
#endif
#ifndef DeployDir
  #define DeployDir "deploy"
#endif

[Setup]
AppId={{B7E3F2A1-4C5D-6E8F-9A0B-1C2D3E4F5A6B}
AppName=素笺写作
AppVersion={#AppVersion}
AppPublisher=Xiwei
AppPublisherURL=https://github.com/Xiwei753/xiezuoruanjian
DefaultDirName={autopf}\素笺写作
DefaultGroupName=素笺写作
OutputBaseFilename=素笺写作安装包-{#Flavor}
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=app.ico
UninstallDisplayIcon={app}\sujian.exe
PrivilegesRequired=admin
WizardStyle=modern
SetupLogging=yes
CloseApplications=yes
CloseApplicationsFilter=*.exe,sujian.exe

[Languages]
Name: "chinese"; MessagesFile: "compiler:Default.isl"

[InstallDelete]
; 仅清理调试文件，不再预删除 Qt 依赖目录和 DLL（由 ShouldInstallFile 按需跳过未变更文件）
Type: files; Name: "{app}\*.pdb"
Type: files; Name: "{app}\*.lib"

[Files]
; 主程序：重命名为 sujian.exe，始终覆盖
Source: "{#DeployDir}\sujian-windows-{#Flavor}.exe"; DestName: "sujian.exe"; DestDir: "{app}"; Flags: ignoreversion

; VC++ 运行库：仅临时提取用于安装，不安装到目标目录
Source: "{#DeployDir}\vc_redist.x64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall; Check: VCRedistSourceExists

; 其他所有文件（递归），排除主程序 exe 和 VC++ 运行库
; 使用 ShouldInstallFile 检查：仅当目标文件不存在或内容不同时才安装，复用未变更依赖
Source: "{#DeployDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs; Excludes: "sujian-windows-*.exe,vc_redist.x64.exe"; Check: ShouldInstallFile

[Icons]
Name: "{group}\素笺写作"; Filename: "{app}\sujian.exe"
Name: "{autodesktop}\素笺写作"; Filename: "{app}\sujian.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加图标:"

[Run]
Filename: "{tmp}\vc_redist.x64.exe"; Parameters: "/install /quiet /norestart"; StatusMsg: "正在安装 Visual C++ 运行库..."; Flags: runhidden skipifdoesntexist; Check: VCRedistExists

[Code]
function VCRedistSourceExists: Boolean;
begin
  Result := FileExists(ExpandConstant('{#DeployDir}\vc_redist.x64.exe'));
end;

function VCRedistExists: Boolean;
begin
  Result := FileExists(ExpandConstant('{tmp}\vc_redist.x64.exe'));
end;

// 判断文件是否为核心文件（主程序或核心动态库），核心文件始终强制覆盖
function IsCoreFile(const FileName: string): Boolean;
begin
  Result := (CompareText(FileName, 'sujian.exe') = 0) or
            (CompareText(FileName, 'writer_core.dll') = 0);
end;

// 覆盖安装时复用未变依赖的检查函数
// 逻辑：
//   1. 核心文件（sujian.exe、writer_core.dll）→ 始终安装
//   2. 目标文件不存在 → 安装
//   3. 源文件与目标文件大小不同 → 安装
//   4. 大小相同但 SHA-256 哈希不同 → 安装
//   5. 大小和哈希均相同 → 跳过安装（复用已有文件）
function ShouldInstallFile: Boolean;
var
  SrcFile, DestFile, RelPath, DeployDirPath: string;
  SrcSize, DestSize: Integer;
begin
  // 默认安装
  Result := True;

  SrcFile := CurrentFileName;
  DeployDirPath := ExpandConstant('{#DeployDir}');

  // 计算源文件相对于部署目录的相对路径，进而得到目标路径
  // SrcFile 格式示例: C:\deploy\platforms\qwindows.dll
  // DeployDirPath 格式: C:\deploy
  // RelPath = platforms\qwindows.dll
  if CompareText(Copy(SrcFile, 1, Length(DeployDirPath)), DeployDirPath) = 0 then
  begin
    RelPath := Copy(SrcFile, Length(DeployDirPath) + 1, MaxInt);
    // 去除开头的路径分隔符
    if (Length(RelPath) > 0) and (Copy(RelPath, 1, 1) = '\') then
      RelPath := Copy(RelPath, 2, MaxInt);
  end
  else
  begin
    // 源文件路径不以部署目录开头（不应发生），退化为仅使用文件名
    RelPath := ExtractFileName(SrcFile);
  end;

  DestFile := ExpandConstant('{app}') + '\' + RelPath;

  // 核心文件始终覆盖安装
  if IsCoreFile(ExtractFileName(DestFile)) then
  begin
    Log(Format('强制覆盖核心文件: %s', [DestFile]));
    Exit;
  end;

  // 目标文件不存在，需要安装
  if not FileExists(DestFile) then
  begin
    Log(Format('新文件安装: %s', [DestFile]));
    Exit;
  end;

  // 比较文件大小
  SrcSize := GetFileSize(SrcFile);
  DestSize := GetFileSize(DestFile);

  // 无法获取源文件大小，安全起见安装
  if SrcSize = -1 then
  begin
    Log(Format('无法获取源文件大小，强制安装: %s', [DestFile]));
    Exit;
  end;

  // 无法获取目标文件大小，安全起见安装
  if DestSize = -1 then
  begin
    Log(Format('无法获取目标文件大小，强制安装: %s', [DestFile]));
    Exit;
  end;

  // 大小不同，需要安装
  if SrcSize <> DestSize then
  begin
    Log(Format('文件大小不同 (%d vs %d)，更新: %s', [SrcSize, DestSize, DestFile]));
    Exit;
  end;

  // 大小相同，比较 SHA-256 哈希
  try
    if GetSHA256OfFile(SrcFile) = GetSHA256OfFile(DestFile) then
    begin
      Log(Format('跳过未变更文件: %s', [DestFile]));
      Result := False;
    end
    else
    begin
      Log(Format('文件哈希不同，更新: %s', [DestFile]));
    end;
  except
    // 哈希计算失败，安全起见安装
    Log(Format('哈希计算异常，强制安装: %s', [DestFile]));
    Result := True;
  end;
end;
