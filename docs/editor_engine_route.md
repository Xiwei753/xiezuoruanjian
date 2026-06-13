# 自绘编辑器与统一事件层路线

Status: active
Last verified: 2026-06-11
Truth source: product decision / code
Supersedes: None

本文档记录新的编辑器底层路线。当前优先级以用户最新指令为准：停止围绕 QML `TextArea` 做文字动画补丁，逐步迁移到 Core 统一编辑事件层 + 平台自绘 renderer。

## 结论

- 最终路线不是在 `TextArea` / `EditText` 上叠补丁，而是自研编辑器。
- Rust Core 统一描述文本状态变化、选区变化、撤销重做语义和动画事件。
- Desktop 使用 Qt/QML 原生组件承载自绘编辑器，底层优先 `QQuickPaintedItem` / `QQuickItem` + `QTextLayout` + `QPainter`。
- Android 使用 Kotlin `View` 自绘，`onDraw(Canvas)` 画正文、选区、光标和动画；`InputConnection` 接输入法。
- 平台层只负责输入适配、字体测量、换行、滚动、命中测试和绘制，不再自己猜 diff。

## 分阶段计划

### 第一阶段：停掉 Desktop 高危链

- 停用 `TextArea` 文字吐字动画。
- 禁止修改 `QTextDocument` 字符格式来隐藏/恢复文字，该排版辅助不得通过 `DocumentHandler` 进行。
- 保留稳定编辑、保存、滚轮和光标显示。
- 设置项可继续存在，但 Desktop typing animation 在新 renderer 落地前不生效。

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
- 当前 Desktop 默认启用 SujianEditorItem。
- 可通过 SUJIAN_DESKTOP_USE_SUJIAN_EDITOR=0/false/no/off 临时关闭回退。
- 当前最高优先级 bug 是 QTextLine xToCursor/cursorToX roundtrip 不互逆，非空行行尾 cursorToX 可能返回行首。
- 默认启用前必须完成中文输入、删除、换行、复制粘贴、全选、撤销重做、滚动裁剪和保存保护测试。
- 由于 Rust `qmetaobject` 当前不直接暴露 IME virtual event，Desktop 首版用隐藏平台输入桥只转发提交后的纯文本和按键命令；正文状态、保存和事务仍归 `SujianEditorItem`。

### 第四阶段：Android SujianEditorView

- 从当前 `WriterEditText` + overlay 逐步迁移到 `SujianEditorView : View`。
- `onDraw(Canvas)` 画正文、选区、光标和动画。
- `onCreateInputConnection()` 返回自定义输入连接，处理 `commitText`、`setComposingText`、`deleteSurroundingText`、`setSelection`。
- Android 渲染层只消费 Core transaction 和平台 text layout 结果。
- 当前 Android 仍保留 `WriterEditText` 过渡形态，但透明 `ForegroundColorSpan` 隐藏正文的吐字动画路径已停用。
- `TypingAnimationController` 只记录轻量 `AndroidEditorAnimationEvent` 占位，等待 `SujianEditorView` 接入 Core `EditorAnimationEvent`。

## 当前代码落点

- `core/writer_core/src/editor/transaction.rs`：统一编辑事务和动画事件骨架。
- `apps/desktop/src/sujian_editor_item/mod.rs`：Desktop 自绘编辑器主路径，内部维护纯文本、光标、选区、撤销重做和 Core transaction。
- `apps/desktop/qml/WritingWorkspace.qml`：默认启用 `SujianEditorItem` 和平台 IME 输入桥，支持通过开关临时回退至稳定 `TextArea`。
- `apps/desktop/qml/EditorTypingAnimator.qml`：保留为兼容占位，当前 inert，不再监听文本变化或修改文档格式。
- `apps/desktop/src/document_handler.rs`：只做视觉排版、纯文本读取和撤销栈清理，不再提供隐藏字符 range API。
- `apps/android/app/src/main/kotlin/com/xiwei/sujian/ui/TypingAnimationController.kt`：Android 过渡期只记录动画事件占位，不再向正文 `Editable` 注入透明 span。

## 红线

- 不允许恢复 `hide_text_range` / `show_text_range` 路线。
- 不允许 QML 或 Android 自己维护一套跨平台业务 diff 语义。
- 不允许通过修改正文内容实现首行缩进、动画或富文本排版。
- 不允许保存 HTML。
- 不允许为了动画污染撤销栈、输入法 composition 或正文格式。
- 不允许 Android 恢复 `ForegroundColorSpan(Color.TRANSPARENT)` 隐藏真实正文文字。

## sujian_editor_item 安全改法约束

以下约束针对 `apps/desktop/src/sujian_editor_item/` 下的高风险文件（rendering.rs、scene_graph.rs、mod.rs），防止 AI 或人类在修改时引入编译错误或逻辑回归。

