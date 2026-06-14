# =============================================================================
# start.ps1 — Windows 下启动素笺写作
# =============================================================================
#
# 用法:
#   .\start.ps1                  正常启动
#   .\start.ps1 debug            debug 模式启动
#   .\start.ps1 debug sync       debug 模式，只调试同步模块
#   .\start.ps1 debug trace      debug 模式，trace 级别
#   .\start.ps1 debug qt         debug 模式，开启 Qt 详细日志
# =============================================================================

param(
    [Parameter(Position=0)]
    [string]$Mode = "",

    [Parameter(ValueFromRemainingArguments)]
    [string[]$ExtraArgs
)

$ErrorActionPreference = "Stop"

$env:QT_QUICK_CONTROLS_STYLE = "Basic"

if ($Mode -eq "debug") {
    $debugScript = Join-Path $PSScriptRoot "start-debug.ps1"
    if (-not (Test-Path $debugScript)) {
        Write-Host "Error: start-debug.ps1 not found." -ForegroundColor Red
        exit 1
    }

    $modules = ""
    $level = ""
    $qtVerbose = $false

    foreach ($arg in $ExtraArgs) {
        switch ($arg) {
            "sync"   { $modules = "sync" }
            "tree"   { $modules = "tree,project,volume,chapter,editor" }
            "ui"     { $modules = "app,workspace,tree,settings" }
            "all"    { $modules = "all" }
            "trace"  { $level = "trace"; $modules = "all" }
            "qt"     { $qtVerbose = $true }
            default  { Write-Host "Error: Unknown debug parameter '$arg'" -ForegroundColor Red; Write-Host "Usage:"; Write-Host "  .\start.ps1 debug [sync|tree|ui|all|trace|qt]"; exit 1 }
        }
    }

    $params = @{}
    if ($modules) { $params["Modules"] = $modules }
    if ($level)   { $params["Level"] = $level }
    if ($qtVerbose) { $params["QtVerbose"] = $true }

    & $debugScript @params
    exit $LASTEXITCODE
}

if ($Mode -ne "" -and $Mode -ne "debug") {
    Write-Host "Error: Unknown argument '$Mode'" -ForegroundColor Red
    Write-Host "Usage:"
    Write-Host "  .\start.ps1"
    Write-Host "  .\start.ps1 debug [sync|tree|ui|all|trace|qt]"
    exit 1
}

# 正常启动模式
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$qtBase = "C:\Qt"
$qtVersion = $null
$qtArchDir = $null

if (Test-Path $qtBase) {
    $versionDirs = Get-ChildItem $qtBase -Directory | Sort-Object Name -Descending
    foreach ($vd in $versionDirs) {
        $archDirs = Get-ChildItem $vd.FullName -Directory | Where-Object { $_.Name -match "msvc" }
        if ($archDirs) {
            $qtVersion = $vd.Name
            $qtArchDir = $archDirs[0].FullName
            break
        }
    }
}

if ($qtArchDir) {
    $qtBinDir = Join-Path $qtArchDir "bin"
    $env:PATH = "$qtBinDir;$cargoBin;$env:PATH"
    $env:QMAKE = Join-Path $qtBinDir "qmake.exe"
    $env:QT_INCLUDE_PATH = Join-Path $qtArchDir "include"
    $env:QT_LIBRARY_PATH = Join-Path $qtArchDir "lib"
    $env:QT_VERSION_MAJOR = "6"
    $QT_VERSION_DETECTED = $qtVersion

    $qmlDir = Join-Path $qtArchDir "qml"
    $pluginDir = Join-Path $qtArchDir "plugins"
    $env:QML2_IMPORT_PATH = if ($env:QML2_IMPORT_PATH) { "$qmlDir;$env:QML2_IMPORT_PATH" } else { $qmlDir }
    $env:QML_IMPORT_PATH = if ($env:QML_IMPORT_PATH) { "$qmlDir;$env:QML_IMPORT_PATH" } else { $qmlDir }
    $env:QT_PLUGIN_PATH = if ($env:QT_PLUGIN_PATH) { "$pluginDir;$env:QT_PLUGIN_PATH" } else { $pluginDir }
} else {
    $env:PATH = "$cargoBin;$env:PATH"
    $QT_VERSION_DETECTED = "unknown"
}

Write-Host "[start] Qt version detected: $QT_VERSION_DETECTED"
Write-Host "[start] QMAKE: $(if ($env:QMAKE) { $env:QMAKE } else { 'not found' })"
Write-Host "[start] QT_INCLUDE_PATH: $(if ($env:QT_INCLUDE_PATH) { $env:QT_INCLUDE_PATH } else { '' })"
Write-Host "[start] QT_LIBRARY_PATH: $(if ($env:QT_LIBRARY_PATH) { $env:QT_LIBRARY_PATH } else { '' })"
Write-Host "[start] QML2_IMPORT_PATH: $(if ($env:QML2_IMPORT_PATH) { $env:QML2_IMPORT_PATH } else { '' })"
Write-Host "[start] QT_PLUGIN_PATH: $(if ($env:QT_PLUGIN_PATH) { $env:QT_PLUGIN_PATH } else { '' })"

Write-Host "[start] Building sujian-desktop package..."
cargo build -p sujian-desktop
if ($LASTEXITCODE -ne 0) {
    Write-Host "[start] Build failed." -ForegroundColor Red
    exit $LASTEXITCODE
}

Write-Host "[start] Running 素笺写作..."
cargo run -p sujian-desktop