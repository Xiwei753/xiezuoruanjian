# 自研编辑器渲染引擎路线

Status: active
Last verified: 2026-06-23
Truth source: code / product decision / protocol
Supersedes: editor_engine_route.md (previous version)

核心问题：写作区走自研渲染，不依赖系统 TextArea / QTextDocument 的排版行为。

## 当前唯一真实路线

Linux_qt 自研写作区渲染管线（已实现、已验收）：

```
Rust Core EditorTransaction
  → SujianEditorItem (QQuickItem)
    → QTextLayout / QTextLine 排版
    → QImage static texture (Layer 0)
    → QSGImageNode 上屏
    → QML Rectangle cursor (绑定 Rust cursor_rect_x/y/width/height)
    → Rust AnimationCoordinator / RenderPlan / VisualPayload (SujianEditorItem 内部动画管线)
```

### 各层职责

| 层 | 实现 | 职责 |
|----|------|------|
| 排版 | `QTextLayout` / `QTextLine` | Unicode 文字排版、光标定位（xToCursor/cursorToX） |
| 静态正文 | `QImage` → `QSGImageNode` | 正常完整绘制正文纹理；插入动画期间临时跳过 inserted range（自研渲染层的内部渲染状态，不是正文数据污染）；动画结束后恢复完整绘制 |
| 光标 | QML `Rectangle` | 绑定 Rust 暴露的 cursor rect 属性 |
| 动画 | Rust `AnimationCoordinator` / `RenderPlan` / `VisualPayload` | 逐字/整簇/run/reflow 动画（insert 从光标吐出，delete 向光标吞回），SujianEditorItem 内部 Scene Graph 渲染 |
| 事务 | Rust Core `EditorTransaction` / `EditorVisualTransaction` | 统一管理插入、删除、选区、格式化和视觉事务；Linux QML 只消费 `visual_transaction_json` |

### 关键文件

| 文件 | 职责 |
|------|------|
| `apps/Linux_qt/src/sujian_editor_item/mod.rs` | 自研编辑器核心入口，处理渲染和交互 |
| `apps/Linux_qt/src/sujian_editor_item/rendering.rs` | 渲染逻辑（QImage → QSGImageNode） |
| `apps/Linux_qt/src/sujian_editor_item/cursor_controller.rs` | 光标控制 |
| `apps/Linux_qt/src/sujian_editor_item/buffer.rs` | 渲染缓冲 |
| `apps/Linux_qt/src/sujian_editor_item/animation_coordinator.rs` | 动画协调器，生成 RenderPlan |
| `apps/Linux_qt/src/sujian_editor_item/animated_slice.rs` | 统一 AnimatedSlice 数据模型（Insert/Delete/Move/CrossfadeOld/CrossfadeNew/SnapshotOld/SnapshotNew） |
| `apps/Linux_qt/src/sujian_editor_item/render_plan.rs` | 渲染计划，驱动 Scene Graph 更新 |
| `apps/Linux_qt/src/editor/layout.rs` | QTextLayout 排版封装 |
| `apps/Linux_qt/src/editor/renderer.rs` | 渲染器 |
| `apps/Linux_qt/src/editor/scene_graph.rs` | QSG 节点管理 |
| `apps/Linux_qt/src/editor/input/` | 输入事件处理（三层结构入口：`qt_surface.rs` / `platform_ime.rs` / `controller.rs`） |
| `apps/Linux_qt/qml/WritingWorkspace.qml` | 写作工作区，直接使用 SujianEditorItem |

### text_revision 机制

- `text_revision` 单调递增计数器，`bump_revision` 每次调用递增，标记"文字内容已变更"
- 任何导致文字内容变化的操作都必须 bump `text_revision`
- `scroll_buffer_miss_reason` 与 `text_revision` 关联，确保 bump 后正确刷新缓存

### 编译验证

- 修改 `sujian_editor_item` 代码后，必须通过 `cargo check -p sujian-linux-qt` 验证编译
- 不依赖本地 Qt6 环境，不假设 CI 有 Qt6 完整开发包

## Linux_qt 动画唯一主路径

