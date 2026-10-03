# 星图视口与 Deep Zoom

## 目标

Core 的星图 / 子星图是**无限层级**的树。视图层要做的是同一棵树的三件事：

1. 把任意一层的全部内容平铺进父 Scene 的可见区域（每层一次局部适配）。
2. 让用户用一台全局相机自由缩放、平移、聚焦任意一层。
3. 让"再往里一层"在任何缩放下都**看得见、点得到、认得出还是一颗子星图**。

第 3 条是本文档的主题。#821 之前它是用"换形状"实现的（展开的圆 / 折叠的矩形摘要卡），
代价是跨阈值时 Embed 在父 Scene 里的显示矩形会跳、子子星图失去"这是星图"的身份、
语义缩放被布局变化污染。#821 改成真正的 Deep Zoom：**只降细节，不改几何**。

## Core 与平台边界

- 子星图的嵌入关系、节点/边的 authored position、层级关系全部在 Core。
- 平台层只负责：布局适配（把内容铺进可用区）、全局相机、Deep Zoom 细节分级、命中与手势。
- 平台层不回写任何布局数据。缩放、平移、掉档都**不允许**改变 authored position。
- 分辨率、屏幕尺寸、是否刘海、是否折叠屏，与"某颗 Embed 该画多少细节"的关系只有一条：
  它通过投影覆盖率间接影响，不直接决定档位。

## 视口的定义

两个概念必须分开，之前混在一起是 #821 要修的主要问题：

| 名称 | 含义 | 谁在改 |
| --- | --- | --- |
| **world 几何** | 对象在它所属那张星图里的 authored 矩形（本地 vp） | 只有 Core / 用户显式创建或移动对象时 |
| **相机与局部适配** | world 几何怎么被搬到屏幕上 | 用户手势 / 每一层的局部 fit |

### 坐标链

```
稳定 world geometry
  → 每层 local fit (fitScale, fitOffset) 逐层向上
  → root world（根 Scene 画布坐标）
  → global camera (cameraScale, cameraOffset)
  → screen
```

其中：

- `effectiveScale(Scene) = globalCameraScale × 所有祖先 EmbedLocalFitScale`
- 一层 Scene 里的对象，其**屏幕尺寸** = `world 尺寸 × effectiveScale(所属 Scene)`

### 覆盖率

```
projectedDiameterVp = DEFAULT_EMBED_DIAMETER × ownerEffectiveScale
coverage            = projectedDiameterVp / min(rootViewportWidth, rootViewportHeight)
```

`coverage` 是唯一的档位输入。它只看"这颗 Embed 在屏幕上到底多大"，
不看设备类型、不看屏幕尺寸、不看绝对 vp 数。

## Deep Zoom 细节分级

三档，只控制**渲染多少细节**和**是否挂载完整可交互组件**：

| 档位 | 条件 | 内部渲染器 | 可交互 |
| --- | --- | --- | --- |
| `interactive` | `coverage ≥ 0.70` | `StarMapEmbedScene`（递归 child Scene） | 是 |
| `preview` | 投影直径 `≥ 48vp`（焦点链上保底） | `StarMapEmbedPreview`（固定尺寸 Canvas 缩略图） | 否 |
| `shell` | 更小 | 无（只画圆壳） | 否 |

滞回带：进入 `interactive` 用 `0.70`，退出用 `0.60`；`preview` 进用 `48vp`，出用 `40vp`。
中间那段不动，避免在阈值附近抖动。

**硬约束（#821）：分级绝不改变 Embed 的形状、world bounds 或在父 Scene 里的 authored position。**

- 子星图在任何缩放下都是正圆，尺寸恒为 `DEFAULT_EMBED_DIAMETER × DEFAULT_EMBED_DIAMETER`，
  圆角 = 半径。永远没有矩形摘要卡。
- 掉档不重建任何布局矩形，不重算 fit，不写 Core。
- 缩放只走显示变换（`.scale()`），不许把缩放乘进 `.width()/.height()/.fontSize()`。
- `preview` 整块 `HitTestMode.None`：它只回答"里面有什么"，不参与触摸竞争，
  根 Scene 的两指 Pinch 必须完整穿过它。

