$ErrorActionPreference = "Stop"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot ".." "..")
$coreDir = Join-Path $projectRoot "core" "writer_core"
$outputDir = Join-Path $projectRoot "apps" "windows" "bin"

Write-Host "Building writer_core.dll for Windows..."

Push-Location $coreDir
try
{
    cargo build --release --features harmony-ffi
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    if (-not (Test-Path $outputDir)) { New-Item -ItemType Directory -Path $outputDir | Out-Null }

    $dllPath = Join-Path $coreDir "target" "release" "writer_core.dll"
    if (Test-Path $dllPath)
    {
        Copy-Item $dllPath $outputDir -Force
        Write-Host "Copied writer_core.dll to $outputDir"
    }
    else
    {
        Write-Error "writer_core.dll not found at $dllPath"
        exit 1
    }
}
finally
{
    Pop-Location
}