### 静态层与 overlay 分离

- 静态层（`paint_onto`）永远完整绘制正文，是文本可见性的唯一权威来源。
- 动画 overlay（`paint_animation_overlay`）只能叠加视觉效果，不能决定正文可见性。
- overlay 的 glyph 位置计算错误，最坏情况是高亮条偏移，正文本身必须始终可见。

### text_revision

- 任何导致文本内容变化的操作必须 bump `text_revision`。
- `scroll_buffer_miss_reason` 依赖 `text_revision` 判断是否需要重绘，漏 bump 会导致旧文本残留。

### render/cache 逻辑改法

- 修改 `render_to_image` / `update_paint_node` / `paint_onto` 等大函数时，**禁止继续堆嵌套 if/else**。
- 必须拆 helper 函数，或使用早返回（early return）风格。
- 嵌套深度不得超过 3 层。超过时必须重构为独立函数。
- `scroll_buffer_miss_reason` 已作为示范：所有判断条件用早返回，最终只留 `None`。

### Qt Scene Graph 操作

- 禁止直接使用 `childAt(index)` 或 `insertChildNode(node, index)` 等 Qt 6.11 不存在的 API。
- 只能通过 `scene_graph.rs` 中已定义的 helper 函数操作 QSGNode 子节点顺序。
- scene graph 固定三层结构（child[0]=static, child[1]=overlay, child[2]=cursor）的注释描述了目标结构，但操作必须走 helper。

### 提交前验证

- 修改 `sujian_editor_item` 下任何文件后，必须运行 `cargo check -p sujian-desktop` 确认编译通过。
- 如果本地缺少 Qt6 开发环境，必须在有 Qt6 的环境（CI 或其他机器）上验证后才能合并。
- 任何涉及 `cpp!` 宏的修改，必须同时检查 Rust 侧和 C++ 侧的类型匹配。

## 最小静态渲染链路协议

自研编辑器任何渲染改动必须遵守"静态渲染最小链路优先"。这是硬规则，不是建议。

### 分层启用顺序

1. **static text texture 单独可运行** — `render_to_image()` → `update_texture_node()` → `QSGImageNode` child[0] 必须独立稳定。
2. **cursor 单独可开关** — `update_cursor_rect()` → `QSGRectangleNode` child[2] 必须在 static text 稳定后才能启用。
3. **overlay 单独可开关** — `paint_animation_overlay()` → `update_animation_overlay()` → `QSGImageNode` child[1] 必须在 static text + cursor 稳定后才能启用。
4. **三层同时开启前必须分别通过** — 不允许在任意一层未单独验证的情况下全量启用。
5. **出现 crash 先禁用上层，不许在完整链路里猜** — 设置 `SUJIAN_MINIMAL_STATIC_RENDER=1` 运行，只保留 Layer 0。

### QSG 层操作约束

- QSG 层不许一次改多个节点层。每次只改一层，验证通过后再改下一层。
- `QImage -> QSGTexture` 必须有单独 smoke test。`createTextureFromImage` 是已知高风险调用，必须在 render thread 正确阶段执行。
- overlay 不许参与正文可见性。overlay 只做叠加视觉效果，正文可见性由 static text texture 唯一决定。
- `glyphRuns` 只服务动画定位，不影响静态正文绘制。

### 最小渲染模式

- 环境变量 `SUJIAN_MINIMAL_STATIC_RENDER=1` 启用最小渲染模式。
- 最小模式下只执行 Layer 0（static text texture），跳过 Layer 1（overlay）和 Layer 2（cursor）的 QSG 更新。
- 动画状态仍会清理（`cleanup_finished_animations`），但不绘制、不更新 QSG 节点。
- 此模式用于隔离崩溃：如果最小模式下仍崩溃，问题在 `QImage -> QSGTexture` / `QSGImageNode` 路线本身。

### 降级路线

如果静态 texture 也崩，说明当前 `QImage + QSGImageNode + rust-cpp` 路线需要降级：

- **短期**：`QQuickPaintedItem` + `QPainter` 静态绘制，先保证正确性。Qt 官方说明 `QQuickPaintedItem` 性能不如直接 QSG，但它适合先把 QPainter 内容稳定画出来。
- **中期**：`QQuickItem + QSG` 只做光标/overlay。
- **长期**：再把正文 texture 化。

这不是倒退，是工程顺序。

### text_revision 闭环

- `text_revision` 字段和 `bump_revision` 方法必须进入统一链路，不允许存在"加了但没读"的死代码。
- 任何导致文本内容变化的操作必须 bump `text_revision`。
- `scroll_buffer_miss_reason` 依赖 `text_revision` 判断是否需要重绘。
- 如果编译器警告 `field text_revision is never read` 或 `method bump_revision never used`，必须立即修复，不能留死代码。
