# StarMap 视口与语义缩放规范 v1.0

Status: active
Last verified: 2026-10-03
Truth source: product decision / platform display contract
Related: [starmap_semantics.md](starmap_semantics.md)

## 目标

StarMap 的语义结构允许无限嵌套，但平台不需要把无限层级同时展开到屏幕上。

“当前只看到两层”只能是小视口下的视觉结果，不能成为数据层规则。不同手机、折叠屏、平板、桌面窗口，以及同一设备的横竖屏、分屏和自由窗口，都必须根据 **StarMap 当前实际可用 Viewport** 决定显示多少细节。

## Core 与平台边界

### Core 负责

- StarMap、Node、Embed、Link、Edge 的真实语义关系。
- Embed 指向哪个独立 StarMap。
- authored position 和持久化数据。
- CRUD、引用安全、图校验。

Core 不知道以下概念：

- 手机、平板、折叠屏、桌面。
- 横屏、竖屏、分屏。
- “显示两层 / 三层”。
- 当前窗口宽高。
- 当前 camera scale。
- 某个 Embed 此刻是展开、折叠还是不渲染。
- 某个子星图当前是不是视觉焦点。

### 平台显示层负责

- 当前 StarMap 可用 Viewport。
- 全局 camera。
- 每层 Embed 的 local fit。
- 屏幕投影尺寸。
- Visual LOD（expanded / collapsed / hidden）。
- 视觉焦点（focus scene）。
- 命中测试、手势、显示几何和渲染实例生命周期。

这些状态只属于当前客户端视图，不写回 Core。

## Viewport 的定义

Viewport 指 **StarMap 实际可绘制区域**，不是物理屏幕尺寸，也不是设备型号。

顶栏、底栏、安全区或系统窗口占用后的剩余 StarMap 内容区，才是计算依据。

同一台设备只要窗口大小改变，就必须重新计算视觉层级。例如：

- 手机横竖屏切换。
- 折叠屏展开 / 合拢。
- Android / HarmonyOS 分屏或自由窗口。
- Linux_Qt 窗口被拖大或拖小。

不允许用“手机 = 两层、平板 = 三层、桌面 = 四层”作为显示规则。

## 坐标与屏幕投影

递归坐标变换保持以下顺序：

```text
child local
→ parent Embed local fit
→ 更上层 Embed local fit
→ root world
→ global camera
→ screen
```

全局 camera 只有一份。Embed 的 local fit 只负责把子图内容适配进父 Embed，不是独立用户缩放。

某个 Scene 的最终有效缩放可以表示为：

```text
effectiveScale
= globalCameraScale × 所有祖先 EmbedLocalFitScale
```

判断一个 Embed 在屏幕上有多大时，使用 **它所属 Scene 的 effectiveScale**：

```text
projectedEmbedSize
= embedDisplaySize × ownerSceneEffectiveScale
```

不要把这个 Embed 自己内部 child Scene 的 local fit 再乘进外壳尺寸，否则会把“容器有多大”和“容器里的内容缩了多少”混在一起。

屏幕占比统一相对于当前 Viewport：

```text
coverage
= projectedEmbedSize / min(viewportWidth, viewportHeight)
```

具体阈值属于平台显示策略，不进入 Core 数据协议。

## Visual LOD

每个 Embed 在当前视图中只有三种显示状态：

### expanded

- 显示完整 Embed 外壳。
- 实例化并渲染它的 child StarMap Scene。
- child 内容可以继续命中、编辑和递归显示。

### collapsed

- 显示为节点大小附近的子星图摘要。
- 保留标题、选中态和“这是一个子星图”的视觉区别。
- 不实例化 child StarMap Scene。
- 整个摘要区域命中当前 Embed，不把事件继续递归给更深层。

### hidden

- 当前祖先已经 collapsed，或当前视觉策略明确不需要这一层。
- 不创建对应的递归 Scene。
- 数据仍然存在于 Core，只有当前视图没有渲染。

因此“视觉两层”应当理解为：

> 当前 Viewport 下，只有当前视觉焦点附近的少数层处于 expanded；更深层自动 collapsed 或 hidden。

更大的 Viewport 可以自然展开更多层，小 Viewport 则自然减少展开层数。层数不是写死常量。

## 视觉焦点

平台可以维护：

```text
focusScenePath
```

它表示“当前把哪一个 Scene 当作主要视觉参考”。

focus 变化只改变显示策略，不改变图结构：

- 不修改 StarMap 的真实父子 / Embed 引用。
- 不把 child StarMap 重挂成 Core 根。
- 不新开页面。
- 不创建第二套 camera。
- 不写入持久化数据。

当一个子星图在屏幕上的投影已经大到足以成为主要编辑对象时，平台可以提升它为 focus；缩小后再退回父层。

提升和退出应使用不同阈值（hysteresis），避免用户停在临界缩放附近时反复闪烁切换。

## 不同屏幕尺寸的统一规则

各平台都使用同一条产品规则：

> 根据当前实际 Viewport 和屏幕投影尺寸决定 Visual LOD，而不是根据设备身份决定层数。

这意味着：

- 小手机竖屏通常只能完整展开较少层。
- 大手机横屏可能多展开一层。
- 折叠屏展开后可以增加可见细节。
- 平板大窗口可以同时展示更多递归内容。
- Linux_Qt 全屏和窄窗口可以得到不同 LOD 结果。

同一端、同一设备、同一个 StarMap，只要 Viewport 改变，视觉结果就允许改变。

## Viewport 改变时的连续性

窗口尺寸变化时：

- 保持当前 `focusScenePath`。
- 保持用户当前 camera 的观察位置和缩放意图。
- 更新 viewportWidth / viewportHeight。
- 重新计算各 Embed 的屏幕投影和 LOD。
- 原来 collapsed 的层可以自然 expanded，原来 expanded 的层也可以自然 collapsed。

不要因为窗口 resize、旋转、折叠展开就重新打开页面或重置整张星图。

## 渲染与性能

Visual LOD 必须控制真实组件实例数量，而不只是把深层内容画得更小。

当 Embed 为 collapsed 或 hidden 时，不应继续递归创建它下面的完整 Scene 树。

LOD 判断属于高频显示计算，平台应在本地完成；不要为了每一帧缩放跨 UniFFI 调用 Core。

## 跨平台一致性

HarmonyOS、Android、Linux_Qt 需要遵守相同语义：

- 无限嵌套属于数据能力。
- 单一全局 camera 属于视图能力。
- local fit 只负责容器内部适配。
- LOD 由实际 Viewport 和屏幕投影决定。
- focus 只是视觉参考根。
- 深层 Scene 按需实例化。

各平台可以使用不同 UI 框架和实现方式，但不能把设备类型、固定视觉层数或平台专属窗口尺寸写进 Core。

## 禁止路线

不得采用以下实现：

- 在 Core 增加“手机只允许显示两层”之类的字段或规则。
- 按设备型号判断视觉层数。
- 给每个递归 child Scene 建立独立用户 camera。
- 为了 focus 改写真实 StarMap / Embed 关系。
- 所有递归层永远实例化，只靠 scale 把深层内容缩到看不清。
- Viewport 改变后直接重置 camera 或退出当前编辑位置。
