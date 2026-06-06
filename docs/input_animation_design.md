# 输入动效设计方案 (Input Animation Design)

本文件已切换到新的底层路线：文字吐字动画不再围绕 QML `TextArea` / Android `EditText` 打补丁。权威路线见 [自绘编辑器与统一事件层路线](editor_engine_route.md)。

## 当前决策

- Linux 文字吐字动画立即停用。
- 废弃 `TextArea` 显示真实文字、QML 猜 diff、`DocumentHandler` 改 `QTextDocument` 字符格式隐藏文字、QML overlay reveal 的链路。
- Android 过渡期停用 `ForegroundColorSpan(Color.TRANSPARENT)` 隐藏正文文字的 `EditText` 吐字动画链路。
- 保留普通编辑、纯文本保存、稳定滚轮和光标显示。
- 新动画只能基于 Core `EditorTransaction` / `EditorAnimationEvent`，由平台自绘 renderer 消费。

## 新结构

- Rust Core：统一决定文本怎么变、选区怎么变、动画事件是什么。
- Desktop：`SujianEditorItem` 使用 Qt `QTextLayout` / `QPainter` 或后续 scene graph 绘制。
- Android：`SujianEditorView : View` 在 `onDraw(Canvas)` 里绘制，`InputConnection` 接输入法。
- Android 当前 `WriterEditText` 仅保留事件占位，不允许再把真实正文变透明后由 overlay reveal。

## 不再允许

- 不允许恢复 hidden-range/reveal。
- 不允许通过 `QTextDocument` 字符格式把真实文字变透明。
- 不允许通过 Android `ForegroundColorSpan(Color.TRANSPARENT)` 把真实文字变透明。
- 不允许 QML/Kotlin 自行维护分叉 diff 语义。
- 不允许为了动画修改正文内容或保存 HTML。
