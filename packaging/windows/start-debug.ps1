# =============================================================================
# start-debug.ps1 — 素笺写作 Windows 打包版 debug 启动器
# =============================================================================
#
# 用法:
#   .\start-debug.ps1                  默认 debug 模式
#   .\start-debug.ps1 -Modules sync    只调试同步模块
#   .\start-debug.ps1 -Modules tree    调试树/项目/卷/章节/编辑器
#   .\start-debug.ps1 -Modules ui      调试 UI 相关模块
#   .\start-debug.ps1 -Modules all     调试所有模块
#   .\start-debug.ps1 -Level trace     trace 级别日志
#   .\start-debug.ps1 -QtVerbose       开启 Qt 详细日志
# =============================================================================

param(
    [string]$Modules = "",
    [string]$Level = "",
    [switch]$QtVerbose
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$exe = Get-ChildItem -Path $scriptDir -Filter "sujian-windows-*.exe" | Select-Object -First 1
if (-not $exe) {
    Write-Host "Error: sujian-windows-*.exe not found in $scriptDir" -ForegroundColor Red
    exit 1
}

$WRITER_DEBUG_MODULES_DEFAULT = "app,workspace,tree,project,volume,chapter,sync,settings"
$WRITER_DEBUG_LEVEL_DEFAULT = "info"

if ($Modules -ne "") {
    $env:WRITER_DEBUG_MODULES = $Modules
} else {
    if (-not $env:WRITER_DEBUG_MODULES) { $env:WRITER_DEBUG_MODULES = $WRITER_DEBUG_MODULES_DEFAULT }
}

if ($Level -ne "") {
    $env:WRITER_DEBUG_LEVEL = $Level
} else {
    if (-not $env:WRITER_DEBUG_LEVEL) { $env:WRITER_DEBUG_LEVEL = $WRITER_DEBUG_LEVEL_DEFAULT }
}

if ($QtVerbose) {
    $env:WRITER_DEBUG_QT_VERBOSE = "1"
} else {
    if (-not $env:WRITER_DEBUG_QT_VERBOSE) { $env:WRITER_DEBUG_QT_VERBOSE = "0" }
}

$env:WRITER_DEBUG = "1"
$env:WRITER_DEBUG_QML = "1"
$env:RUST_BACKTRACE = "full"
if (-not $env:RUST_LOG) { $env:RUST_LOG = "warn" }

$env:QML_DEBUGGING_ENABLED = "1"
$env:QML_IMPORT_TRACE = "1"
$env:QT_DEBUG_PLUGINS = "1"

if (-not $env:QT_LOGGING_RULES) {
    if ($env:WRITER_DEBUG_QT_VERBOSE -eq "1") {
        $env:QT_LOGGING_RULES = "*.debug=true;qt.quick.hover.trace=false;qt.scenegraph.renderloop=false;qt.quick.mouse.target=false;qt.quick.mouse=false;qt.qml.warning=true;*.warning=true;*.critical=true"
    } else {
        $env:QT_LOGGING_RULES = "*.debug=false;qt.quick.hover.trace=false;qt.scenegraph.renderloop=false;qt.quick.mouse.target=false;qt.quick.mouse=false;qt.quick.dirty=false;qt.scenegraph.time.*=false;qt.qml.warning=true;*.warning=true;*.critical=true"
    }
}

$logsDir = Join-Path $scriptDir "logs"
if (-not (Test-Path $logsDir)) {
    New-Item -ItemType Directory -Path $logsDir | Out-Null
}

$oldLogs = Get-ChildItem $logsDir -Filter "sujian-debug-*.log" | Sort-Object LastWriteTime -Descending | Select-Object -Skip 20
if ($oldLogs) {
    Remove-Item $oldLogs -Force
}

$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$logFile = Join-Path $logsDir "sujian-debug-$timestamp.log"

Write-Host "=== Debug Configuration ==="
Write-Host "Exe: $($exe.Name)"
Write-Host "Debug modules: $env:WRITER_DEBUG_MODULES"
Write-Host "Debug level: $env:WRITER_DEBUG_LEVEL"
Write-Host "RUST_LOG: $env:RUST_LOG"
$qtVerboseLabel = if ($env:WRITER_DEBUG_QT_VERBOSE -eq "1") { "enabled" } else { "disabled" }
Write-Host "Qt verbose: $qtVerboseLabel"
Write-Host "QT_LOGGING_RULES: $env:QT_LOGGING_RULES"
Write-Host "QML_DEBUGGING_ENABLED: $env:QML_DEBUGGING_ENABLED"
Write-Host "QML_IMPORT_TRACE: $env:QML_IMPORT_TRACE"
Write-Host "QT_DEBUG_PLUGINS: $env:QT_DEBUG_PLUGINS"
Write-Host "Log file: $logFile"
Write-Host "==========================="

Write-Host "[debug] Launching $($exe.Name)..."
& $exe.FullName 2>&1 | Tee-Object -FilePath $logFile

Write-Host ""
Write-Host "[debug] Exited. Log saved to: $logFile"
