param(
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [string]$QtRoot = $env:QT_ROOT,
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
$WindowsQml = Join-Path $AppDir "qml"
$LinuxQml = Join-Path $RepoRoot "apps\Linux_qt\qml"
$CommonArgs = @("--dir", $OutputDir, "--compiler-runtime")
if ($Configuration -eq "release") { $CommonArgs += "--release" } else { $CommonArgs += "--debug" }

& $Windeployqt @CommonArgs "--qmldir" $WindowsQml $Exe
if ($LASTEXITCODE -ne 0) { throw "windeployqt (Windows QML) 失败" }

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
