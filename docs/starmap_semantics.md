# StarMap 语义地基 v1

## 概述

StarMap 不再仅仅是一个简单的“点和边”的通用图结构，而是一个**语义丰富的创作视图**。
节点不是一个简单的 UI 卡片，而是**创作实体在图上的视图映射**。它负责展示内容、提供跳转、指示来源、控制缩放行为。

## 节点语义模型

在 `StarMapNode` 中引入了以下核心语义字段：

### 1. `content` (StarMapNodeContent)
节点可以包含不同类型的内容引用：
* `Empty`：空节点，仅用于结构。
* `Inline`：内联内容，包含简单的摘要和详情（不鼓励将数万字正文存入节点）。
* `ChapterRef`：引用作品中的章节范围。
* `EntityRef`：引用项目中的实体（角色、地点等）。
* `ExternalRef`：外部资源的 URI 引用。

### 2. `anchors` (StarMapAnchor)
用于表达“这个节点来自哪里”或“这个节点指向哪里”，可以跨越节点甚至跨越作品。
* `ChapterRange`：指向特定章节的文本范围。
* `Project` / `Volume` / `Chapter`：指向具体的作品层级。
* `Character` / `Item` / `Location` / `Event`：指向实体。
* `Starmap`：指向另一个星图。
* `role`：区分是 `Source`、`Destination` 还是 `Reference`。

### 3. `portal` (StarMapPortal)
星图支持嵌套（类似于宇宙 -> 星系 -> 行星 -> 城市）。
通过 Portal 机制，当前节点本身可以是另一个星图的入口：
* `EnterChild`：双击或操作后，进入子星图。
* `PreviewInline`：可以在当前节点内嵌预览子星图。
* `ReferenceOnly`：仅作为链接参考。

### 4. `display_policy` (StarMapDisplayPolicy)
用于实现未来的语义缩放（Semantic Zoom）机制。它只定义策略，不实现具体的 UI 渲染。
包含重要性 (`importance`)、最小可见比例 (`min_visible_scale`)，以及何时显示标题、摘要或详情的比例控制 (`title_scale`, `summary_scale`, `detail_scale`)，确保在不同的视图层级只渲染有用的信息。

### 5. `open_behavior` (StarMapOpenBehavior)
点击或聚焦该节点时发生的行为定义：
* `Inspector`：打开检查器属性栏。
* `ExpandCard`：展开卡片预览详细信息。
* `WritingMode`：进入写作模式（如果节点承载了较长的正文）。
* `JumpToAnchor`：跳转到其绑定的锚点。
* `EnterPortal`：进入子星图。

### 6. `provenance` (StarMapProvenance)
用于记录节点的生成来源，实现内容追溯。
* `source`：支持人类 (`Human`)、导入 (`Import`)、插件 (`Plugin`) 和 AI (`Ai`)。
* 支持记录相关的 prompt ID 或生成标识，但 AI 仅仅是辅助工具和数据侧载的来源之一，并非 StarMap 的主设计约束。

### Legacy Payload
节点中的 `payload` 字段依然保留作为 Legacy/自定义扩展字段，但不应再作为主语义的承载入口。

## 布局语义

在 `StarMapLayoutNode` 中：
* 保留 `x`, `y`, `width`, `height`, `radius`
* 引入 `scale` 用于独立缩放节点
* 引入 `depth` 和 `focus_weight` 用于未来的 3D 或分层逻辑
* 引入 `orbit_group` 支持星系轨道排列

所有新增字段均通过 serde 的 `#[serde(default)]` 实现向后兼容，可以透明反序列化旧版 `graph.json` 和 `layout.json`。
