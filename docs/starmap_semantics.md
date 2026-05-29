# StarMap 语义地基 v1

## 概述

StarMap 不再仅仅是一个简单的“点和边”的通用图结构，也不再是一个强依赖父子拥有权的嵌套树。
它是一个**语义丰富的独立创作视图**。
每个 StarMap 都是独立的文档对象。大星图里出现小星图，是因为大星图 Embed（嵌入）了另一个独立星图的实例，而不是因为它“拥有”那个星图。

## 核心独立模型

在新的架构中，明确区分了三类事物：

### 1. StarMap 本体
* 它是独立的文档对象，拥有自己的 `starmap_id`、meta、graph 和 layout。
* 它不依赖父星图（`parent_starmap_id`）才能存在。`parent_starmap_id` 仅作为兼容或组织的辅助字段，不再作为星图潜入与所有权的主模型。
* StarMap 可以为空（没有 json 文件，或 graph 里没有任何节点），依然是合法的目标。
* 一个 StarMap 可以只包含其他 StarMap 的 Embed，而完全没有自己的普通节点（例如一个“总览宇宙”星图）。

### 2. StarMapEmbed (或 Import)
* 表示当前 StarMap 中嵌入了另一个独立 StarMap 的**一个实例**。
* 它保存的仅仅是 `instance_id`、显示策略、视口坐标等渲染层的信息，**不是**把目标图复制过来。
* 删除一个 Embed，仅仅是删除了这个实例，绝对**不影响目标 StarMap 本体**。
* 同一个目标 StarMap，可以被多个不同的 Host StarMap 多次 Embed。

### 3. StarMapLink
* 表示从当前图的某个端点（节点、锚点或星图本身）指向另一个目标。
* 仅仅用于引用、跳转、打开。它**不会**把目标放进当前画布。
* 删除 Link 对目标 StarMap 也没有任何影响。

## 节点与连接语义模型

### 深层指向 (Deep Target & Path)
为了实现“从宇宙深处一层层放大直到看见微小节点”或者跨图链接的能力，引入了深层目标（`StarMapDeepTarget`）。
这是 StarMap 语义中最核心的基础，允许一个实体（例如节点或连线）稳定指向跨层级目标。

**深层目标包含：**
* `starmap_id`：起点的目标星图。必须真实存在。空图也算存在。
* `path`：一条可选的路径片段（`StarMapPathSegment`），例如连续多次 `EnterChild` / `EnterNode`。
* `target`：最终的目标实体（`StarMapTargetDetail`），支持整个星图、具体节点、特定锚点、章节范围等。

**为什么不能只靠路径字符串解析？**
深层路径解析不能仅仅拼接字符串或者依赖目录结构。解析器必须深入到每个目标星图的 graph 中，真实检验节点和锚点是否存在。星图间的关系是网状的（Embed、Link 都是跨星图引用），简单的字符串路径无法保证目标的语义真实性，也无法阻断死循环（比如 A embed B, B embed A）。因此我们的 Resolver 有 32 层深度限制和循环检测。

### portal (StarMapPortal)
通过 Portal 机制，节点本身可以作为打开或进入目标的“门户”：
* Portal **仅仅表示打开/进入行为**，不代表所有权。
* 它可以指向 Link 或 Embed 的目标，目标存在性检查不再基于简单的目录检查，而是通过底层元数据校验。
* 允许指向一个空图。

## 场景示例

为了更好地理解独立对象和 Embed/Link，请参考以下例子：

1. **新建独立星图**
   * 新建独立 StarMap A。
   * 新建独立 StarMap B。
2. **多重嵌入 (Embed)**
   * A embed B（A的画布上出现了一个B的实例，可以缩放查看）。
   * C 也 embed B（C的画布上也有一个B的实例）。
   * **删除保护**：如果用户在 A 的画布中删除了 B 的 embed 实例，只会在 A 中消失，B 本身完好无损。
3. **仅仅链接 (Link)**
   * A link 到 B。这只是一条线或一个按钮，点击后跳转到 B，但不会把 B 嵌在 A 的画布空间里。
4. **宇宙视角图 (Universe)**
   * 我们可以创建一个 Universe StarMap，它里面没有任何普通的 Note/Character 节点，它仅仅包含 10 个 StarMapEmbed。这是完全合法且常见的高级视图用法。

## 目标展示状态 (Target Display Status)
深层目标在不同的缩放层级（Scale）下会有不同的展开细节，Core 提供纯函数 `resolve_target_display_status` 根据 `StarMapDisplayPolicy` 返回当前的展示状态，让 UI 知道如何渲染它：
* `Unresolved`：目标不存在、路径死循环或暂时不可解析。
* `TitleOnly`：最小比例，远处只看到一个小圆点和名称。
* `TitleSummary`：稍近处，显示大标题和一段文字摘要。
* `MiniMap`：再近一点，可以预览子星图的缩略图结构。
* `ExpandedGraph`：足够近或通过 Portal 进入后，展示真实的完整子图。

## Legacy 兼容
- **`parent_starmap_id`**：继续存在，但仅仅作为老版“子星图”接口的组织归属字段，不被核心解析器视为唯一引用路径。
- **Payload**：节点中的 `payload` 字段依然保留作为自定义扩展，但不承载 StarMap 主关系模型。
