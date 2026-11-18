param(
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [string]$QtRoot = $env:QT_ROOT
)

$ErrorActionPreference = "Stop"
$AppDir = Split-Path -Parent $PSScriptRoot
$RepoRoot = (Resolve-Path (Join-Path $AppDir "..\..\..")).Path

if ($QtRoot) {
    $env:QT_ROOT = $QtRoot
    $env:QMAKE = Join-Path $QtRoot "bin\qmake.exe"
}

Push-Location $AppDir
try {
    # Build the complete offline package from the same pinned assets as Linux.
    $FetchTool = Join-Path $RepoRoot "crates\sujian-proof\tools\fetch_model.py"
    $AssetRoot = Join-Path $AppDir "generated\correction"
    $Python = (Get-Command python -ErrorAction Stop).Source
    & $Python $FetchTool --output (Join-Path $AssetRoot "macbert4csc") --runtime windows-x64 --runtime-output (Join-Path $AssetRoot "runtime")
    if ($LASTEXITCODE -ne 0) {
        throw "准备 Windows 内置纠错模型及 ONNX Runtime 失败，退出码 $LASTEXITCODE"
    }

    $cargoArgs = @("build", "-p", "sujian-windows-qt", "-p", "sujian_mcp", "--features", "sujian-windows-qt/ai", "--bin", "sujian-agent-host", "--bin", "sujian-mcp-bridge")
    if ($Configuration -eq "release") {
        $cargoArgs += "--release"
    }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build 失败，退出码 $LASTEXITCODE"
    }

    & (Join-Path $PSScriptRoot "deploy.ps1") -Configuration $Configuration -QtRoot $QtRoot
} finally {
    Pop-Location
}
