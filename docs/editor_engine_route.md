# 自绘编辑器渲染引擎与统一编辑事件事务层路线

Status: active
Last verified: 2026-06-19
Truth source: product decision / code
Supersedes: None

核心问题：写作区到底走原生控件还是自研渲染？答案是：逐步脱离 QML `TextArea` 的排版限制，转向 Core 统一编辑事务 + 自研 renderer。

## 目标

- 不再依赖系统 `TextArea` / `EditText` 的排版行为，不再被其限制
- Rust Core 统一管理编辑事务、选区、动画事件和保存状态
- Desktop 走 Qt/QML 自研渲染层，基于 `QQuickPaintedItem` / `QQuickItem` + `QTextLayout` + `QPainter`
- Android 走 Kotlin `View` 层，`onDraw(Canvas)` 自绘文字和选区；`InputConnection` 接管输入
- 所有修改正文的操作必须通过 Core `EditorTransaction`，平台端不得自行修改正文
- 所有平台共享同一份 Core 事务逻辑，UI 层只消费 diff

## 长按、选区与系统文本能力接管边界

素笺不追求在所有平台重写完整系统文本栈。

平台原生能力负责：
- IME 组合输入
- 系统键盘接入
- 基础剪贴板
- 基础辅助功能
- 平台文字布局/字体 fallback
- 系统级文本控件生命周期

素笺必须接管：
- 长按后弹出的菜单内容
- 长按选区策略
- 选区手柄拖动后的命令状态
- copy/cut/paste/selectAll/share/加入星图/AI 辅助等菜单项的可见性与执行入口
- 所有会修改正文的动作进入 Core EditorTransaction
- 动画事件只消费 transaction，不污染正文

## Android 路线

Android 当前阶段不立刻切换到 SujianEditorView。
短期主路径是 WriterEditText + AppCompatEditText 原生文本栈，
通过 ActionMode.Callback / onTextContextMenuItem / 长按手势接管写作菜单和选区策略。
SujianEditorView 仅作为后续阶段，在菜单、选区、IME、保存、动画契约稳定后推进。

## 实施阶段

### 第一阶段：脱离 Desktop TextArea

- 脱离 `TextArea` 的排版和光标控制
- 不再用 `QTextDocument` 做排版/格式化/选区，不再依赖 `DocumentHandler` 修复
- 建立统一编辑事务层
- 确立渲染管线，为 Desktop typing animation 和 renderer 打下基础

### 第二阶段：建立 Core 编辑事务层

- 在 `core/writer_core/src/editor` 建立统一编辑事务模型
- `EditorTransaction` 统一描述插入、删除、替换、选区变更、格式化等操作
- `EditorAnimationEvent` 从 renderer 角度描述动画触发和参数
- Desktop 和 Android 都通过 Core transaction，不再由 QML/Kotlin 各自拼 diff

### 第三阶段：Desktop SujianEditorItem

- 基于 `SujianEditorItem : QQuickItem` 或 `QQuickPaintedItem` 实现
- 自绘文字、光标、选区高亮、行号等
- 使用 `QTextLayout` 做 Unicode 文字排版和光标定位
- QML `WritingWorkspace` 直接使用 `SujianEditorItem`，不再用 `TextArea` 回退
- 默认启用 Desktop 自研 SujianEditorItem
- 可通过 SUJIAN_DESKTOP_USE_SUJIAN_EDITOR=0/false/no/off 回退到 TextArea
- 已知关键 bug 是 QTextLine xToCursor/cursorToX roundtrip 不一致，必须先保证 cursorToX 正确
- 在自研编辑器稳定前，不得删除 TextArea fallback 路径，不得强制所有用户切到自研
- 未来考虑 Rust `qmetaobject` 暴露 IME virtual event，Desktop 不再依赖 QML 输入事件；但这不是当前阶段必须项，当前阶段仍走 `SujianEditorItem`

### 第四阶段：Android SujianEditorView

- 从 `WriterEditText` + overlay 过渡到 `SujianEditorView : View`
- `onDraw(Canvas)` 自绘文字、光标、选区
- `onCreateInputConnection()` 接管输入连接，处理 `commitText`、`setComposingText`、`deleteSurroundingText`、`setSelection`
- Android 也通过 Core transaction 统一 text layout 和保存
- 在 Android 切到 `WriterEditText` 之前，不得用 `ForegroundColorSpan` 做文字隐藏和动画
- `TypingAnimationController` 当前消费 `AndroidEditorAnimationEvent`，未来切到 `SujianEditorView` 后消费 Core `EditorAnimationEvent`

## 关键文件

- `core/writer_core/src/editor/transaction.rs`：统一编辑事务模型和状态管理
- `apps/desktop/src/sujian_editor_item/mod.rs`：Desktop 自研编辑器核心，处理渲染和交互，消费 Core transaction
- `apps/desktop/qml/WritingWorkspace.qml`：直接使用 `SujianEditorItem` 和 IME 交互，不再回退到 `TextArea`
- `apps/desktop/qml/EditorTypingAnimator.qml`：打字动画组件，当前 inert，等待 Core transaction 驱动
- `apps/desktop/src/document_handler.rs`：仅作为 legacy/stable TextArea 兼容辅助，不得用于修复自研写作区光标、命中、滚动、动画
- `apps/android/app/src/main/kotlin/com/xiwei/sujian/ui/TypingAnimationController.kt`：Android 打字动画控制器，当前消费 `Editable` 和 span

