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
# 不再全量复制 CraftRoot/bin 下所有 DLL，改为从 EXE/MCP/QML 插件/Qt 插件
# 入口解析 PE imported DLL 依赖，递归复制 CraftRoot 内实际需要的 DLL 闭包。
if (-not $CraftRoot) {
    throw "需要 -CraftRoot 或环境变量 CRAFT_ROOT 指向 KDE Craft 安装，以便随包部署 Kirigami 运行时。"
}
$CraftRootPath = (Resolve-Path $CraftRoot).Path
$CraftBin = Join-Path $CraftRootPath "bin"

# --- PE 依赖闭包解析 ---

function Convert-RvaToOffset {
    param([byte[]]$Bytes, [uint32]$Rva, [int]$SectionOffset, [uint16]$NumSections)
    for ($i = 0; $i -lt $NumSections; $i++) {
        $secOffset = $SectionOffset + $i * 40
        if ($secOffset + 40 -gt $Bytes.Length) { return -1 }
        $virtualSize = [BitConverter]::ToUInt32($Bytes, $secOffset + 8)
        $virtualAddress = [BitConverter]::ToUInt32($Bytes, $secOffset + 12)
        $rawOffset = [BitConverter]::ToUInt32($Bytes, $secOffset + 20)
        if ($Rva -ge $virtualAddress -and $Rva -lt ($virtualAddress + $virtualSize)) {
            return [int]($Rva - $virtualAddress + $rawOffset)
        }
    }
    return -1
}

function Get-PeImports {
    param([string]$Path)
    $imports = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    try {
        $bytes = [System.IO.File]::ReadAllBytes($Path)
        if ($bytes.Length -lt 64) { return $imports }
        $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
        if ($peOffset -lt 0 -or $peOffset -ge $bytes.Length) { return $imports }
        if ([BitConverter]::ToInt32($bytes, $peOffset) -ne 0x00004550) { return $imports }
        $numSections = [BitConverter]::ToUInt16($bytes, $peOffset + 6)
        $optHeaderSize = [BitConverter]::ToUInt16($bytes, $peOffset + 20)
        $optHeaderOffset = $peOffset + 24
        $magic = [BitConverter]::ToUInt16($bytes, $optHeaderOffset)
        if ($magic -eq 0x20B) {
            $dataDirOffset = $optHeaderOffset + 112
        } elseif ($magic -eq 0x10B) {
            $dataDirOffset = $optHeaderOffset + 96
        } else {
            return $imports
        }
        $importDirRva = [BitConverter]::ToUInt32($bytes, $dataDirOffset + 8)
        if ($importDirRva -eq 0) { return $imports }
        $sectionOffset = $optHeaderOffset + $optHeaderSize
        $importFileOffset = Convert-RvaToOffset -Bytes $bytes -Rva $importDirRva -SectionOffset $sectionOffset -NumSections $numSections
        if ($importFileOffset -lt 0) { return $imports }
        $offset = $importFileOffset
        while ($offset -lt $bytes.Length - 20) {
            $nameRva = [BitConverter]::ToUInt32($bytes, $offset + 12)
            $originalFirstThunk = [BitConverter]::ToUInt32($bytes, $offset)
            if ($originalFirstThunk -eq 0 -and $nameRva -eq 0) { break }
            if ($nameRva -ne 0) {
                $nameOffset = Convert-RvaToOffset -Bytes $bytes -Rva $nameRva -SectionOffset $sectionOffset -NumSections $numSections
                if ($nameOffset -ge 0 -and $nameOffset -lt $bytes.Length) {
                    $sb = [System.Text.StringBuilder]::new()
                    $i = $nameOffset
                    while ($i -lt $bytes.Length -and $bytes[$i] -ne 0) {
                        [void]$sb.Append([char]$bytes[$i])
                        $i++
                    }
                    $imports.Add($sb.ToString()) | Out-Null
                }
            }
            $offset += 20
        }
    } catch {
        # Ignore parse errors for non-PE files
    }
    return $imports
}

