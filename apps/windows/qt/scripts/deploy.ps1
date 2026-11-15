param(
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [string]$QtRoot = $env:QT_ROOT,
    [string]$CraftRoot = $env:CRAFT_ROOT,
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"
$AppDir = Split-Path -Parent $PSScriptRoot
$RepoRoot = Resolve-Path (Join-Path $AppDir "..\..\..")
$Profile = if ($Configuration -eq "release") { "release" } else { "debug" }
$TargetDir = Join-Path $RepoRoot "target\$Profile"
$Exe = Join-Path $TargetDir "sujian-windows-qt.exe"
$McpExe = Join-Path $TargetDir "sujian_mcp.exe"

if (-not (Test-Path $Exe)) {
    throw "找不到 $Exe，请先执行 build.ps1"
}
if (-not (Test-Path $McpExe)) {
    throw "找不到 $McpExe，请先执行 build.ps1（会同时构建 sujian_mcp）"
}

if (-not $OutputDir) {
    $OutputDir = Join-Path $AppDir "dist\$Configuration"
}
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

if ($QtRoot) {
    $env:QMAKE = Join-Path $QtRoot "bin\qmake.exe"
}

$Windeployqt = if ($QtRoot) {
    Join-Path $QtRoot "bin\windeployqt.exe"
} else {
    (Get-Command windeployqt.exe -ErrorAction Stop).Source
}

# QRC 已把 QML 编译进 exe；windeployqt 仍需要扫描 qmldir 才能收集 Qt 模块。
# 共享 QML 与 Linux 同源。org.kde.* QML 模块（Kirigami/KirigamiAddons 等）
# 不是 Qt 官方模块，windeployqt 不会收集，由下方 Craft 部署段随包复制。
$LinuxQml = Join-Path $RepoRoot "apps\Linux_qt\qml"
$CommonArgs = @("--dir", $OutputDir, "--compiler-runtime")
if ($Configuration -eq "release") { $CommonArgs += "--release" } else { $CommonArgs += "--debug" }

& $Windeployqt @CommonArgs "--qmldir" $LinuxQml $Exe
if ($LASTEXITCODE -ne 0) { throw "windeployqt (共享 QML) 失败" }

Copy-Item $Exe $OutputDir -Force

# 平台适配动态库与其它随包 Rust/第三方 DLL。
Get-ChildItem $TargetDir -Filter "*.dll" | ForEach-Object {
    Copy-Item $_.FullName $OutputDir -Force
}

# MCP sidecar：Cargo 实际产物名为 sujian_mcp.exe，客户端按该稳定文件名查找。
Copy-Item $McpExe $OutputDir -Force

# 便携版需要应用本地的 MSVC 运行库。只从 Visual Studio 官方 Redist
# 目录部署可再分发 CRT，不能从 Windows System32 / debug_nonredist 取文件。
$CrtCandidates = [System.Collections.Generic.List[string]]::new()
if ($env:VCToolsRedistDir) {
    $CrtCandidates.Add((Join-Path $env:VCToolsRedistDir "x64\Microsoft.VC143.CRT"))
}
$RedistRoots = [System.Collections.Generic.List[string]]::new()
if ($env:VCINSTALLDIR) {
    $RedistRoots.Add((Join-Path $env:VCINSTALLDIR "Redist\MSVC"))
}
$ProgramFilesX86 = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFilesX86)
$VsWhere = Join-Path $ProgramFilesX86 "Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path -LiteralPath $VsWhere) {
    $VsInstall = (& $VsWhere -latest -products "*" -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
    if ($VsInstall) {
        $RedistRoots.Add((Join-Path $VsInstall "VC\Redist\MSVC"))
    }
}
foreach ($root in $RedistRoots) {
    if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
    foreach ($version in (Get-ChildItem -LiteralPath $root -Directory | Sort-Object Name -Descending)) {
        $x64Dir = Join-Path $version.FullName "x64"
        if (-not (Test-Path -LiteralPath $x64Dir -PathType Container)) { continue }
        foreach ($crt in (Get-ChildItem -LiteralPath $x64Dir -Directory -Filter "Microsoft.VC143.CRT")) {
            $CrtCandidates.Add($crt.FullName)
        }
    }
}
$CrtDir = $CrtCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Container } | Select-Object -First 1
if (-not $CrtDir) {
    throw "找不到 Visual Studio x64 Microsoft.VC143.CRT 官方再分发目录，无法生成独立便携版。"
}
Copy-Item -Path (Join-Path $CrtDir "*.dll") -Destination $OutputDir -Force
foreach ($name in @("MSVCP140.dll", "MSVCP140_1.dll", "MSVCP140_2.dll", "VCRUNTIME140.dll", "VCRUNTIME140_1.dll")) {
    if (-not (Test-Path -LiteralPath (Join-Path $OutputDir $name))) {
        throw "MSVC 运行库部署不完整：缺少 $name"
    }
}
Write-Host "MSVC x64 CRT 已部署：$CrtDir"

