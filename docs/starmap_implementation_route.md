# 星图实现路线

Status: active
Last verified: 2026-06-13
Truth source: docs/starmap_canvas_model.md / community practice / implementation constraints
Scope: Core storage, DTO, renderer, interaction, migration, and validation for StarMap

## 1. 本文定位

`docs/starmap_canvas_model.md` 是产品与数据模型契约，回答“星图到底是什么”。

本文是实现路线，回答“怎么做出来”。

实现不得绕过画布模型契约：

- 星图是独立画布。
- 节点是原子内容卡片。
- 子星图不是节点。
- 包含不是线。
- 超链接不是线。
- 线的端点是 EndpointPath。
- 缩放简化只改变渲染细节，不改变数据。
- SQLite 只能做可重建本地索引，不能做真相库。

## 2. 社区路线对照

实现路线参考以下成熟方向，但不照搬任何一个产品：

- JSON Canvas / Obsidian Canvas 路线：画布以 nodes + edges 的 JSON 结构表达，节点有位置和尺寸，边连接节点并可带 label。适合作为基础文件格式参考。
- tldraw 路线：把 document records 与 session/camera/UI 状态分开；document 可保存到自定义后端；schema 需要迁移系统。
- React Flow 路线：nodes、edges、viewport 可以序列化保存和恢复；说明“节点/边/视口”是图编辑器的基础状态边界。
- Excalidraw 路线：scene 由 elements 与 appState 组成，元素有 id/version/isDeleted 等字段；说明画布对象应有稳定 id、版本和软删除/历史能力。

结论：

- 使用 JSON 表达画布对象是成熟路线。
- 使用 nodes/edges/viewport/session 分层是成熟路线。
- 使用 schemaVersion + migration 是必须路线。
- 本项目的特殊点是 EndpointPath、子星图、Git 友好文件包和 Core-first，因此不能直接套任何现成格式。

## 3. 分层实现

### 3.1 Core domain layer

Core 定义星图真相模型：

- `StarMapDocument`
- `StarMapNode`
- `StarMapChildMap`
- `StarMapEdge`
- `StarMapEndpointPath`
- `StarMapHyperlink`
- `StarMapLayout`
- `StarMapVisualFilter`
- `StarMapProposal`

Core 负责：

- 加载星图包。
- 保存星图包。
- 校验 EndpointPath。
- 校验子星图循环引用。
- 校验断链。
- 生成渲染快照。
- 执行增删改事务。
- 生成 AI proposal patch。
- 迁移旧格式。

前端不得自己维护长期星图业务状态。

### 3.2 Storage layer

星图真相落盘为 Git 友好的文件包：

```text
starmaps/
└── <starmap_id>/
    ├── graph.json
    ├── nodes/
    │   └── <node_id>.json
    ├── child_starmaps/
    │   └── <child_instance_id>.json
    ├── edges/
    │   └── <edge_id>.json
    ├── hyperlinks/
    │   └── <hyperlink_id>.json
    ├── layouts/
    │   ├── default.json
    │   └── local.json
    └── metadata/
        └── migration.json
```

#### graph.json

只保存星图元信息和对象顺序，不保存所有对象全文。

```json
{
  "schemaVersion": 2,
  "id": "tools",
  "title": "工具星图",
  "description": "常用工具整理",
  "nodeOrder": ["node-gpt"],
  "childStarMapOrder": ["child-ai-tools"],
  "edgeOrder": ["edge-me-gpt"],
  "hyperlinkOrder": [],
  "createdAt": 0,
  "updatedAt": 0
}
```

#### nodes/<node_id>.json

保存原子节点内容。

节点不允许包含节点或子图。

#### child_starmaps/<child_instance_id>.json

保存当前星图对另一张星图的放置对象。

子星图有自己的 `targetStarMapId`，以及在父星图中的位置、尺寸、缩放、显示策略。

#### edges/<edge_id>.json

保存用户画出来的线。

Edge 必须使用 EndpointPath。

#### hyperlinks/<hyperlink_id>.json

保存文本/内容跳转链接。

Hyperlink 不参与画布关系线渲染。

#### layouts/*.json

保存坐标、缩放、折叠、视口等显示数据。

语义对象不得依赖 layout 才能成立。

### 3.3 Optional index layer

后续可以新增：

```text
.sujian/cache/starmap_index.sqlite
```

只用于：

- 全文搜索。
- EndpointPath 快速解析。
- 节点相关边查询。
- 星图包含关系查询。
- 视口快照加速。

