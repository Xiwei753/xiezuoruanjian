# Windows 原生客户端路线

Status: active  
Last verified: 2026-07-08  
Truth source: issue #433 / code / product decision  
Supersedes: 旧 Qt 桌面端 Windows 路线

## 核心结论

Windows 端是独立原生客户端，不复用 Linux Qt/QML 客户端。两端只共享 Rust `writer_core`、DTO 契约、资源规范和功能清单，不共享 UI 和编辑器实现。

## 技术路线

| 层 | 技术 | 说明 |
|---|---|---|
| 应用壳 | WinUI 3 / Windows App SDK | 主窗口、标题栏、导航、普通控件 |
| 正文写作区 | 自研 `SujianEditor` | 不使用 RichEditBox/TextBox |
| 文本布局/渲染 | DirectWrite + Direct2D（通过 Win2D） | 布局、hit test、caret metrics、绘制 |
| 输入法 | Windows IME / TSF | composition + commit + 候选窗口锚点 |
| 业务核心 | Rust `writer_core` | 唯一业务底层 |
| 桥接 | C ABI cdylib + P/Invoke | `writer_core.dll` → C# `WriterCoreBridge` |
| 动画 | 待定（MVP 后评估） | 基于 Core `EditorVisualTransaction` |

## 目录规划

- `apps/windows/`：Windows 原生客户端唯一落点
- `bindings/windows/`：Rust cdylib 构建、P/Invoke 契约、构建脚本
- `core/writer_core/`：共享业务核心（`--features harmony-ffi` 导出 C ABI）

## SujianEditor 内部模块

| 模块 | 职责 | 当前状态 |
|---|---|---|
| `SujianEditor` | 编辑器核心控件：行存储、光标、输入、删除、换行、滚动、绘制 | 已实现基础版 |
| `SujianEditorHost` | 宿主控件：IME InputPane 协调、ScrollViewer | 已实现基础版 |
| TSF 集成 | 自动 composition/commit、候选窗口锚点 | 待实现 |
| `SujianTextLayout` | 段落/行/cluster 布局、换行、首行缩进 | 部分实现（Win2D CanvasTextLayout） |
| `SujianTextRenderer` | 正文、光标、选区、预编辑文本绘制 | 部分实现 |
| `SujianAnimationController` | 吐字、吞字、光标移动、重排动画 | 待实现 |

## 阶段执行顺序

1. ✅ 建 Windows 原生工程骨架
2. ✅ 建 Rust core Windows bridge（P/Invoke）
3. ✅ 建 `SujianEditor` 自研控件骨架
4. 🔄 实现自研编辑器最小输入闭环
5. ⬜ 验证中文输入和候选窗口位置
6. ✅ 接入章节打开/保存
7. 🔄 补布局、首行缩进、字号、滚动
8. ⬜ 补撤销、选区、剪贴板
9. ⬜ 接入正文动画
10. ⬜ 补同步、日志、设置、统计等完整页面
11. ⬜ 补 Windows 打包和 CI

## 第一阶段最小闭环验收标准

- 正文控件不是 RichEditBox/TextBox ✅
- 英文输入正常 ✅
- 中文输入能完成组合、候选、提交（基础 API 已就位，TSF 自动集成待完成）
- 光标在空行、非空行、行首、行尾基本准确 ✅
- 保存后重新打开内容一致 ✅（通过 WriterCoreBridge P/Invoke）
- 光标矩形可以通过 debug 日志输出 ✅（GetCursorRect / GetCursorRectForIME）
- 首行缩进是视觉效果，不污染正文 ✅

## 禁止路线

- 禁止继续把 Windows 正式客户端建立在 Qt 上
- 禁止继续让 Windows 和 Linux 共用同一个高风险编辑器实现
- 禁止先用 RichEditBox/TextBox 做正式写作区
- 禁止把正文动画做成依赖原生控件内部布局的猜测方案
- 禁止只实现英文输入后宣称写作区完成
- 禁止跳过中文输入法和候选窗口定位验收
- 禁止把首行缩进写成正文空格
- 禁止把 writer_core 业务逻辑复制到 Windows UI 层
- 禁止为了 Windows 修复去修改 Linux 输入法逻辑
