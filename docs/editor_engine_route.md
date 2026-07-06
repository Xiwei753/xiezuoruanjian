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
| 静态正文 | `QImage` → `QSGImageNode` | 正常完整绘制正文纹理；插入动画期间临时跳过 inserted range（自研渲染层的内部渲染状态，不是正文数据污染）；动画结束后恢复完整绘制 |
| 光标 | QML `Rectangle` | 绑定 Rust 暴露的 cursor rect 属性 |
| 动画 | QML `EditorAnimationOverlay` + `EditorGlyphGhost` | 逐字 ghost 动画（insert 从光标吐出，delete 向光标吞回） |
| 事务 | Rust Core `EditorTransaction` / `EditorAnimationEvent` | 统一管理插入、删除、选区、格式化、动画事件 |

### 关键文件

| 文件 | 职责 |
|------|------|
| `apps/Linux_qt/src/sujian_editor_item/mod.rs` | 自研编辑器核心入口，处理渲染和交互 |
| `apps/Linux_qt/src/sujian_editor_item/rendering.rs` | 渲染逻辑（QImage → QSGImageNode） |
| `apps/Linux_qt/src/sujian_editor_item/cursor_controller.rs` | 光标控制 |
| `apps/Linux_qt/src/sujian_editor_item/buffer.rs` | 渲染缓冲 |
| `apps/Linux_qt/src/sujian_editor_item/text_animation_state.rs` | 文字动画状态管理 |
| `apps/Linux_qt/src/editor/layout.rs` | QTextLayout 排版封装 |
| `apps/Linux_qt/src/editor/renderer.rs` | 渲染器 |
| `apps/Linux_qt/src/editor/scene_graph.rs` | QSG 节点管理 |
| `apps/Linux_qt/src/editor/input/` | 输入事件处理（三层结构入口：`qt_surface.rs` / `platform_ime.rs` / `controller.rs`） |
| `apps/Linux_qt/qml/WritingWorkspace.qml` | 写作工作区，直接使用 SujianEditorItem |
| `apps/Linux_qt/qml/EditorAnimationOverlay.qml` | 动画 overlay，唯一动画主路径 |

### text_revision 机制

- `text_revision` 单调递增计数器，`bump_revision` 每次调用递增，标记"文字内容已变更"
- 任何导致文字内容变化的操作都必须 bump `text_revision`
- `scroll_buffer_miss_reason` 与 `text_revision` 关联，确保 bump 后正确刷新缓存

### 编译验证

- 修改 `sujian_editor_item` 代码后，必须通过 `cargo check -p sujian-linux-qt` 验证编译
- 不依赖本地 Qt6 环境，不假设 CI 有 Qt6 完整开发包

## Desktop 动画唯一主路径

1. Rust Core 生成 `EditorTransaction` / `EditorAnimationEvent`，填充 `glyph_rects`
2. `SujianEditorItem` 通过 `animation_events_json` 属性暴露给 QML
3. QML `EditorAnimationOverlay` 监听 `animationEventsChanged` 信号，解析 JSON 事件
4. `EditorGlyphGhost` 组件渲染逐字 ghost 动画

Rust 侧只负责：排版、命中、选区、光标、事务、glyph rects。
QML 侧负责：把 ghost 动画画出来。

动画期间静态正文层行为：
- **Insert 动画**：静态正文层临时跳过 inserted range（自研渲染层的内部渲染状态，不是正文数据污染），动画 overlay 渲染 ghost glyph
- **Delete 动画**：使用旧 glyph snapshot（删除前的字形位置），overlay 渲染吞回动画
- 动画 overlay 是动画层，不是完整正文 overlay 冒充

## 禁止（硬性红线）

| 禁止行为 | 原因 |
|---------|------|
| 用 DocumentHandler / QTextDocument 修自研写作区 | 旧路线，已删除（document_handler.rs 已清理） |
| 用 TextArea 排版逻辑驱动自研编辑器 | 旧路线，已删除（EditorPage.qml 已清理） |
| QSG 三层 overlay（paint_animation_overlay / update_animation_overlay）当主路径 | experimental，不是当前验收路径 |
| 静态正文层永久隐藏文字 | 自研渲染层的 hidden range 是临时渲染状态，插入动画期间临时跳过 inserted range 是允许的；禁止的是：永久隐藏正文、在正文数据中插入隐藏字符/透明span/透明颜色污染正文数据、正文完整绘制+overlay冒充真吐字 |
| 正文透明 span / ForegroundColorSpan 污染正文数据 | 破坏纯文本，与自研渲染层临时 hidden range（纯渲染状态）不同 |
| 正文完整绘制 + overlay 冒充真吐字 | overlay 是动画层，不是完整正文 overlay |
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

Android SujianEditorView 已进入自绘阶段，分层绘制：静态正文层 → 选区高亮层 → preedit 层 → 动画层 → 光标层。
动画期间静态层跳过 animated insert range 避免重影，删除动画使用删除前 snapshot glyph rect。
WriterEditText 仍作为兼容 fallback 存在。
详见 `docs/TECHNICAL_ROUTE.md`。
