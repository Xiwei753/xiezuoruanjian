# build_harmony.ps1 - Windows build script for HarmonyOS FFI library
#
# Cross-compiles Rust writer_core FFI .so using DevEco Studio's OHOS NDK.
# Output: apps/harmony/entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so
#
# Prerequisites:
#   - Rust toolchain + rustup target add aarch64-unknown-linux-ohos
#   - DevEco Studio NEXT (includes OHOS NDK)
#
# Usage:
#   .\tools\build_harmony.ps1
#   .\tools\build_harmony.ps1 -Release

param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"

$WorkspaceRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$PrebuiltDir = Join-Path $WorkspaceRoot "apps\harmony\entry\src\main\prebuilt\arm64-v8a"

Write-Host "=== Sujian HarmonyOS FFI Build (Windows) ===" -ForegroundColor Cyan
Write-Host ""

# Find OHOS NDK
$NdkHome = $env:OHOS_NDK_HOME
if (-not $NdkHome) {
    $DevEcoSdkPaths = @(
        "${env:LOCALAPPDATA}\Huawei\Sdk\openharmony\native",
        "C:\Users\$env:USERNAME\ruanjian\DevEco Studio\sdk\default\openharmony\native",
        "C:\Users\$env:USERNAME\devecosdk\openharmony\native"
    )
    foreach ($p in $DevEcoSdkPaths) {
        if (Test-Path (Join-Path $p "llvm\bin\clang.exe")) {
            $NdkHome = $p
            break
        }
    }
}

# If NDK path has spaces, create a junction to avoid RUSTFLAGS parsing issues
if ($NdkHome -and $NdkHome -match " ") {
    $JunctionPath = "C:\Users\$env:USERNAME\devecosdk\openharmony\native"
    if (Test-Path $JunctionPath) {
        $NdkHome = $JunctionPath
    } else {
        Write-Host "WARNING: NDK path contains spaces which may cause build issues" -ForegroundColor Yellow
        Write-Host "Create a junction: New-Item -ItemType Junction -Path 'C:\Users\$env:USERNAME\devecosdk' -Target '<DevEco SDK path>'"
    }
}

if (-not $NdkHome -or -not (Test-Path (Join-Path $NdkHome "llvm\bin\clang.exe"))) {
    Write-Host "ERROR: OHOS NDK not found" -ForegroundColor Red
    Write-Host ""
    Write-Host "Set environment variable:"
    Write-Host "  `$env:OHOS_NDK_HOME = 'C:\path\to\Sdk\openharmony\native'"
    Write-Host ""
    Write-Host "Or install OHOS NDK via DevEco Studio"
    exit 1
}

Write-Host "OHOS NDK: $NdkHome" -ForegroundColor Green

# Check Rust target
$targetInstalled = rustup target list | Select-String "aarch64-unknown-linux-ohos \(installed\)"
if (-not $targetInstalled) {
    Write-Host "ERROR: aarch64-unknown-linux-ohos target not installed" -ForegroundColor Red
    Write-Host "Run: rustup target add aarch64-unknown-linux-ohos"
    exit 1
}

# Create prebuilt directory
if (-not (Test-Path $PrebuiltDir)) {
    New-Item -ItemType Directory -Path $PrebuiltDir -Force | Out-Null
}

# Clean old library
$oldSo = Join-Path $PrebuiltDir "libwriter_core_ffi.so"
if (Test-Path $oldSo) {
    Remove-Item $oldSo -Force
    Write-Host "Cleaned old FFI library"
}

# Configure linker environment variables
$ClangPath = Join-Path $NdkHome "llvm\bin\clang.exe"
$ArPath = Join-Path $NdkHome "llvm\bin\llvm-ar.exe"
$LdPath = Join-Path $NdkHome "llvm\bin\ld.lld.exe"

$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER = $LdPath
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_AR = $ArPath

# ring and other C deps need CC for cross-compilation
$env:CC_aarch64_unknown_linux_ohos = $ClangPath
$env:AR_aarch64_unknown_linux_ohos = $ArPath
$env:CXX_aarch64_unknown_linux_ohos = (Join-Path $NdkHome "llvm\bin\clang++.exe")

Write-Host "Linker: $LdPath"
Write-Host "CC: $ClangPath"
Write-Host "AR: $ArPath"
Write-Host ""

# Build
$buildArgs = @("build", "--target", "aarch64-unknown-linux-ohos", "--no-default-features", "--features", "harmony-ffi")
if ($Release) {
    $buildArgs += "--release"
    $profileDir = "release"
} else {
    $profileDir = "debug"
}

# Add sysroot and library search paths for OHOS system libs
# Use junction path (no spaces) so RUSTFLAGS don't need quoting
$SysrootDir = Join-Path $NdkHome "sysroot"
$SysrootLibDir = Join-Path $SysrootDir "usr\lib\aarch64-linux-ohos"
$LlvmLibDir = Join-Path $NdkHome "llvm\lib\aarch64-linux-ohos"
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_RUSTFLAGS = "-C link-arg=--sysroot=$SysrootDir -C link-arg=-L$SysrootLibDir -C link-arg=-L$LlvmLibDir"

Write-Host "cargo $($buildArgs -join ' ')" -ForegroundColor Yellow
Write-Host ""

Push-Location (Join-Path $WorkspaceRoot "core\writer_core")
try {
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "ERROR: Rust FFI build failed" -ForegroundColor Red
        exit 1
    }
} finally {
    Pop-Location
}

# Copy .so to prebuilt
$soSource = Join-Path $WorkspaceRoot "target\aarch64-unknown-linux-ohos\$profileDir\libwriter_core.so"
if (-not (Test-Path $soSource)) {
    Write-Host "ERROR: Build artifact not found: $soSource" -ForegroundColor Red
    exit 1
}

Copy-Item $soSource $oldSo -Force

if (-not (Test-Path $oldSo)) {
    Write-Host "ERROR: Failed to copy libwriter_core_ffi.so" -ForegroundColor Red
    exit 1
}

$fileSize = (Get-Item $oldSo).Length / 1KB
Write-Host ""
Write-Host "=== Build Succeeded ===" -ForegroundColor Green
Write-Host "  FFI library: $oldSo"
Write-Host "  Size: $([math]::Round($fileSize, 1)) KB"
Write-Host ""
Write-Host "Next: Build HarmonyOS app in DevEco Studio"
