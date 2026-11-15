# Windows Qt 客户端

`apps/windows/qt` 是 Windows 的正式桌面入口：Rust + Qt6/QML，与
`apps/Linux_qt` 共用同一份 QML（含 Kirigami.ApplicationWindow 壳层
`main.qml`、KDE 设置页 `KdeSettingsPage.qml` 与 `SettingsDialog.qml`）、
编辑器、星图和后端实现；org.kde.* QML 模块由随包部署的真实 KDE
Kirigami/KirigamiAddons 运行时提供，Windows 不再保留手写替身组件。

## 目录

- `src/windows_main.rs`：Windows 启动、QML 资源注册、平台服务、诊断与退出；
  强制 Qt Quick Controls 使用 `org.kde.desktop` 样式（与 Linux KDE/Plasma
  一致）并初始化随包 Breeze 图标主题。
- `src/platform/linux_qt/`：保持共享源码使用的模块名，提供 Windows 桌面设置、
  屏幕几何、UTF-16 转换、剪贴板适配与运行环境。
- `src/editor/input/platform/windows/input_surface.rs`：Qt Windows 插件下的
  TSF/IME `PlatformImeAdapter`；preedit/commit/replacement 仍走共享
  `platform_ime.rs` 写入 Rust EditorKernel。
- `src/windows_overrides/windows_appearance.cpp`：深浅色只切换 Qt 颜色方案
  （配色由 Qt/Breeze/Kirigami 主题链决定，不写 Windows 独立调色板 token），
  并为便携包注册 `share/icons` 下的 Breeze 图标主题搜索路径。
- `src/windows_overrides/native/windows_render_backend.rs`：读取真实
  `QQuickWindow` RHI 后端并核对 D3D11/HLSL 着色器；实际绘制复用共享 QSG 图层。
- `src/agent_runtime/`：Windows 专属工作目录与 MCP 启动；其余 Agent 运行时复用
  共享实现。
- `build.rs`：生成 QRC 清单（Linux 通用 QML 顶层文件 + 完整共享 `main.qml`）、
  探测 Windows Qt6（>= 6.11）、校验 `.qsb` 的 HLSL 变体并编译 Windows 平台
  C++。构建时还会把 `apps/Linux_qt/src` 镜像到 `src/shared/`（已忽略），并用
  `src/windows_overrides/` 覆盖平台差异文件。
- `scripts/build.ps1`、`scripts/deploy.ps1`：MSVC 构建与 `windeployqt` +
  KDE Craft 运行时部署。

## 运行时依赖与打包来源

| 组件 | 用途 | 来源 |
| --- | --- | --- |
| Qt 6.11+（Core/Gui/Widgets/Qml/Quick/QuickControls2/ShaderTools/Tools） | 构建、QSB 着色器、lrelease | KDE Craft `qt6/qtshadertools`、`qt6/qttools`（Qt 与 KDE 组件同一 ABI 来源） |
| org.kde.kirigami | 共享 QML 的 Kirigami.ApplicationWindow、Icon、Heading、Theme、Units 等 | Craft `kde/kirigami` |
| org.kde.kirigamiaddons（formcard/delegates/dateandtime） | 共享设置页的 FormCard、delegates、DatePopup | Craft `kde/kirigami-addons` |
| org.kde.desktop QQC2 样式 | 全部 Qt Quick Controls 的桌面控件外观 | Craft `kde/qqc2-desktop-style` |
| KIconThemes 及 KF6 依赖库 | Kirigami.Icon 图标加载 | Craft `kde/kiconthemes`（依赖库随 blueprint 一并安装） |
| Breeze 图标主题 | 共享 QML 的命名图标 | Craft `kde/breeze` |
| VC++ 14x 运行库 | CRT 依赖 | Visual Studio 官方 Redist `Microsoft.VC143.CRT` |
| MacBERT4CSC FP32 模型、ONNX Runtime 1.22.0 Windows x64 | 中文纠错 | `crates/sujian-proof/tools/fetch_model.py` + 同一份 `model_manifest.json` |

`deploy.ps1` 把 CraftRoot 的 `qml/org/kde/*`（含各模块 qmldir 与 QML 插件
DLL）、`qml/QtQuick/Controls/org.kde.desktop`、`bin/lib*.dll`（KF6 运行库）、
`share/icons/breeze*` 与许可文件复制进发布目录；缺任一必需模块直接报错。

## 构建

前置：Windows 10/11 x64、MSVC、Rust `x86_64-pc-windows-msvc`、Python 3，
以及 KDE Craft 安装的 Qt 6.11+ MSVC x64 与上述 KDE 组件（qmake.exe 在
`PATH`，或设置 `QMAKE` / `QT_ROOT`）。

```powershell
pwsh -File apps/windows/qt/scripts/build.ps1 -Configuration release -QtRoot <CraftRoot>
```

产物在 `target/release/sujian-windows-qt.exe`，发布目录为
`apps/windows/qt/dist/release/`。

发布目录内包含 Qt/KDE QML 插件、`sujian_mcp.exe`、VC++ 运行库、
`models/macbert4csc/`（474 MB FP32 模型及 tokenizer）、
`onnxruntime.dll` / `onnxruntime_providers_shared.dll`、Breeze 图标，以及用于
便携部署的 `qt.conf`（`Prefix = .`、`Plugins = .`、`QmlImports = qml`）。
Windows 桌面端构建启用 `sujian-windows-qt/ai`；启动时从可执行文件所在目录
设置内置模型、运行库与图标主题路径，用户设置的外部模型路径仍优先。下载和
校验逻辑复用 `crates/sujian-proof/tools/fetch_model.py` 与同一份
`model_manifest.json`。

## 边界

- 正文唯一事实来源仍是 Rust Core；Qt 只维护排版显示镜像与输入事件。
- Windows 专属窗口、输入法、渲染、图标主题定位和部署逻辑只放在本目录；
  页面布局、控件层次、设置 UI、图标和主题由同一份共享 QML/KDE 实现负责。
- 平台无关的 QML 与 Rust 实现继续留在 `apps/Linux_qt` 的唯一共享副本中，
  不在 Windows 目录复制分叉。