function Resolve-DllClosure {
    param(
        [string[]]$RootFiles,
        [string]$CraftBin,
        [string]$OutputDir
    )
    $needed = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $queue = [System.Collections.Generic.Queue[string]]::new()

    # DLLs already in OutputDir (from windeployqt and Rust target) are roots too
    $existingDlls = Get-ChildItem -LiteralPath $OutputDir -Filter "*.dll" -ErrorAction SilentlyContinue
    foreach ($dll in $existingDlls) {
        if ($needed.Add($dll.Name)) { $queue.Enqueue($dll.FullName) }
    }

    # Add explicit root files
    foreach ($file in $RootFiles) {
        $queue.Enqueue($file)
    }

    while ($queue.Count -gt 0) {
        $file = $queue.Dequeue()
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { continue }
        $imports = Get-PeImports -Path $file
        foreach ($import in $imports) {
            if ($needed.Contains($import)) { continue }
            $craftDll = Join-Path $CraftBin $import
            if (Test-Path -LiteralPath $craftDll -PathType Leaf) {
                $needed.Add($import) | Out-Null
                $queue.Enqueue($craftDll)
            }
        }
    }

    return $needed
}

# 1. 收集 PE 依赖闭包，只复制 CraftRoot/bin 中实际需要的 DLL
$RootFiles = [System.Collections.Generic.List[string]]::new()
$RootFiles.Add($Exe)
$RootFiles.Add($McpExe)

# KDE QML 插件 DLL 作为显式根集
$CraftQml = Join-Path $CraftRootPath "qml"
if (-not (Test-Path -LiteralPath (Join-Path $CraftQml "org\kde\kirigami\qmldir") -PathType Leaf)) {
    throw "Craft 安装缺少 org.kde.kirigami QML 模块：$CraftQml（请先 craft 安装 kde/frameworks/tier1/kirigami）"
}
$kdeQmlDlls = Get-ChildItem -LiteralPath $CraftQml -Filter "*.dll" -Recurse -ErrorAction SilentlyContinue
foreach ($dll in $kdeQmlDlls) { $RootFiles.Add($dll.FullName) }

# Qt 插件 DLL 作为显式根集（KDE 组件依赖 imageformats/iconengines/styles）
$CraftPlugins = Join-Path $CraftRootPath "plugins"
if (Test-Path -LiteralPath $CraftPlugins -PathType Container) {
    $pluginDlls = Get-ChildItem -LiteralPath $CraftPlugins -Filter "*.dll" -Recurse -ErrorAction SilentlyContinue
    foreach ($dll in $pluginDlls) { $RootFiles.Add($dll.FullName) }
}

$NeededDlls = Resolve-DllClosure -RootFiles $RootFiles -CraftBin $CraftBin -OutputDir $OutputDir

if ($NeededDlls.Count -eq 0) {
    throw "PE 依赖闭包解析结果为空，CraftRoot/bin 可能缺少 DLL：$CraftBin"
}

foreach ($dllName in $NeededDlls) {
    $src = Join-Path $CraftBin $dllName
    Copy-Item -LiteralPath $src -Destination $OutputDir -Force
}
Write-Host "已复制 $($NeededDlls.Count) 个 DLL 从 $CraftBin（PE 依赖闭包）"

# 2. 复制 KDE QML 模块（org/kde/*）
$KdeQmlTarget = Join-Path $OutputDir "qml\org\kde"
New-Item -ItemType Directory -Force -Path $KdeQmlTarget | Out-Null
Copy-Item -Path (Join-Path $CraftQml "org\kde\*") -Destination $KdeQmlTarget -Recurse -Force
foreach ($rel in @(
    "kirigami\qmldir",
    "kirigamiaddons\formcard\qmldir",
    "kirigamiaddons\delegates\qmldir",
    "kirigamiaddons\dateandtime\qmldir",
    "desktop\qmldir",
    "desktop\private\qmldir",
    "qqc2desktopstyle\private\qmldir"
)) {
    if (-not (Test-Path -LiteralPath (Join-Path $KdeQmlTarget $rel) -PathType Leaf)) {
        throw "KDE QML 模块部署不完整：缺少 org/kde/$rel（CraftRoot: $CraftRootPath）"
    }
}

# 3. 复制 Qt 插件（KDE 组件依赖 imageformats/iconengines/styles）
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

# 4. 复制许可证
$LicenseSource = Join-Path $CraftRootPath "share\licenses"
if (Test-Path -LiteralPath $LicenseSource -PathType Container) {
    $LicenseTarget = Join-Path $OutputDir "share\licenses"
    New-Item -ItemType Directory -Force -Path $LicenseTarget | Out-Null
    Copy-Item -Path (Join-Path $LicenseSource "*") -Destination $LicenseTarget -Recurse -Force
}
Write-Host "KDE Kirigami 与 Breeze 图标资源库已随包部署（来源：$CraftRootPath）"

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