SQLite 必须可删除、可重建，不参与同步真相。

## 4. EndpointPath 设计

EndpointPath 是线端点的唯一新路线。

旧 `from: string` / `to: string` 只能作为迁移兼容字段。

### 4.1 数据结构

```json
{
  "type": "endpointPath",
  "path": [
    { "kind": "starmap", "id": "tools" },
    { "kind": "starmap", "id": "ai-tools" },
    { "kind": "node", "id": "gpt" }
  ]
}
```

### 4.2 合法路径

合法路径必须满足：

- 第一层必须能从当前星图上下文解析。
- 中间层只能是 starmap / child_starmap。
- 末层可以是 starmap、child_starmap、node。
- 不允许路径循环。
- 不允许指向已经删除的对象。
- 允许暂时 unresolved，但必须带状态，不能假装有效。

### 4.3 渲染解析

渲染器接收完整 EndpointPath，但不一定画到最终对象。

Core 快照应给出多级解析结果：

```json
{
  "fullPath": [...],
  "visibleTargetLevel": 1,
  "resolvedScreenTarget": {
    "kind": "starmap",
    "id": "tools",
    "x": 100,
    "y": 120
  }
}
```

缩放越近，`visibleTargetLevel` 越深。

数据不变，渲染精度变化。

## 5. 渲染路线

### 5.1 快照驱动

前端不得直接读星图文件。

Core 输出 `StarMapRenderSnapshot`：

- visible nodes
- visible child starmaps
- visible edges
- visible edge labels
- visible hyperlinks indicators
- viewport
- simplification level
- hit test regions

前端只渲染快照，并把交互事件回传 Core。

### 5.2 缩放简化等级

建议分四级：

#### Detail

- 显示节点标题和内容摘要。
- 显示线标签。
- 显示箭头细节。
- 可直接编辑。

#### Normal

- 显示节点标题。
- 显示主要线标签。
- 子星图显示缩略边界。

#### Far

- 节点变小。
- 子星图变亮点/星团。
- 线变细。
- 标签隐藏。

#### Galaxy

- 节点和子星图主要表现为亮点。
- 线表现为光丝或弱连接。
- 不显示文字。
- 点击/放大后恢复细节。

简化不改变数据。

### 5.3 线粗细

线粗细主要由缩放决定：

```text
screenLineWidth = worldLineWidth * currentScale
```

系统不得把语义重要度默认映射成线粗细。

若未来允许用户手动设置线视觉属性，必须是独立可见的样式设置。

### 5.4 命中测试

远景命中优先命中外层星图对象。

近景命中可以命中更深层 EndpointPath 目标。

命中结果必须返回 EndpointPath，而不是只返回 nodeId。

## 6. 交互路线

### 6.1 创建

创建资源只创建资源：

- createWork
- createStarMap
- createNode
- createChildStarMapPlacement
- createEdge
- createHyperlink

不得在 createWork 中自动创建或绑定 StarMap。

### 6.2 链接与放置

链接和放置是明确动作：

- placeStarMapInStarMap
- createHyperlink
- createEdge
- bindTextToTarget

### 6.3 删除

删除线 = 删除关系。

隐藏线 = 视觉过滤。

删除节点或星图前必须检查：

- 相关 Edge。
- 相关 EndpointPath。
- 相关 Hyperlink。
- 相关 child_starmap placement。
- 相关 proposal。

### 6.4 过滤

过滤是视觉状态，不改变数据。

可过滤：

- 节点类型。
- 线类型。
- 标签。
- hidden flag。
- unresolved 状态。

## 7. 事务与 Undo/Redo

所有星图修改必须走 Core transaction。

一个用户动作对应一个事务：

- 移动节点。
- 移动子星图。
- 新增线。
- 修改线 label。
- 修改 EndpointPath。
- 删除线。
- 隐藏某类线。

事务要能生成：

- changed files
- changed entities
- undo patch
- redo patch
- render invalidation region

## 8. AI proposal 路线

AI 只能写 proposal，不得直接写正式星图。

```text
proposals/
└── ai/<proposal_id>/
    ├── patch.json
    ├── preview/
    └── explanation.md
```

proposal patch 可以包含：

- addNode
- addChildStarMap
- addEdge
- addHyperlink
- updateLabel
- updateNote

用户可：

- 全部接受。
- 局部接受。
- 另存为新星图。
- 丢弃。