# KDE Kirigami / KirigamiAddons / qqc2-desktop-style / KIconThemes / Breeze
# 运行时由 KDE Craft 构建并与构建用 Qt 同一 MSVC ABI（共享 QML 的
# Kirigami.ApplicationWindow、FormCard 等直接使用真实 KDE 组件，不再使用
# 仓库内手写替身）。windeployqt 不收集非 Qt 的 org.kde.* 模块，必须从
# CraftRoot 随包复制，并通过 qt.conf 的 QmlImports = qml 定位。
#
# KDE Frameworks DLL 命名不以 "lib" 开头（如 KF6Kirigami.dll），不能仅凭
# lib*.dll 筛选；需复制 CraftRoot/bin 下全部 DLL，并按 Qt 目录层级部署
# QML 模块和插件（imageformats/iconengines/styles），确保递归依赖闭包完整。
if (-not $CraftRoot) {
    throw "需要 -CraftRoot 或环境变量 CRAFT_ROOT 指向 KDE Craft 安装，以便随包部署 Kirigami 运行时。"
}
$CraftRootPath = (Resolve-Path $CraftRoot).Path

# 1. 复制 CraftRoot/bin 下全部 DLL（KDE Frameworks DLL 不以 lib 开头）
$CraftBin = Join-Path $CraftRootPath "bin"
$AllDlls = Get-ChildItem -LiteralPath $CraftBin -Filter "*.dll" -ErrorAction SilentlyContinue
if (-not $AllDlls) {
    throw "CraftRoot bin 目录缺少 DLL：$CraftBin"
}
foreach ($dll in $AllDlls) {
    Copy-Item -LiteralPath $dll.FullName -Destination $OutputDir -Force
}
Write-Host "已复制 $($AllDlls.Count) 个 DLL 从 $CraftBin"

# 2. 复制 KDE QML 模块（org/kde/*）
$CraftQml = Join-Path $CraftRootPath "qml"
if (-not (Test-Path -LiteralPath (Join-Path $CraftQml "org\kde\kirigami\qmldir") -PathType Leaf)) {
    throw "Craft 安装缺少 org.kde.kirigami QML 模块：$CraftQml（请先 craft 安装 kde/frameworks/tier1/kirigami）"
}
$KdeQmlTarget = Join-Path $OutputDir "qml\org\kde"
New-Item -ItemType Directory -Force -Path $KdeQmlTarget | Out-Null
Copy-Item -Path (Join-Path $CraftQml "org\kde\*") -Destination $KdeQmlTarget -Recurse -Force
foreach ($rel in @(
    "kirigami\qmldir",
    "kirigamiaddons\formcard\qmldir",
    "kirigamiaddons\delegates\qmldir",
    "kirigamiaddons\dateandtime\qmldir"
)) {
    if (-not (Test-Path -LiteralPath (Join-Path $KdeQmlTarget $rel) -PathType Leaf)) {
        throw "KDE QML 模块部署不完整：缺少 org/kde/$rel（CraftRoot: $CraftRootPath）"
    }
}

# 3. 复制 QQC2 Desktop Style（QtQuick/Controls/org.kde.desktop）
$DesktopStyleSource = Join-Path $CraftQml "QtQuick\Controls\org.kde.desktop"
if (Test-Path -LiteralPath $DesktopStyleSource -PathType Container) {
    $StyleTarget = Join-Path $OutputDir "qml\QtQuick\Controls"
    New-Item -ItemType Directory -Force -Path $StyleTarget | Out-Null
    Copy-Item -LiteralPath $DesktopStyleSource -Destination $StyleTarget -Recurse -Force
} else {
    throw "Craft 安装缺少 QQC2 Desktop Style：$DesktopStyleSource（请先 craft 安装 kde/frameworks/tier3/qqc2-desktop-style）"
}

