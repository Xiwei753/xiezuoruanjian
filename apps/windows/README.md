# Windows 原生客户端路线（issue #433）

Windows 不再走 Qt 桌面端路线，也不复用 `apps/Linux_qt`。本目录是唯一 Windows 客户端落点，不另设 Windows 桌面专用新目录。

## 技术路线

- 应用壳：WinUI 3 / Windows App SDK，适合承载普通应用壳和导航页面。
- 正文写作区：自研 `SujianEditor`，不能把 `RichEditBox` / `TextBox` 作为正式编辑器路线。
- 文本布局与渲染：DirectWrite + Direct2D 是 Windows 自研文本布局和渲染主路径；DirectWrite 负责 text layout / glyph / hit test / caret metrics，Direct2D 负责绘制。
- 输入法：Windows IME / TSF composition + commit 链路，候选窗口锚点由 `SujianEditor` 光标矩形提供。
- 业务核心：`core/writer_core` 是唯一业务核心。Windows 只做 UI、输入法、文本布局、渲染、动画和平台集成。

## 风险前置

WinUI 3 / Windows App SDK 与 DirectWrite/Direct2D 自定义文本渲染可成立，但社区经验表明细节多且常需 workaround。Windows 端不得先堆完整页面；必须先证明自研写作区 MVP 稳定，尤其是中文 IME、composition、commit、候选窗口锚点、光标矩形、DPI 和字体渲染。

## 当前实现状态

本 README 记录 issue #433 的目标路线和验收口径；在对应工程文件、编辑器实现和桥接代码正式提交前，不把目标能力写成“已完成”。

### 待实现 / 待提交验收
- WinUI 3 / Windows App SDK 应用壳。
- 自研 `SujianEditor` 基础骨架：纯文本行存储、点击定位光标、字符输入、删除、换行、方向键移动、基础滚动。
- DirectWrite/Direct2D 文本布局与渲染。
- 光标绘制与 hit test / caret metrics。
- Windows IME / TSF composition/commit 完整链路，候选窗口锚点跟随 `SujianEditor` 光标矩形。
- `WriterCoreBridge` 绑定真实 writer_core（通过 C ABI / UniFFI）。
- 打开 workspace、列项目、列卷、列章节、打开章节、保存章节端到端闭环。
- 文字动画（Core `EditorVisualTransaction` → SujianEditor overlay）：暂不实现动画，不参与当前动画主路径。Windows `SujianEditor` 不消费 `editor_visual_transaction` 契约，不创建 hidden range，不创建 overlay 动画。待自研写作区 MVP 稳定后再评估是否接入同一动画契约。
- 滚动优化（虚拟行、viewport 裁剪）。

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
