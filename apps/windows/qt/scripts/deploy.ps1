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

$LinuxQml = Join-Path $RepoRoot "apps\Linux_qt\qml"
$CommonArgs = @("--dir", $OutputDir, "--compiler-runtime")
if ($Configuration -eq "release") { $CommonArgs += "--release" } else { $CommonArgs += "--debug" }

& $Windeployqt @CommonArgs "--qmldir" $LinuxQml $Exe
if ($LASTEXITCODE -ne 0) { throw "windeployqt (共享 QML) 失败" }

Copy-Item $Exe $OutputDir -Force

Get-ChildItem $TargetDir -Filter "*.dll" | ForEach-Object {
    Copy-Item $_.FullName $OutputDir -Force
}

Copy-Item $McpExe $OutputDir -Force

# 便携版需要应用本地的 MSVC 运行库。
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
# 运行时由 KDE Craft 构建。windeployqt 不收集非 Qt 的 org.kde.* 模块，
# 必须从 CraftRoot 随包复制，并通过 qt.conf 的 QmlImports = qml 定位。
#
# 使用 dumpbin /DEPENDENTS 递归解析 PE 依赖闭包，只复制实际需要的 DLL。
# KDE QML 以 main.qml 的实际 import 模块为入口，不扫描整个 Craft QML 树。
if (-not $CraftRoot) {
    throw "需要 -CraftRoot 或环境变量 CRAFT_ROOT 指向 KDE Craft 安装，以便随包部署 Kirigami 运行时。"
}
$CraftRootPath = (Resolve-Path $CraftRoot).Path
$CraftBin = Join-Path $CraftRootPath "bin"
$CraftQml = Join-Path $CraftRootPath "qml"
$CraftPlugins = Join-Path $CraftRootPath "plugins"

# --- dumpbin /DEPENDENTS 依赖闭包解析 ---

function Get-Dumpbin {
    $cmd = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $searchRoots = [System.Collections.Generic.List[string]]::new()
    if ($env:VCINSTALLDIR) {
        $searchRoots.Add((Join-Path $env:VCINSTALLDIR "Tools\MSVC"))
    }
    $ProgramFilesX86 = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFilesX86)
    $VsWhere = Join-Path $ProgramFilesX86 "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $VsWhere) {
        $VsInstall = (& $VsWhere -latest -products "*" -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
        if ($VsInstall) {
            $searchRoots.Add((Join-Path $VsInstall "VC\Tools\MSVC"))
        }
    }
    foreach ($root in $searchRoots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        $latest = Get-ChildItem -LiteralPath $root -Directory | Sort-Object Name -Descending | Select-Object -First 1
        if ($latest) {
            $dumpbin = Join-Path $latest.FullName "bin\Hostx64\x64\dumpbin.exe"
            if (Test-Path -LiteralPath $dumpbin -PathType Leaf) { return $dumpbin }
        }
    }
    return $null
}

function Get-PeDependencies {
    param([string]$FilePath, [string]$Dumpbin, [switch]$IsCritical)

    $output = & $Dumpbin /DEPENDENTS $FilePath 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "dumpbin /DEPENDENTS 失败（退出码 $LASTEXITCODE）：$FilePath"
    }

    $deps = @()
    $inSection = $false
    foreach ($line in ($output -split "`r?`n")) {
        if ($line -match "^\s*Image has the following (?:delay load )?dependencies:\s*$") {
            $inSection = $true
            continue
        }
        if ($inSection) {
            if ($line -match "^\s*Summary\s*$") { break }
            if ($line -match "^\s*(\S+\.dll)\s*$") {
                $deps += $matches[1]
            }
        }
    }
    if ($deps.Count -eq 0 -and $IsCritical) {
        throw "dumpbin /DEPENDENTS 解析出零个依赖（核心入口应有导入表）：$FilePath"
    }
    return $deps
}

function Resolve-DllClosure {
    param(
        [string[]]$RootFiles,
        [string]$CraftBin,
        [string]$OutputDir,
        [string]$Dumpbin
    )

    $scanned = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $craftToCopy = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $queue = [System.Collections.Generic.Queue[string]]::new()

    $existingDlls = Get-ChildItem -LiteralPath $OutputDir -Filter "*.dll" -ErrorAction SilentlyContinue
    foreach ($dll in $existingDlls) {
        if ($scanned.Add($dll.Name)) { $queue.Enqueue($dll.FullName) }
    }

    $rootSet = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($file in $RootFiles) {
        $rootSet.Add($file) | Out-Null
        $queue.Enqueue($file)
    }

    while ($queue.Count -gt 0) {
        $file = $queue.Dequeue()
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { continue }

        $isCritical = $rootSet.Contains($file)
        $deps = Get-PeDependencies -FilePath $file -Dumpbin $Dumpbin -IsCritical:$isCritical
        foreach ($dep in $deps) {
            if ($scanned.Contains($dep)) { continue }
            $scanned.Add($dep) | Out-Null
            $craftDll = Join-Path $CraftBin $dep
            $outputDll = Join-Path $OutputDir $dep
            if ((Test-Path -LiteralPath $craftDll -PathType Leaf) -and
                -not (Test-Path -LiteralPath $outputDll -PathType Leaf)) {
                $craftToCopy.Add($dep) | Out-Null
                $queue.Enqueue($craftDll)
            } elseif (Test-Path -LiteralPath $craftDll -PathType Leaf) {
                $queue.Enqueue($craftDll)
            }
        }
    }

    return $craftToCopy
}

$Dumpbin = Get-Dumpbin
if (-not $Dumpbin) {
    throw "找不到 dumpbin.exe，请确保 Visual Studio 环境已初始化（运行 vcvars64.bat 或在 VS Developer PowerShell 中执行）"
}
Write-Host "使用 dumpbin：$Dumpbin"

