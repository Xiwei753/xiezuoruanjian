param(
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [string]$QtRoot = $env:QT_ROOT
)

$ErrorActionPreference = "Stop"
$AppDir = Split-Path -Parent $PSScriptRoot

if ($QtRoot) {
    $env:QT_ROOT = $QtRoot
    $env:QMAKE = Join-Path $QtRoot "bin\qmake.exe"
}

Push-Location $AppDir
try {
    $cargoArgs = @("build", "-p", "sujian-windows-qt", "-p", "sujian_mcp")
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
