# 自绘编辑器与统一事件层路线

本文档记录新的编辑器底层路线。当前优先级以用户最新指令为准：停止围绕 QML `TextArea` 做文字动画补丁，逐步迁移到 Core 统一编辑事件层 + 平台自绘 renderer。

## 结论

- 最终路线不是在 `TextArea` / `EditText` 上叠补丁，而是自研编辑器。
- Rust Core 统一描述文本状态变化、选区变化、撤销重做语义和动画事件。
- Desktop 使用 Qt/QML 原生组件承载自绘编辑器，底层优先 `QQuickPaintedItem` / `QQuickItem` + `QTextLayout` + `QPainter`。
- Android 使用 Kotlin `View` 自绘，`onDraw(Canvas)` 画正文、选区、光标和动画；`InputConnection` 接输入法。
- 平台层只负责输入适配、字体测量、换行、滚动、命中测试和绘制，不再自己猜 diff。

## 分阶段计划

### 第一阶段：停掉 Linux 高危链

- 停用 `TextArea` 文字吐字动画。
- 禁止通过 `DocumentHandler` 修改 `QTextDocument` 字符格式来隐藏/恢复文字。
- 保留稳定编辑、保存、滚轮和光标显示。
- 设置项可继续存在，但 Linux typing animation 在新 renderer 落地前不生效。

### 第二阶段：统一编辑事件层

- 在 `core/writer_core/src/editor` 下维护平台无关编辑事务模型。
- `EditorTransaction` 记录旧文本、新文本、变更列表、旧选区、新选区、原因和是否应该播放动画。
- `EditorAnimationEvent` 记录 renderer 可消费的插入、删除、光标事件。
- Desktop 和 Android 后续都必须消费 Core transaction，不得在 QML/Kotlin 里各自猜 diff。

### 第三阶段：Desktop SujianEditorItem

- 新增 `SujianEditorItem : QQuickItem` 或先用 `QQuickPaintedItem` 验证。
- 内部维护文本、选区、光标、滚动、布局缓存和动画列表。
- 使用 `QTextLayout` 负责 Unicode 文本布局、换行、光标位置和绘制辅助。
- QML `WritingWorkspace` 最终只放 `SujianEditorItem`，不再把 `TextArea` 作为主显示层。

### 第四阶段：Android SujianEditorView

- 从当前 `WriterEditText` + overlay 逐步迁移到 `SujianEditorView : View`。
- `onDraw(Canvas)` 画正文、选区、光标和动画。
- `onCreateInputConnection()` 返回自定义输入连接，处理 `commitText`、`setComposingText`、`deleteSurroundingText`、`setSelection`。
- Android 渲染层只消费 Core transaction 和平台 text layout 结果。

## 当前代码落点

- `core/writer_core/src/editor/transaction.rs`：统一编辑事务和动画事件骨架。
- `apps/desktop/qml/EditorTypingAnimator.qml`：保留为兼容占位，当前 inert，不再监听文本变化或修改文档格式。
- `apps/desktop/src/document_handler.rs`：只做视觉排版、纯文本读取和撤销栈清理，不再提供隐藏字符 range API。

## 红线

- 不允许恢复 `hide_text_range` / `show_text_range` 路线。
- 不允许 QML 或 Android 自己维护一套跨平台业务 diff 语义。
- 不允许通过修改正文内容实现首行缩进、动画或富文本排版。
- 不允许保存 HTML。
- 不允许为了动画污染撤销栈、输入法 composition 或正文格式。