# 4. 复制 Qt 插件（KDE 组件依赖 imageformats/iconengines/styles）
$CraftPlugins = Join-Path $CraftRootPath "plugins"
if (Test-Path -LiteralPath $CraftPlugins -PathType Container) {
    foreach ($pluginDir in @("imageformats", "iconengines", "styles")) {
        $srcDir = Join-Path $CraftPlugins $pluginDir
        if (Test-Path -LiteralPath $srcDir -PathType Container) {
            $dstDir = Join-Path $OutputDir $pluginDir
            New-Item -ItemType Directory -Force -Path $dstDir | Out-Null
            Copy-Item -Path (Join-Path $srcDir "*.dll") -Destination $dstDir -Force
        }
    }
}

# 5. 复制 Breeze 图标主题
$IconSourceRoot = Join-Path $CraftRootPath "share\icons"
if (Test-Path -LiteralPath $IconSourceRoot -PathType Container) {
    $IconTarget = Join-Path $OutputDir "share\icons"
    New-Item -ItemType Directory -Force -Path $IconTarget | Out-Null
    Get-ChildItem -LiteralPath $IconSourceRoot -Directory -Filter "breeze*" | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $IconTarget -Recurse -Force
    }
}
if (-not (Test-Path -LiteralPath (Join-Path $OutputDir "share\icons\breeze") -PathType Container)) {
    throw "Breeze 图标主题未随包部署（CraftRoot: $CraftRootPath）"
}

# 6. 复制许可证
$LicenseSource = Join-Path $CraftRootPath "share\licenses"
if (Test-Path -LiteralPath $LicenseSource -PathType Container) {
    $LicenseTarget = Join-Path $OutputDir "share\licenses"
    New-Item -ItemType Directory -Force -Path $LicenseTarget | Out-Null
    Copy-Item -Path (Join-Path $LicenseSource "*") -Destination $LicenseTarget -Recurse -Force
}
Write-Host "KDE Kirigami 运行时与 Breeze 图标已随包部署（来源：$CraftRootPath）"

# Bundle MacBERT4CSC and the native Windows ONNX Runtime fetched by build.ps1.
$CorrectionRoot = Join-Path $AppDir "generated\correction"
$ModelSource = Join-Path $CorrectionRoot "macbert4csc"
$ModelTarget = Join-Path $OutputDir "models\macbert4csc"
foreach ($name in @("model.onnx", "vocab.txt", "tokenizer.json", "calibrator.json")) {
    if (-not (Test-Path -LiteralPath (Join-Path $ModelSource $name) -PathType Leaf)) {
        throw "Windows 中文纠错模型不完整：缺少 $name"
    }
}
New-Item -ItemType Directory -Force -Path $ModelTarget | Out-Null
Copy-Item -Path (Join-Path $ModelSource "*") -Destination $ModelTarget -Recurse -Force

$RuntimeSource = Join-Path $CorrectionRoot "runtime"
foreach ($name in @("onnxruntime.dll", "onnxruntime_providers_shared.dll")) {
    $source = Join-Path $RuntimeSource $name
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Windows ONNX Runtime 不完整：缺少 $name"
    }
    Copy-Item -LiteralPath $source -Destination $OutputDir -Force
}
Write-Host "Windows 内置纠错模型和 ONNX Runtime 已部署：$ModelTarget"

# Resolve Qt plugins/QML from the portable application directory on ordinary
# Windows machines that do not have a developer Qt installation.
$QtConfig = Join-Path $AppDir "qt.conf"
if (-not (Test-Path -LiteralPath $QtConfig -PathType Leaf)) {
    throw "缺少 Windows Qt 便携包的 qt.conf：$QtConfig"
}
Copy-Item -LiteralPath $QtConfig -Destination (Join-Path $OutputDir "qt.conf") -Force

Write-Host "Windows Qt 发布目录：$OutputDir"
