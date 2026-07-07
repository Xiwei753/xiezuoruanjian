# Windows/Linux 客户端分离契约

Status: active  
Last verified: 2026-07-08  
Truth source: issue #433 / code / product decision

## 核心原则

Windows 和 Linux 是两个独立客户端，不再追求同一份 UI/编辑器源码。两端只共享业务核心，不共享高风险平台 UI 和编辑器实现。

## 共享

| 共享项 | 位置 | 说明 |
|---|---|---|
| Rust `writer_core` | `core/writer_core/` | 唯一业务核心 |
| 数据格式 | `docs/workspace_format.md` | 工作区目录结构是唯一事实来源 |
| DTO 契约 | `core/writer_core/src/api/` | 跨平台稳定 API 层 |
| 同步协议 | `core/writer_core/src/sync/` | 同步逻辑在 core |
| 设置 key | `core/writer_core/src/settings/` | 设置 schema 统一 |
| 统计事件格式 | `core/writer_core/src/writing_stats/` | 统计在 core |
| 星图数据结构 | `core/writer_core/src/starmap/` | 星图在 core |
| 编辑器动画事务定义 | `core/writer_core/src/editor/` | `EditorVisualTransaction` 语义统一 |
| 视觉规范文档 | `docs/` | 不含平台实现细节 |
| 图标和品牌资源 | `assets/` | 共享资源 |

## 不强制共享

| 项目 | Windows | Linux |
|---|---|---|
| UI 页面源码 | WinUI 3 XAML / C# | Qt/QML |
| 编辑器实现 | `SujianEditor` (Win2D/DirectWrite) | `SujianEditorItem` (QTextLayout/QImage) |
| 输入法处理 | Windows IME/TSF | fcitx5/ibus |
| 光标处理 | Win2D caret metrics | QTextLine cursorToX/xToCursor |
| 深色模式实现 | WinUI 3 RequestedTheme | Qt/KDE 主题 |
| 打包脚本 | MSIX / exe | AppImage |
| 平台日志路径 | `%LOCALAPPDATA%` | `~/.local/share` |
| 文件选择器 | WinUI FilePicker | Qt FileDialog |
| 平台快捷键细节 | Ctrl+Z/Y | Ctrl+Z/Shift+Ctrl+Z |

## 必须分离

| 项目 | 原因 |
|---|---|
| Windows IME 与 Linux fcitx5/ibus | 完全不同的输入法框架 |
| Windows DPI 与 Linux Wayland/X11 缩放 | 完全不同的缩放机制 |
| Windows 标题栏与 Linux 窗口装饰 | 完全不同的窗口管理 |
| Windows 安装包与 Linux AppImage | 完全不同的打包格式 |
| Windows 自研编辑器与 Linux Qt 编辑器 | 完全不同的渲染和布局引擎 |

## 维护规则

- 修 Windows 输入法，只改 `apps/windows/`
- 修 Linux 输入法，只改 `apps/Linux_qt/`
- 修 Windows 标题栏、DPI、安装包，只改 `apps/windows/`
- 修 Linux AppImage、Wayland、fcitx5，只改 `apps/Linux_qt/`
- 新增普通功能时，两边按功能清单同步实现，不要求源码一致
- 修改 core API 时，必须同时确认 Android、Windows、Linux bridge 的契约
- PR 必须写清影响平台：core / android / windows / linux / docs

## 迁移来源

当前 `apps/Linux_qt` 已是独立 Linux 客户端。旧 `apps/desktop`（如果存在）是迁移来源，不再承载新路线。Windows 客户端在 `apps/windows/` 从零建设，不复用 Qt 代码。

## Bridge 对比

| Bridge | Windows | Linux | Android |
|---|---|---|---|
| 技术栈 | P/Invoke (C ABI cdylib) | Rust cxx/Qt binding | UniFFI (Kotlin) |
| 位置 | `bindings/windows/` + `apps/windows/Bridge/` | `apps/Linux_qt/src/` | `bindings/android/` |
| 数据传输 | JSON C string + Marshal | Rust 直接调用 | typed DTO |
| 初始化 | `writer_core_init` | `WriterCore::new` | `WriterAppService` 构造 |