## 禁止

- 禁止用 `hide_text_range` / `show_text_range` 做文字隐藏
- 禁止在 QML 或 Android 端自行拼接 diff 和保存
- 禁止绕过 Core transaction 直接修改正文
- 禁止保存 HTML
- 禁止在正文里插入空格做首行缩进
- 禁止在 Android 用 `ForegroundColorSpan(Color.TRANSPARENT)` 做文字隐藏

## sujian_editor_item 实现指南

以下针对 `apps/desktop/src/sujian_editor_item/` 目录的模块（rendering.rs、scene_graph.rs、mod.rs），供 AI 助手理解和修改时参考。

### 渲染 overlay 分层

- 基础层（`paint_onto`）：绘制静态文字，不包含动画效果
- 动画 overlay（`paint_animation_overlay`）：绘制打字动画效果，叠加在基础层之上
- overlay 的 glyph 位置必须与基础层完全对齐，否则会出现文字抖动或重影

### text_revision

- 任何导致文字内容变化的操作都必须 bump `text_revision`
- `scroll_buffer_miss_reason` 与 `text_revision` 关联，确保 bump 后正确刷新缓存

### render/cache 路径

- 所有 `render_to_image` / `update_paint_node` / `paint_onto` 路径必须清晰，**不能有模糊的 if/else**
- 每个 helper 函数职责单一，尽早返回（early return），避免嵌套
- 缓存失效条件必须明确，不能超过 3 层嵌套判断
- `scroll_buffer_miss_reason` 必须有明确语义：什么条件下缓存失效，什么条件下返回 `None`

### Qt Scene Graph 指南

- 不使用 `childAt(index)` 和 `insertChildNode(node, index)` 等 Qt 6.11 未稳定 API
- 在 `scene_graph.rs` 中提供 helper 函数管理 QSGNode 子节点顺序
- scene graph 子节点顺序固定（child[0]=static, child[1]=overlay, child[2]=cursor），不得随意调换，提供 helper 保护

### 编译验证

- 修改 `sujian_editor_item` 代码后，必须通过 `cargo check -p sujian-desktop` 验证编译
- 不依赖本地 Qt6 环境，不假设 CI 有 Qt6 完整开发包，不做本地链接验证
- 注意 `cpp!` 宏的使用，确保 Rust 和 C++ 之间的类型转换正确

## 渲染管线分步验证策略

自研编辑器渲染管线采用"分步验证、逐步叠加"策略，原因如下：

### 分步验证步骤

1. **static text texture 验证** - `render_to_image()` 和 `update_texture_node()` 将 `QSGImageNode` child[0] 渲染到屏幕
2. **cursor 验证** - `update_cursor_rect()` 将 `QSGRectangleNode` child[2] 正确叠加在 static text 之上
3. **overlay 验证** - `paint_animation_overlay()` 和 `update_animation_overlay()` 将 `QSGImageNode` child[1] 正确叠加在 static text + cursor 之上
4. **交互事件验证** - 键盘/鼠标/IME 事件正确传递到 Core
5. **如有 crash 或渲染异常，先降级验证** - 设置 `SUJIAN_MINIMAL_STATIC_RENDER=1`，只验证 Layer 0

### QSG 调试要点

- QSG 节点树结构必须保持稳定，不能在渲染过程中频繁重建
- `QImage -> QSGTexture` 转换必须有 smoke test，`createTextureFromImage` 可能返回 null，必须在 render thread 检查
- overlay 必须独立于 static text texture 更新，overlay 更新不能导致 static text 重新渲染
- `glyphRuns` 必须正确获取，不能为空或错位

### 降级验证流程

- 设置 `SUJIAN_MINIMAL_STATIC_RENDER=1` 只验证基础渲染
- 基础渲染通过后，逐步叠加 Layer 0（static text texture），然后 Layer 1（overlay）和 Layer 2（cursor）的 QSG 节点
- 动画清理逻辑（`cleanup_finished_animations`）必须正确移除过期动画，不能残留 QSG 节点
- 降级验证必须覆盖：正常渲染、滚动、文字变更、resize，确保 `QImage -> QSGTexture` / `QSGImageNode` 路径稳定

### 备选方案

如果 texture 方案遇到无法解决的问题，可以考虑 `QImage + QSGImageNode + rust-cpp` 之外的其他方案：

- **方案 A**：`QQuickPaintedItem` + `QPainter` 直接绘制，性能较低但 Qt 官方支持 `QQuickPaintedItem` 内部自动管理 QSG，不需要手动管理 QPainter 生命周期
- **方案 B**：`QQuickItem + QSG` 直接绘制/overlay
- **方案 C**：混合 texture 方案

具体选择视性能和稳定性决定。

### text_revision 机制

- `text_revision` 是一个单调递增计数器，`bump_revision` 每次调用递增，用于标记"文字内容已变更"
- 任何导致文字内容变化的操作都必须 bump `text_revision`
- `scroll_buffer_miss_reason` 与 `text_revision` 关联，确保 bump 后正确刷新缓存
- 编译时可能有 `field text_revision is never read` 或 `method bump_revision never used` 警告，这是正常的，因为该机制尚未完全接入
