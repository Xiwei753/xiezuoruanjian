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
Type: filesandordirs; Name: "{app}\platforms"
Type: filesandordirs; Name: "{app}\imageformats"
Type: filesandordirs; Name: "{app}\styles"
Type: filesandordirs; Name: "{app}\tls"
Type: filesandordirs; Name: "{app}\qml"
Type: filesandordirs; Name: "{app}\*.dll"
Type: files; Name: "{app}\*.pdb"
Type: files; Name: "{app}\*.lib"

[Files]
; 主程序：重命名为 sujian.exe
Source: "{#DeployDir}\sujian-windows-{#Flavor}.exe"; DestName: "sujian.exe"; DestDir: "{app}"; Flags: ignoreversion

; VC++ 运行库：仅临时提取用于安装，不安装到目标目录
Source: "{#DeployDir}\vc_redist.x64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall; Check: VCRedistSourceExists

; 其他所有文件（递归），排除主程序 exe 和 VC++ 运行库
Source: "{#DeployDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "sujian-windows-*.exe,vc_redist.x64.exe"

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