## 显示变换而不是布局增长

因为所有缩放都发生在**布局之后**，每个 Scene 有两个坐标口径：

- **box 坐标**：本 Scene 自己的盒子。对象布局尺寸恒为本地基准常量，
  `.scale(boxScale)` + `.position()` 的中心补偿产生视觉位置。
- **累计屏幕坐标**：box 坐标 × `parentScale()`。几何、命中、注册表只用这一种。

ArkUI 的 `.scale()` 以组件中心缩放，所以视觉左上角 = `canvas × boxScale + boxOffset`
对应到组件 `.position()` 时必须补上 `base × (boxScale − 1) / 2`。

对象在屏幕上显示多粗、多大，只取决于 `boxScale` 逐层连乘，等价于旧的 `effectiveScale`。
这条不变式是 #821 的核心：显示结果和 #820 一样，但组件布局尺寸永远是本地常量，
因此相机放到几十倍也不会让 RenderService 分配巨型缓冲。

## 视觉焦点

焦点（`focusScenePath`）是"用户当前最关心的那一层"，由覆盖率 + 圆心位置双条件驱动，
带滞回。它只影响两件事：是否保底 `preview`、以及递归交互时的优先层。
焦点从不改变任何几何。

"手机两层 / 平板三层"是深度的**结果**，不是常量：层级可见数由各层投影直径是否越过
阈值自然决定。

## 不同屏幕尺寸

- 屏幕变大变小只改 `min(viewportWidth, viewportHeight)`，即只改 coverage 的分母。
- 适配尺寸只影响初始 `fitScale` 和 `fitOffset`，属于"看一眼全貌"的起点。
- 屏幕尺寸变化不重排 authored position，不重建 Embed 矩形（#821）。

## 缩放后的重叠

正常相机缩放**不触发任何碰撞规避、不推开邻居**。

- 稳定 world 几何 → 均匀相机变换 → 同层对象同比缩放。这才是"缩放"的语义。
- 只有当用户真的创建、移动、缩放了某个对象的 world bounds 时，才走局部碰撞规避 / 空位选择。
- 缩放不是布局变化。

## 相机范围

- `CAMERA_SCALE_MIN = 1e-4`，`CAMERA_SCALE_MAX = 1e5`：数值安全边界，不是产品上限。
- 用户不该感觉到一个硬天花板。#820 的 `3` 是被"组件布局尺寸 × 缩放"逼出来的，
  显示变换改造之后它没有理由存在。
- 工具栏用**乘法**步进（`ZOOM_FACTOR = 1.2`），保证各档手感一致。
- 双指捏合保持连续比例，并保持双指中心下的 world 锚点不动。

## 渲染与性能

- 三档的递归深度上限由 `interactive` 档决定；`preview` 不创建 child Scene，
  也不注册 Scene handle，所以深层内容的开销是真的降下来了。
- `preview` 用 Canvas 一次性画出子图的节点 / 边 / 子 Embed 圆，
  不为每个子节点创建 ArkUI 组件，不挂任何手势。
- 边和文字的线宽 / 字号在 Canvas 里除掉累计缩放，保持屏幕上的恒定粗细。

## 跨平台一致性

- Core 侧的 Embed 语义、层数、authored position 在所有平台完全一致。
- 覆盖率公式、三档阈值、滞回带是平台层共享常量，不允许各平台各写一套。
- 平台层不得新增"折叠态矩形卡片"这类与 Android / Linux 不一致的表现。

## 禁止路线

- ❌ 用 LOD 改变对象几何（换形状、换尺寸、换 authored position）。
- ❌ 把缩放乘进 `.width()/.height()/.fontSize()`（导致 RenderService 巨型缓冲）。
- ❌ 缩放时跑碰撞规避或推开邻居（缩放不是布局变化）。
- ❌ `opacity(0)` 假装折叠 / "先建好再缩到看不见"。
- ❌ 在 Rust 内部把 DTO 转 JSON 再解析回来。
- ❌ Core 里出现任何视口、相机、投影、缩放相关的字段。
