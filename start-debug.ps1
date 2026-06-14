# =============================================================================
# start-debug.ps1 — Windows 下启动素笺写作 debug 模式
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

$env:QT_QUICK_CONTROLS_STYLE = "Basic"

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

if (-not $env:QT_LOGGING_RULES) {
    if ($env:WRITER_DEBUG_QT_VERBOSE -eq "1") {
        $env:QT_LOGGING_RULES = "*.debug=true;qt.quick.hover.trace=false;qt.scenegraph.renderloop=false;qt.quick.mouse.target=false;qt.quick.mouse=false;qt.qml.warning=true;*.warning=true;*.critical=true"
    } else {
        $env:QT_LOGGING_RULES = "*.debug=false;qt.quick.hover.trace=false;qt.scenegraph.renderloop=false;qt.quick.mouse.target=false;qt.quick.mouse=false;qt.quick.dirty=false;qt.scenegraph.time.*=false;qt.qml.warning=true;*.warning=true;*.critical=true"
    }
}

# 设置 PATH：确保 cargo 和 Qt6 在路径中
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

# 创建日志目录
$logsDir = Join-Path $PSScriptRoot "logs"
if (-not (Test-Path $logsDir)) {
    New-Item -ItemType Directory -Path $logsDir | Out-Null
}

# 清理旧日志，保留最近 20 个
$oldLogs = Get-ChildItem $logsDir -Filter "sujian-desktop-debug-*.log" | Sort-Object LastWriteTime -Descending | Select-Object -Skip 20
if ($oldLogs) {
    Remove-Item $oldLogs -Force
}

$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$logFile = Join-Path $logsDir "sujian-desktop-debug-$timestamp.log"

# 打印调试配置
Write-Host "=== Debug Configuration ==="
Write-Host "Launch mode: debug"
Write-Host "Debug modules: $env:WRITER_DEBUG_MODULES"
Write-Host "Debug level: $env:WRITER_DEBUG_LEVEL"
Write-Host "RUST_LOG: $env:RUST_LOG"
$qtVerboseLabel = if ($env:WRITER_DEBUG_QT_VERBOSE -eq "1") { "enabled" } else { "disabled" }
Write-Host "Qt verbose: $qtVerboseLabel"
Write-Host "QT_LOGGING_RULES: $env:QT_LOGGING_RULES"
Write-Host "Qt version detected: $QT_VERSION_DETECTED"
Write-Host "QMAKE: $(if ($env:QMAKE) { $env:QMAKE } else { 'not found' })"
Write-Host "QT_INCLUDE_PATH: $(if ($env:QT_INCLUDE_PATH) { $env:QT_INCLUDE_PATH } else { '' })"
Write-Host "QT_LIBRARY_PATH: $(if ($env:QT_LIBRARY_PATH) { $env:QT_LIBRARY_PATH } else { '' })"
Write-Host "QML2_IMPORT_PATH: $(if ($env:QML2_IMPORT_PATH) { $env:QML2_IMPORT_PATH } else { '' })"
Write-Host "QT_PLUGIN_PATH: $(if ($env:QT_PLUGIN_PATH) { $env:QT_PLUGIN_PATH } else { '' })"
Write-Host "Log file path: $logFile"
if ($env:WRITER_DEBUG_QT_VERBOSE -eq "0") {
    Write-Host "Tip: Qt verbose logging is disabled by default. Use -QtVerbose to enable it."
}
Write-Host "==========================="

# 构建
Write-Host "[start-debug] Building sujian-desktop package..."
$buildProc = Start-Process -FilePath "cargo" -ArgumentList "build", "-p", "sujian-desktop" -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$logsDir\build-stdout.log" -RedirectStandardError "$logsDir\build-stderr.log"
$buildStdout = Get-Content "$logsDir\build-stdout.log" -Raw
$buildStderr = Get-Content "$logsDir\build-stderr.log" -Raw
$buildStdout | Out-File $logFile -Append -Encoding utf8
$buildStderr | Out-File $logFile -Append -Encoding utf8
Write-Host $buildStdout
Write-Host $buildStderr

if ($buildProc.ExitCode -ne 0) {
    Write-Host "[start-debug] Build failed with exit code $($buildProc.ExitCode)" -ForegroundColor Red
    exit $buildProc.ExitCode
}

# 运行
Write-Host "[start-debug] Running 素笺写作 with tracing..."
$runProc = Start-Process -FilePath "cargo" -ArgumentList "run", "-p", "sujian-desktop" -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$logsDir\run-stdout.log" -RedirectStandardError "$logsDir\run-stderr.log"
$runStdout = Get-Content "$logsDir\run-stdout.log" -Raw
$runStderr = Get-Content "$logsDir\run-stderr.log" -Raw
$runStdout | Out-File $logFile -Append -Encoding utf8
$runStderr | Out-File $logFile -Append -Encoding utf8
Write-Host $runStdout
Write-Host $runStderr

# 生成摘要
$summaryFile = Join-Path $logsDir "latest-summary.txt"
$sb = [System.Text.StringBuilder]::new()
[void]$sb.AppendLine("=== Debug Summary ===")
[void]$sb.AppendLine("Generated at: $(Get-Date)")
[void]$sb.AppendLine("Log file path: $logFile")
[void]$sb.AppendLine("WRITER_DEBUG_MODULES: $env:WRITER_DEBUG_MODULES")
[void]$sb.AppendLine("WRITER_DEBUG_LEVEL: $env:WRITER_DEBUG_LEVEL")
[void]$sb.AppendLine("")
[void]$sb.AppendLine("--- Staged debug and critical/warning/error logs ---")

if (Test-Path $logFile) {
    $logContent = Get-Content $logFile -Raw
    $filtered = $logContent -split "`n" | Where-Object {
        $_ -match "\[SujianDebug\]|error|warn|critical|conflict|failed|success" -and
        $_ -notmatch "qt\.quick|qt\.scenegraph|qt\.qpa|\[Qt DEBUG\]"
    }
    [void]$sb.AppendLine(($filtered -join "`n"))
    [void]$sb.AppendLine("")
    [void]$sb.AppendLine("--- Last 200 lines of non-Qt-debug output ---")
    $nonQt = $logContent -split "`n" | Where-Object { $_ -notmatch "qt\.quick|qt\.scenegraph|qt\.qpa|\[Qt DEBUG\]" }
    $tail = $nonQt | Select-Object -Last 200
    [void]$sb.AppendLine(($tail -join "`n"))
}

$sb.ToString() | Out-File $summaryFile -Encoding utf8

Write-Host ""
Write-Host "[start-debug] Run completed. Logs saved to: $logFile"
Write-Host "[start-debug] Summary generated at: $summaryFile"