1. Rust Core 生成 `EditorTransaction` / `EditorVisualTransaction`，平台层填充 glyph / cursor / reflow rects
2. `SujianEditorItem` 通过 `visual_transaction_json` 属性接收视觉事务
3. Rust `AnimationCoordinator` 解析 payload，生成 `RenderPlan`
4. `SujianEditorItem::updatePaintNode` 按 RenderPlan 更新 Scene Graph 节点

Rust 侧负责：排版、命中、选区、光标、事务、glyph rects、动画协调、RenderPlan 生成。
QML 侧负责：光标 Rectangle 绑定、滚动容器。

动画期间静态正文层行为：
- **Insert 动画**：静态正文层临时跳过 inserted range（自研渲染层的内部渲染状态，不是正文数据污染），AnimationCoordinator 驱动纹理动画
- **Reflow 动画**：静态正文层临时跳过受插入影响的 reflow hidden ranges，由 AnimationCoordinator 驱动位移动画；insert / reflow 完成或跳过时优先按 transactionId / rangeId 清理，byte range 只作兜底
- **Delete 动画**：使用旧 glyph snapshot（删除前的字形位置），AnimationCoordinator 驱动吞回动画
- 动画由 SujianEditorItem 内部 Scene Graph 渲染，不是 QML overlay

滚动打断、字号/行距变化、章节切换、关闭动画、加载正文时，必须立即清掉 hidden range 并重绘静态层，不能依赖 timeout，不能出现文字消失或重影。

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

Android SujianEditorView 已进入自绘阶段，且是 Android 正文写作区唯一主路径，分层绘制：静态正文层 → 选区高亮层 → preedit 层 → 动画层 → 光标层。
动画期间静态层跳过 animated insert range 避免重影，删除动画使用删除前 snapshot glyph rect；活跃动画期间继续 `invalidate`，`insertRangeId` / `reflowRangeIds` 精确清 hidden range 必须保留。
不得回退 WriterEditText、Span/透明文字或 JSON parser 路线。
详见 `docs/TECHNICAL_ROUTE.md`。

## 动画契约边界（两端一致）

### 已实现（Linux_qt + Android）

| 动画类型 | hidden range | overlay | 超时安全 | 滚动/加载/设置打断 |
|---------|-------------|---------|---------|------------------|
| Insert (Glyph/Cluster/Run) | ✅ 创建+清理 | ✅ ghost | ✅ 2×duration+200ms | ✅ 立即清理 |
| Insert (LineReflow) | ✅ reflow ranges | ✅ reflow ghost | ✅ 2×duration+200ms | ✅ 立即清理 |
| Delete | 无 hidden range | ✅ snapshot ghost | N/A（无 hidden range） | ✅ 清除动画状态 |
| SnapshotAnimation | ❌ Core 不再下发此模式（choose_animation_mode 返回 RunAnimation），枚举保留用于前向兼容 | 同左 | N/A | N/A |
| Preedit | ✅ 清除活跃动画 | N/A | N/A | ✅ |

### 超时安全策略（两端统一）

- Linux_qt: `TextAnimationState::tick` — 2×duration + 200ms 宽限期
- Android: `SujianEditorRenderer.tickAnimations` — 动态计算 2×duration + 200ms（下限 520ms，上限 3000ms）
- 两端策略一致：基于动画 duration 动态计算，不是固定值

### 当前边界（不虚报）

1. **SnapshotAnimation 不可用**：Core 的 `choose_animation_mode()` 不再返回此模式（>40 cluster 改用 RunAnimation）。枚举变体保留用于前向兼容，但各端不应再收到此模式。两端已有的降级逻辑（Linux_qt → SystemSuppressed，Android → skip）作为防御性兜底保留。
2. **Delete 动画无 hidden range 回收问题**：Delete 不产生 hidden range，只有 overlay snapshot ghost。如果 QML overlay 的 delete 动画卡住，不影响正文显示（正文已删除，overlay 只是视觉残留）。
3. **Delete 动画无独立超时**：Delete 动画没有 hidden range 需要回收，其 overlay ghost 的 `isFinished` 基于 `durationMs` 判断，超时后自动从 `activeAnimations` 移除。
4. **滚动/加载/格式化/设置变化/关闭动画**：所有路径立即清除 hidden range 和动画状态，不依赖 timeout。
