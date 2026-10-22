# StarMap 语义地基 v2.0 (独立画布与跨图导航)

Status: active
Last verified: 2026-10-16
Truth source: product decision / protocol
Supersedes: v1.1 (2026-06-11)

## 概述

每张星图都是**独立的无限二维画布**。子星图在父星图中只是指向另一张画布的入口（Embed），用户将来通过连续缩放进入、缩小退出。跨父子及兄弟星图连线连接真实节点对象（通过 `StarMapTargetPath`），而不是连接两个屏幕坐标。

StarMap 不再是一个强依赖父子拥有权的嵌套树，也不是"父画布里递归塞满子画布内容"的容器。它是一个**语义丰富的独立创作视图**。每个 StarMap 都是独立的文档对象，拥有自己的无限画布空间。

## 核心独立模型与 CRUD

### 1. StarMap 本体
* 它是独立的文档对象，拥有自己的 `starmap_id`、meta 和 graph。对象的 authored 位置（`StarMapNode.position`、`StarMapEmbed.position`）与样式（`StarMapNode.style`）都在 graph 里的对象自身上；布局算法、视口、运动策略、显示策略、命中测试、边渲染几何全部属于平台显示层，不在 Core。
* **每张画布只保存自己节点与 Embed 的 authored 文档坐标**。不存在"父画布无限放大后把子画布全部铺进去"的坐标链。
* 它不依赖父星图（`parent_starmap_id`）才能存在。`parent_starmap_id` 仅作为遗留的兼容组织字段。
* **删除安全 (Deletion Safety)**：删除 StarMap 时，底层会进行深度引用检测 (`find_starmap_references`)。如果该星图被其他星图通过 Embed、Link、Portal 或跨层 Edge 引用，底层将**拒绝删除**并返回引用报告。**不再存在静默的级联删除行为**。

### 2. StarMapEmbed (嵌入实例 = 入口)
* Embed 是**入口实例**，不是物理子画布尺寸约束。当前星图里放置一个 Embed，表示"这里有一个通往另一张独立画布的入口"。
* **没有物理父子画布尺寸约束**：父画布不需要为子画布预留空间，子画布也不需要适配父画布的显示区域。
* 同一个目标 StarMap，可以被多个不同的 Host StarMap 多次 Embed，每个入口路径独立。
* **正式 CRUD API**：`add_starmap_embed`、`update_starmap_embed`、`delete_starmap_embed`。
* 删除一个 Embed，仅仅是从当前画布上移除了这个入口实例，绝对**不影响目标 StarMap 本体**。

### 3. StarMapLink (跳转链接)
* Link = **跳转**。表示从当前图的某个端点指向另一个目标，仅用于引用、跳转、打开，不会把目标放进当前画布。
* **正式 CRUD API**：`add_starmap_link`、`update_starmap_link`、`delete_starmap_link`。
* 删除 Link 对目标 StarMap 也没有任何影响。

### 4. StarMapEdge (语义连线)
* Edge = **语义连线**。连接的是真实节点对象（通过 `StarMapTargetPath`），不是连接两个屏幕坐标。
* 边的端点使用 `StarMapTargetPath`，支持跨星图层级引用——从宿主图出发，经过 Embed/Portal 路径段，到达目标节点。
* Edge 的宿主图是两端 route 的**最长公共实例路径前缀**对应的星图。同层、父子、不同子分支均适用。
* 跨**不同根星图**、无共同入口链时明确返回 `NoCommonHost`，不能伪造宿主关系。

## 跨画布导航

### 导航路由 (StarMapNavigationRoute)

导航路由描述从根图到当前画布的语义路径，仅承载**语义位置**，不保存镜头：

```
StarMapNavigationRoute {
    root_starmap_id: String,       // 导航起点（根星图 ID）
    segments: Vec<StarMapPathSegment>, // EnterEmbed(instance_id) / EnterPortal(node_id)
}
```

### 导航操作

* **`resolve_navigation_route`**：解析当前所在画布 ID 和经过的画布列表。
* **`enter_embed(route, instance_id)`**：通过 Embed 入口进入子画布。在当前图确认实例存在后追加路径段。
* **`enter_portal(route, node_id)`**：通过 Portal 节点进入子画布。
* **`exit_canvas(route)`**：弹出最后一个段，返回上一层。对同一目标星图的多个 Embed，返回的是**原进入实例**。
* **`jump_to_ancestor(route, level)`**：跳到任意祖先层级（纯路径截断）。

路径环、缺失实例、超过 32 层限制，沿用现有明确错误类型。

上述导航操作通过统一 Core API 导出：`resolve_starmap_navigation` 解析当前画布，
`enter_starmap_embed` / `enter_starmap_portal` 进入子画布，`exit_starmap_canvas`
返回上一层，`jump_starmap_ancestor` 跳到任意祖先层级。返回层级同样只在 Core
内做路径截断，平台端不自行弹段以免丢失原始进入实例。

## 跨画布连线

### 端点选择 (StarMapEndpointSelection)

```
StarMapEndpointSelection {
    route: StarMapNavigationRoute,  // 导航路径
    target: StarMapTargetDetail,    // Node / Anchor / Starmap
}
```

### 连线规划 (plan_cross_canvas_edge)

