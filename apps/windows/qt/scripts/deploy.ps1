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

Write-Host "Windows Qt 发布目录：$OutputDir"