接受 proposal 时，由 Core 把 patch 转换为正式 transaction。

## 9. 迁移路线

### 9.1 旧单文件 starmap.json

旧格式读取后迁移为星图包。

迁移步骤：

1. 读取旧 StarMapGraph。
2. 写 graph.json。
3. 拆分 nodes。
4. 拆分 child_starmaps / embeds。
5. 拆分 edges。
6. 拆分 hyperlinks / links。
7. 拆分 layout。
8. 写 metadata/migration.json。
9. 保留旧文件备份或写 manifest 标记。

### 9.2 旧 from/to 字段

旧边字段：

```json
{ "from": "node-a", "to": "node-b" }
```

迁移为：

```json
{
  "fromEndpoint": { "type": "endpointPath", "path": [{ "kind": "node", "id": "node-a" }] },
  "toEndpoint": { "type": "endpointPath", "path": [{ "kind": "node", "id": "node-b" }] }
}
```

## 10. 测试要求

必须新增 Core 测试：

- 创建星图只创建星图，不绑定作品。
- 创建节点只创建节点，不创建线。
- 子星图不是节点。
- 包含关系不产生 edge。
- Hyperlink 不产生 edge。
- Edge endpoint roundtrip 支持 EndpointPath。
- EndpointPath 支持多层子星图。
- EndpointPath 循环检测。
- 删除线就是删除关系。
- 隐藏线不删除关系。
- 缩放简化不改变数据。
- 旧 starmap.json 可迁移为图谱包。
- 单节点修改只改 nodes/<id>.json。
- 单线修改只改 edges/<id>.json。
- 拖动布局只改 layouts/*.json。
- AI proposal 不修改正式星图。
- 接受 proposal 后才修改正式星图。

必须新增前端/桥接测试：

- 前端只拿 render snapshot，不直接读文件。
- 点击远景子星图返回外层 EndpointPath。
- 放大后点击内部节点返回更深 EndpointPath。
- 触摸端超链接不误触发画布连线。

## 11. 实施顺序

### Phase 0：文档与防回归

- 保留 `docs/starmap_canvas_model.md`。
- 本文加入路线。
- `TECHNICAL_ROUTE.md` 引用两份文档。
- 加静态检查，禁止新增 `mind_map` 正式入口。

### Phase 1：类型收口

- 新增 EndpointPath 类型。
- StarMapEdge 新增强制 endpoint path。
- 旧 from/to 标 legacy。
- 新增 child starmap placement 类型。
- Hyperlink 与 Edge 分离。

### Phase 2：星图包存储

- 新增 StarMapPackageStorage。
- load 组装 StarMapDocument。
- save 拆分写文件。
- pretty JSON + 稳定排序。
- 旧格式迁移。

### Phase 3：Core 事务

- 新增 create/update/delete transaction。
- 增加 changed files / changed entities。
- 增加 undo/redo patch。

### Phase 4：Render Snapshot

- Core 输出 StarMapRenderSnapshot。
- 支持缩放简化等级。
- 支持 EndpointPath 可见层级解析。
- 支持 hit test region。

### Phase 5：前端接入

- Linux_qt 和 Android 都只渲染 snapshot。
- 前端事件回传 Core transaction。
- 不在 UI 层维护长期业务状态。

### Phase 6：AI proposal

- AI 生成 proposal patch。
- 用户接受后 Core transaction apply。
- proposal 不污染正式星图。

### Phase 7：本地索引

- 只有在性能需要时加入 SQLite cache。
- SQLite under `.sujian/cache/`。
- 可删除可重建。

## 12. 禁止事项

禁止一边做星图包，一边让前端直接拼业务状态。

禁止用单个大 `starmap.json` 继续承载正式新功能。

禁止把 child StarMap 当 Node 存。

禁止把 containment 当 Edge 存。

禁止把 Hyperlink 当 Edge 存。

禁止把 EndpointPath 降级成 nodeId。

禁止 AI proposal 直接写正式图。

禁止 SQLite 成为真相库。

禁止先做 UI 假效果再补 Core 数据模型。

## 13. 最小可落地版本

最小实现必须包含：

- 独立 StarMap。
- 原子 Node。
- child StarMap placement。
- Edge with EndpointPath。
- Hyperlink 独立于 Edge。
- 星图包存储。
- Core transaction。
- RenderSnapshot。
- 缩放简化等级。
- 旧格式迁移。

没有这些，不算真正落地星图路线。