算法：
1. 分别解析两端 route 与本地目标，确认对象存在。
2. 找到两个 route 的**最长公共实例路径前缀**，对应的星图就是这条 Edge 的宿主图。
3. 从公共宿主图切去公共前缀，将余下路径段作为 `from/to.segments`，两条路径的 `starmap_id` 都填 Edge 宿主图 ID。
4. 写入**唯一一条真实 StarMapEdge**到宿主图，Edge ID 由 Core 生成。
5. 跨不同根星图、无共同入口链时返回 `NoCommonHost`。

示例：根图 A 有节点 a、Embed e1→B、Embed e2→C，B 有节点 b、C 有节点 c。
* a→b：Edge 放 A；from = `A/[]/Node(a)`，to = `A/[e1]/Node(b)`。
* b→c：Edge 放 A；from = `A/[e1]/Node(b)`，to = `A/[e2]/Node(c)`。
* B 内部 b→b2：Edge 放 B，走现有本地边逻辑。

## 关联反查

统一入口是 `list_starmap_connections(route, target)`：`target = Some(t)` 时返回指向 `t` 的连接，`target = None` 时返回当前画布可见的所有连线。两种查询都返回 `{ owner_starmap_id, edge_id, from, to, kind, label }`，既包括本图 Edge，也包括**存于祖先图但至少一端 route-aware 落在当前画布**的 Edge。前端不应为了看当前层连线，自己递归读取所有祖先 graph 再猜路径。

### 派生反向关系索引

`relation_index.rs` 只索引本地图引用，不能把深层 Node ID 错当作宿主图本地 Node ID。保持这个不变量，另建立按解析后目标 `(starmap_id, object_id)` 查询的**派生反向关系索引**。真相仍是各图 `StarMapEdge`，索引由现有数据构建，关系写入/修改/删除后增量更新。`list_starmap_connections` 从该索引收集候选祖先图，再用统一的 route-aware 端点匹配（比较完整实例路径后缀，区分 `e1→B`、`e2→B` 等不同进入实例）做精确过滤；不再保留逐 Edge 加载所有祖先图的第二套扫描实现。

## 节点与连接语义模型

### 深层指向 (Deep Target & Path)
允许一个实体稳定指向跨层级目标，即使目标处于另一个被嵌入的星图深处。
* **深度引用扫描**：系统的 `find_starmap_references` 不仅会扫描直接指向 `starmap_id` 的目标，还会扫描路径段经过的星图。

### Semantic Edge (语义边)
* 边 (`StarMapEdge`) 不再强制要求 `to` 和 `from` 必须是当前图里的实际节点 ID。
* 只要边提供了合法的 `StarMapTargetPath`，它就可以指向外部的节点或星图，而不需要在当前图中捏造一个虚拟节点。

## 场景示例

1. **多重嵌入与安全删除**
   * A embed B，C 也 embed B。
   * 如果用户试图直接删除 B 本体，系统会拒绝，并报告 B 正在被 A 和 C 引用。
   * 用户必须先去 A 和 C 里删除对 B 的 Embed 实例，才能删除 B。
2. **仅仅链接 (Link)**
   * A link 到 B。这只是一个跳转入口，点击后跳转到 B。
3. **宇宙视角图 (Universe)**
   * 创建一个 Universe StarMap，里面没有任何普通的 Note 节点，仅仅包含十个 StarMapEmbed。完全合法。
4. **跨画布连线**
   * 根图 A 有 Embed e1→B 和 Embed e2→C。B 中节点 b 与 C 中节点 c 的 Edge 存放在 A（公共宿主图），from 路径穿越 e1，to 路径穿越 e2。
5. **跨根星图无共同宿主**
   * 根图 A 的节点 a 与根图 D 的节点 d 之间无法创建 Edge，因为它们没有共同入口链。系统返回 `NoCommonHost`。

## 严格图校验 (validate_graph)
* **重复性**：同图不能有重复的 `instance_id` 或 `link_id`。
* **目标存活**：Embed 指向的星图必须存在。不允许 Self-Embed。
* **端点安全**：Edge、Link、Embed 都使用 `StarMapTargetPath`。如果端点是 Node/Anchor 且路径无 segments，则它们必须存在于当前 Host StarMap 中。
* **位置合法**：节点的 `position`、嵌入的 `position` 数值必须 finite（无 NaN/Inf）。坐标精度为 f64。
* **深层可达性**：Link/Edge 的目标深层路径不能指向虚无，不能发生环路（CycleDetected）或层级太深（TooDeep）。
* **级联清理**：调用 `delete_starmap_node` 删除节点时，会自动级联清理连接到该节点的本地 Edge、Embed 和 Link。
* **跨图引用检查**：删除目标节点、Embed 入口或 Portal 之前，使用反查识别**其他 graph 持有的跨层引用**：有跨图关联时**拒绝破坏性删除**并明确返回引用列表。不能只扫描本图导致祖先 Edge 变悬空。更新 Portal 目标等能改变路径解析结果的操作也须做跨图引用检查。
* **删除保护**：调用 `delete_starmap` 时，仅当存在**外部引用**（被其他星图引用）时才阻止删除。

## 相关显示规范

StarMap 的视口协议、Embed 入口标识的显示策略见 [starmap_viewport.md](starmap_viewport.md)。普通节点的名称/正文双字段、卡片呈现、预览折叠与编辑行为见 [starmap_node_presentation.md](starmap_node_presentation.md)。这些都属于平台显示层，不改变本文定义的 Core 语义对象与引用关系。镜头位置、比例、阈值、动画和矢量绘制**仍归各平台**，不要回写 Core。
