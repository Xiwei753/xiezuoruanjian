# StarMap 语义地基 v1.1 (独立对象与引用安全)

## 概述

StarMap 不再是一个强依赖父子拥有权的嵌套树。它是一个**语义丰富的独立创作视图**。
每个 StarMap 都是独立的文档对象。大星图里出现小星图，是因为大星图 Embed（嵌入）了另一个独立星图的实例，而不是因为“拥有”它。

## 核心独立模型与 CRUD

### 1. StarMap 本体
* 它是独立的文档对象，拥有自己的 `starmap_id`、meta、graph 和 layout。
* 它不依赖父星图（`parent_starmap_id`）才能存在。`parent_starmap_id` 仅作为遗留的兼容组织字段。
* **删除安全 (Deletion Safety)**：删除 StarMap 时，底层现在会进行深度引用检测 (`find_starmap_references`)。如果该星图被其他星图通过 Embed、Link、Portal 或 DeepTarget Edge 引用，底层将**拒绝删除**并返回引用报告。**不再存在静默的级联删除行为**。如果需要遗留的强制连带删除，必须调用专门的 `delete_starmap_cascade_legacy`。

### 2. StarMapEmbed (嵌入实例)
* 表示当前 StarMap 中嵌入了另一个独立 StarMap 的一个显示实例。
* **正式 CRUD API**：`add_starmap_embed`、`update_starmap_embed`、`delete_starmap_embed`。
* 删除一个 Embed，仅仅是从当前画布上移除了这个实例，绝对**不影响目标 StarMap 本体**。
* 同一个目标 StarMap，可以被多个不同的 Host StarMap 多次 Embed。

### 3. StarMapLink (跳转链接)
* 表示从当前图的某个端点（节点、锚点或星图本身）指向另一个目标。仅仅用于引用、跳转、打开，不会把目标放进当前画布。
* **正式 CRUD API**：`add_starmap_link`、`update_starmap_link`、`delete_starmap_link`。
* 删除 Link 对目标 StarMap 也没有任何影响。

## 节点与连接语义模型

### 深层指向 (Deep Target & Path)
允许一个实体稳定指向跨层级目标，即使目标处于另一个被嵌入的星图深处。
* **深度引用扫描**：系统的 `find_starmap_references` 不仅会扫描直接指向 `starmap_id` 的目标，还会扫描 DeepTarget 的 `path` 数组。如果路径经过了某个星图（例如 `EnterChild`），该星图也会被视为正在被引用。

### Semantic Edge (语义边)
* 边 (`StarMapEdge`) 不再强制要求 `to` 和 `from` 必须是当前图里的实际节点 ID。
* 只要边提供了合法的深层目标 (`to_target` 或 `from_target`)，它就可以指向外部的节点或星图，而不需要在当前图中捏造一个虚拟的 dummy 节点。

## 场景示例

1. **多重嵌入与安全删除**
   * A embed B，C 也 embed B。
   * 如果用户试图直接删除 B 本体，系统会拒绝，并报告 B 正在被 A 和 C 引用。
   * 用户必须先去 A 和 C 里删除对 B 的 Embed 实例，才能删除 B。这防止了“悬空引用”导致的整个关系网崩溃。
2. **仅仅链接 (Link)**
   * A link 到 B。这只是一条线或一个按钮，点击后跳转到 B。
3. **宇宙视角图 (Universe)**
   * 创建一个 Universe StarMap，里面没有任何普通的 Note 节点，仅仅包含十个 StarMapEmbed。完全合法。

## 严格图校验 (validate_graph)
由于引入了第一公民级别的 Embed 和 Link，图保存时会进行以下严格校验：
* **重复性**：同图不能有重复的 `instance_id` 或 `link_id`。
* **目标存活**：Embed 指向的星图必须存在。
* **坐标安全**：Embed `viewport` 内的缩放与坐标不能出现 NaN，且 scale 必须大于 0。
* **源端点合法**：Link 如果从节点或锚点出发，该节点/锚点必须真实存在于当前图中。
* **深层可达性**：Link 的目标深层路径不仅不能指向虚无，而且不能发生环路（CycleDetected）或者层级太深（TooDeep）。
