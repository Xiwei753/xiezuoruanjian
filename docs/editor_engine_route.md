# 自研编辑器渲染引擎路线

Status: active
Last verified: 2026-06-23
Truth source: code / product decision / protocol
Supersedes: editor_engine_route.md (previous version)

核心问题：写作区走自研渲染，不依赖系统 TextArea / QTextDocument 的排版行为。

## 当前唯一真实路线

Desktop 自研写作区渲染管线（已实现、已验收）：

```
Rust Core EditorTransaction
  → SujianEditorItem (QQuickItem)
    → QTextLayout / QTextLine 排版
    → QImage static texture (Layer 0)
    → QSGImageNode 上屏
    → QML Rectangle cursor (绑定 Rust cursor_rect_x/y/width/height)
    → QML EditorAnimationOverlay (消费 animation_events_json 信号，渲染 EditorGlyphGhost)
```

### 各层职责

| 层 | 实现 | 职责 |
|----|------|------|
| 排版 | `QTextLayout` / `QTextLine` | Unicode 文字排版、光标定位（xToCursor/cursorToX） |
| 静态正文 | `QImage` → `QSGImageNode` | 完整绘制正文纹理，不为动画隐藏/替换文字 |
| 光标 | QML `Rectangle` | 绑定 Rust 暴露的 cursor rect 属性 |
| 动画 | QML `EditorAnimationOverlay` + `EditorGlyphGhost` | 逐字 ghost 动画（insert 从光标吐出，delete 向光标吞回） |
| 事务 | Rust Core `EditorTransaction` / `EditorAnimationEvent` | 统一管理插入、删除、选区、格式化、动画事件 |

### 关键文件

| 文件 | 职责 |
|------|------|
| `apps/desktop/src/sujian_editor_item/mod.rs` | 自研编辑器核心入口，处理渲染和交互 |
| `apps/desktop/src/sujian_editor_item/rendering.rs` | 渲染逻辑（QImage → QSGImageNode） |
| `apps/desktop/src/sujian_editor_item/cursor_controller.rs` | 光标控制 |
| `apps/desktop/src/sujian_editor_item/buffer.rs` | 渲染缓冲 |
| `apps/desktop/src/editor/layout.rs` | QTextLayout 排版封装 |
| `apps/desktop/src/editor/renderer.rs` | 渲染器 |
| `apps/desktop/src/editor/scene_graph.rs` | QSG 节点管理 |
| `apps/desktop/src/editor/input.rs` | 输入事件处理 |
| `apps/desktop/qml/WritingWorkspace.qml` | 写作工作区，直接使用 SujianEditorItem |
| `apps/desktop/qml/EditorAnimationOverlay.qml` | 动画 overlay，唯一动画主路径 |
| `apps/desktop/src/document_handler.rs` | 仅 legacy TextArea 兼容辅助 |

### text_revision 机制

- `text_revision` 单调递增计数器，`bump_revision` 每次调用递增，标记"文字内容已变更"
- 任何导致文字内容变化的操作都必须 bump `text_revision`
- `scroll_buffer_miss_reason` 与 `text_revision` 关联，确保 bump 后正确刷新缓存

### 编译验证

- 修改 `sujian_editor_item` 代码后，必须通过 `cargo check -p sujian-desktop` 验证编译
- 不依赖本地 Qt6 环境，不假设 CI 有 Qt6 完整开发包

## Desktop 动画唯一主路径

1. Rust Core 生成 `EditorTransaction` / `EditorAnimationEvent`，填充 `glyph_rects`
2. `SujianEditorItem` 通过 `animation_events_json` 属性暴露给 QML
3. QML `EditorAnimationOverlay` 监听 `animationEventsChanged` 信号，解析 JSON 事件
4. `EditorGlyphGhost` 组件渲染逐字 ghost 动画

Rust 侧只负责：排版、命中、选区、光标、事务、glyph rects。
QML 侧负责：把 ghost 动画画出来。

## 禁止（硬性红线）

| 禁止行为 | 原因 |
|---------|------|
| 用 DocumentHandler / QTextDocument 修自研写作区 | 旧路线，已被取代 |
| 用 TextArea 排版逻辑驱动自研编辑器 | TextArea 是 fallback，不是主路径 |
| QSG 三层 overlay（paint_animation_overlay / update_animation_overlay）当主路径 | experimental，不是当前验收路径 |
| 静态正文层为动画隐藏文字 | 正文永远完整绘制 |
| 正文透明 span / hidden range / ForegroundColorSpan | 破坏纯文本 |
| 保存 HTML | 正文永远是纯文本 |
| 在正文里插入空格做首行缩进 | 破坏纯文本 |
| 绕过 Core transaction 直接修改正文 | 违反 Core-first 原则 |
| 用 `childAt(index)` / `insertChildNode(node, index)` 等 Qt 6.11 未稳定 API | 不稳定 |

## Qt Scene Graph 指南

- 在 `scene_graph.rs` 中提供 helper 函数管理 QSGNode 子节点顺序
- scene graph 子节点顺序固定，不得随意调换
- QSG 节点树结构必须保持稳定，不能在渲染过程中频繁重建
- `QImage -> QSGTexture` 转换必须有 smoke test，`createTextureFromImage` 可能返回 null，必须在 render thread 检查

## Android 路线（概要）

Android 当前阶段走 WriterEditText + AppCompatEditText 原生文本栈。
通过 ActionMode.Callback / onTextContextMenuItem / 长按手势接管写作菜单和选区策略。
SujianEditorView 仅作为后续阶段，在菜单、选区、IME、保存、动画契约稳定后推进。
详见 `docs/TECHNICAL_ROUTE.md`。