# 1. 确定根文件集
$RootFiles = [System.Collections.Generic.List[string]]::new()
$RootFiles.Add($Exe)
$RootFiles.Add($McpExe)

# KDE QML 模块入口：只收集实际 import 的模块目录下的 DLL
# 从 main.qml 及其他 QML 文件的 import 语句确定的模块列表：
#   org.kde.kirigami, org.kde.kirigamiaddons.formcard,
#   org.kde.kirigamiaddons.delegates, org.kde.kirigamiaddons.dateandtime,
#   org.kde.desktop, org.kde.qqc2desktopstyle
if (-not (Test-Path -LiteralPath (Join-Path $CraftQml "org\kde\kirigami\qmldir") -PathType Leaf)) {
    throw "Craft 安装缺少 org.kde.kirigami QML 模块：$CraftQml（请先 craft 安装 kde/frameworks/tier1/kirigami）"
}
$KdeQmlModules = @(
    "org\kde\kirigami",
    "org\kde\kirigamiaddons\formcard",
    "org\kde\kirigamiaddons\delegates",
    "org\kde\kirigamiaddons\dateandtime",
    "org\kde\desktop",
    "org\kde\qqc2desktopstyle"
)
foreach ($mod in $KdeQmlModules) {
    $modDir = Join-Path $CraftQml $mod
    if (Test-Path -LiteralPath $modDir -PathType Container) {
        $modDlls = Get-ChildItem -LiteralPath $modDir -Filter "*.dll" -Recurse -ErrorAction SilentlyContinue
        foreach ($dll in $modDlls) { $RootFiles.Add($dll.FullName) }
    } else {
        Write-Host "警告：KDE QML 模块目录不存在：$modDir"
    }
}

# Qt 插件入口：只收集实际需要的插件目录下的 DLL
$NeededPluginDirs = @("imageformats", "iconengines", "styles")
if (Test-Path -LiteralPath $CraftPlugins -PathType Container) {
    foreach ($pluginDir in $NeededPluginDirs) {
        $srcDir = Join-Path $CraftPlugins $pluginDir
        if (Test-Path -LiteralPath $srcDir -PathType Container) {
            $pluginDlls = Get-ChildItem -LiteralPath $srcDir -Filter "*.dll" -ErrorAction SilentlyContinue
            foreach ($dll in $pluginDlls) { $RootFiles.Add($dll.FullName) }
        }
    }
}

# 2. 解析 PE 依赖闭包
$CraftDllsToCopy = Resolve-DllClosure -RootFiles $RootFiles -CraftBin $CraftBin -OutputDir $OutputDir -Dumpbin $Dumpbin

foreach ($dllName in $CraftDllsToCopy) {
    $src = Join-Path $CraftBin $dllName
    if (-not (Test-Path -LiteralPath $src -PathType Leaf)) {
        throw "Craft DLL 消失（解析时存在，复制时缺失）：$src"
    }
    Copy-Item -LiteralPath $src -Destination $OutputDir -Force
}
Write-Host "已复制 $($CraftDllsToCopy.Count) 个 DLL 从 $CraftBin（dumpbin PE 依赖闭包）"

# 3. 复制 KDE QML 模块（只复制实际需要的模块）
$KdeQmlTarget = Join-Path $OutputDir "qml"
New-Item -ItemType Directory -Force -Path $KdeQmlTarget | Out-Null
foreach ($mod in $KdeQmlModules) {
    $srcMod = Join-Path $CraftQml $mod
    $dstMod = Join-Path $KdeQmlTarget $mod
    if (Test-Path -LiteralPath $srcMod -PathType Container) {
        New-Item -ItemType Directory -Force -Path $dstMod | Out-Null
        Copy-Item -Path (Join-Path $srcMod "*") -Destination $dstMod -Recurse -Force
    }
}
$KdeChecksRoot = Join-Path $OutputDir "qml\org\kde"
foreach ($rel in @(
    "kirigami\qmldir",
    "kirigamiaddons\formcard\qmldir",
    "kirigamiaddons\delegates\qmldir",
    "kirigamiaddons\dateandtime\qmldir",
    "desktop\qmldir",
    "desktop\private\qmldir",
    "qqc2desktopstyle\private\qmldir"
)) {
    if (-not (Test-Path -LiteralPath (Join-Path $KdeChecksRoot $rel) -PathType Leaf)) {
        throw "KDE QML 模块部署不完整：缺少 org/kde/$rel（CraftRoot: $CraftRootPath）"
    }
}

# 4. 复制 Qt 插件（只复制实际需要的插件目录）
if (Test-Path -LiteralPath $CraftPlugins -PathType Container) {
    foreach ($pluginDir in $NeededPluginDirs) {
        $srcDir = Join-Path $CraftPlugins $pluginDir
        if (Test-Path -LiteralPath $srcDir -PathType Container) {
            $dstDir = Join-Path $OutputDir $pluginDir
            New-Item -ItemType Directory -Force -Path $dstDir | Out-Null
            Copy-Item -Path (Join-Path $srcDir "*.dll") -Destination $dstDir -Force
        }
    }
}

# 5. 复制许可证
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

$QtConfig = Join-Path $AppDir "qt.conf"
if (-not (Test-Path -LiteralPath $QtConfig -PathType Leaf)) {
    throw "缺少 Windows Qt 便携包的 qt.conf：$QtConfig"
}
Copy-Item -LiteralPath $QtConfig -Destination (Join-Path $OutputDir "qt.conf") -Force

Write-Host "Windows Qt 发布目录：$OutputDir"
