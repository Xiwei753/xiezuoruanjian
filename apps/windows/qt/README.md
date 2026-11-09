# Windows Qt 客户端

`apps/windows/qt` 是 Windows 的正式桌面入口：Rust + Qt6/QML，复用
`apps/Linux_qt` 中平台无关的 QML、编辑器、星图和后端实现。

## 目录

- `src/windows_main.rs`：Windows 启动、QML 资源注册、平台服务、诊断与退出。
- `src/platform/linux_qt/`：保持共享源码使用的模块名，提供 Windows 桌面设置、
  屏幕几何、UTF-16 转换、剪贴板适配与运行环境。
- `src/editor/input/platform/windows/input_surface.rs`：Qt Windows 插件下的
  TSF/IME `PlatformImeAdapter`；preedit/commit/replacement 仍走共享
  `platform_ime.rs` 写入 Rust EditorKernel。
- `src/native/windows_appearance.cpp`：读取 Windows 深浅色设置并应用 Qt 调色板。
- `src/native/windows_render_host.rs`：Windows Qt Quick Scene Graph/RHI 宿主。
- `src/agent_runtime/`：Windows 专属工作目录与 MCP 启动；其余 Agent 运行时复用
  共享实现。
- `qml/org/kde/`：Qt6/QML 下的 Kirigami / FormCard / delegates / DatePopup
  兼容模块，让 Linux 通用 QML 无需复制即可运行。
- `build.rs`：生成 QRC 清单、探测 Windows Qt6、校验 `.qsb` 的 HLSL 变体并
  编译 Windows 平台 C++。构建时还会把 `apps/Linux_qt/src` 镜像到
  `src/shared/`（已忽略），并用 `src/windows_overrides/` 覆盖平台差异文件。
- `scripts/build.ps1`、`scripts/deploy.ps1`：MSVC 构建与 `windeployqt` 部署。

## 构建

前置：Windows 10/11 x64、MSVC、Rust `x86_64-pc-windows-msvc`、Qt 6.7+
MSVC x64（`qmake.exe` 在 `PATH`，或设置 `QMAKE` / `QT_ROOT`）。

```powershell
pwsh -File apps/windows/qt/scripts/build.ps1 -Configuration release -QtRoot C:\Qt\6.8.0\msvc2022_64
```

产物在 `target/release/sujian-windows-qt.exe`，发布目录为
`apps/windows/qt/dist/release/`。

## 边界

- 正文唯一事实来源仍是 Rust Core；Qt 只维护排版显示镜像与输入事件。
- Windows 专属窗口、输入法、渲染和部署逻辑只放在本目录。
- 平台无关的 QML 与 Rust 实现继续留在 `apps/Linux_qt` 的唯一共享副本中，
  不在 Windows 目录复制分叉。
