# Windows 原生客户端路线（issue #433）

Windows 不再走 Qt 桌面端路线，也不复用 `apps/Linux_qt`。本目录是唯一 Windows 客户端落点，不另设 Windows 桌面专用新目录。

## 技术路线

- 应用壳：WinUI 3 / Windows App SDK，适合承载普通应用壳和导航页面。
- 正文写作区：自研 `SujianEditor`，不能把 `RichEditBox` / `TextBox` 作为正式编辑器路线。
- 文本布局与渲染：DirectWrite + Direct2D 是 Windows 自研文本布局和渲染主路径；DirectWrite 负责 text layout / glyph / hit test / caret metrics，Direct2D 负责绘制。
- 输入法：Windows IME / TSF composition + commit 链路，通过 `CoreTextEditContext` 自动集成，候选窗口锚点由 `SujianEditor` 光标矩形提供。
- 业务核心：`core/writer_core` 是唯一业务核心。Windows 只做 UI、输入法、文本布局、渲染、动画和平台集成。
- 桥接层：`bindings/windows/` 提供 Rust cdylib → P/Invoke → C# `WriterCoreBridge` 完整链路。

## 目录结构

```
apps/windows/
├── App.xaml / App.xaml.cs          — 应用入口，深色模式支持
├── MainWindow.xaml / .cs           — 主窗口，导航、项目/章节列表
├── Bridge/
│   └── WriterCoreBridge.cs        — P/Invoke 桥接层（writer_core.dll C ABI）
├── Editor/
│   ├── SujianEditor.cs            — 自研写作区核心控件（TSF + 选区 + 剪贴板）
│   ├── SujianEditorHost.cs        — 写作区宿主（IME/InputPane/自动保存）
│   └── Animation/
│       ├── EditorVisualTransaction.cs — 视觉事务 DTO
│       ├── SujianAnimationController.cs — 动画控制器
│       └── SujianAnimationOverlay.cs   — 动画叠加层
├── Pages/
│   ├── SettingsPage.xaml / .cs    — 设置页（字号、行高、缩进、主题）
│   ├── SyncPage.xaml / .cs        — 同步页（远程仓库、自动同步）
│   ├── StatsPage.xaml / .cs       — 写作统计页
│   └── StarmapPage.xaml / .cs     — 星图页
├── SujianWindows.csproj           — 项目文件（WinUI 3 + Win2D）
└── app.manifest                   — 应用清单

bindings/windows/
├── WriterCoreBridge.cs            — P/Invoke 契约参考（与 apps/windows/Bridge/ 同步）
├── build.ps1                      — 编译 writer_core.dll 并复制到 bin/
└── README.md                      — 桥接层文档
```

## 当前实现状态

### 已实现

- [x] WinUI 3 / Windows App SDK 应用壳
- [x] 自研 `SujianEditor` 基础骨架：纯文本行存储、点击定位光标、字符输入、删除、换行、方向键移动、基础滚动
- [x] Win2D (CanvasControl) 文本布局与渲染（基于 DirectWrite/Direct2D）
- [x] 光标绘制与 hit test / caret metrics
- [x] 首行缩进（视觉效果，不写入正文空格）
- [x] 光标闪烁
- [x] 撤销/重做基础栈
- [x] Home/End/PageUp/PageDown 键
- [x] `WriterCoreBridge` P/Invoke 桥接层（调用 writer_core.dll C ABI）
- [x] 打开 workspace、列项目、列卷、列章节、打开章节、保存章节端到端闭环
- [x] 设置页（字号、行高、首行缩进、自动保存、主题）
- [x] 深色模式支持（WinUI 3 主题切换）
- [x] `SujianEditorHost` 宿主控件（IME InputPane 协调、自动保存）
- [x] TSF 自动集成（`CoreTextEditContext`：composition/commit/候选窗口锚点自动跟随）
- [x] 鼠标拖选 + Shift+方向键选择 + Ctrl+A 全选
- [x] 选区可视化渲染（半透明蓝色高亮）
- [x] 复制 (Ctrl+C) / 剪切 (Ctrl+X) / 粘贴 (Ctrl+V)
- [x] 创建项目/卷/章节（通过 WriterCoreBridge P/Invoke）
- [x] 正文动画骨架（`SujianAnimationController` + `SujianAnimationOverlay` + `EditorVisualTransaction` DTO）
- [x] 同步页（远程 URL、访问令牌、自动同步、试运行、立即同步）
- [x] 写作统计页（总字数、今日字数、本次字数、连续天数）
- [x] 星图页（列表、新建、刷新）
- [x] Windows CI 工作流（`.github/workflows/windows_build.yml`）

### 待实现 / 待提交验收

- [ ] 动画与编辑器集成（当前骨架就位，需接入 Core `editor_visual_transaction` 实际数据流）
- [ ] 星图可视化画布（Canvas 交互式节点/边渲染）
- [ ] 滚动优化（虚拟行、viewport 裁剪）
- [ ] Windows 打包（MSIX / exe 安装器）
- [ ] 真机验证：中文 IME composition/commit、候选窗口位置、DPI 缩放

## 第一阶段最小闭环

Windows issue #433 的执行顺序固定为：**先做自研 `SujianEditor` MVP，再补设置页、统计页、同步页等完整页面**。MVP 必须先证明：

1. WinUI 3 应用能启动，`apps/windows` 是唯一 Windows 目录，不另设 Windows 桌面专用新目录。
2. 自研 `SujianEditor` 可以显示纯文本、点击定位光标、输入、删除、换行、方向键移动、基础滚动。
3. 微软拼音 IME composition/commit 正常，候选窗口锚点跟随 `SujianEditor` 光标矩形。
4. DirectWrite/Direct2D 渲染在常见 DPI 与字体设置下稳定，hit test 与 caret metrics 可用。
5. `WriterCoreBridge` 只负责 UI 到 `writer_core` 的稳定桥接：打开 workspace、列项目、列卷、列章节、打开章节、保存章节。
6. 普通导航/UI 可以使用 WinUI 控件；正文写作区不得回退到 `RichEditBox` / `TextBox`。

## 平台边界

三端共享 `writer_core`、DTO、设置 key、同步协议、统计格式和动画事务语义；不共享 UI、输入法、光标、打包、标题栏、平台渲染实现。

Windows 旧 Qt 打包、安装器、workflow、runtime profile、DWM/IME/pending key 兼容入口已废弃；如发现引用，应删除或改为本目录原生路线。
