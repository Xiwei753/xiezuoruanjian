# 宽屏 UI 手稿基准

本目录是 Linux_Qt 与 HarmonyOS 宽屏 UI 的视觉基准。这里的四张 PNG 是用户手稿的等比例参考副本，用来约束空间结构、分区关系、展开/收起方式和相对尺寸。实现时不要根据现有手机界面自行发挥。

## 参考图

- `作品页.png`：宽屏作品首页。左侧一级导航，顶部右侧同步 / 搜索 / 设置，主内容区只放作品卡和“+”卡。
- `设置页.png`：宽屏设置。设置是覆盖当前页面的悬浮面板，不跳转成独立全屏页。
- `左右缩回.png`：写作工作台左右面板收起时的骨架。
- `宽屏全打开.png`：写作工作台完全展开时的骨架，左章节树 + 中间正文 + 右工具面板 + 最右工具 rail。

## 硬约束

1. 宽屏禁止底部一级导航。作品 / 星图 / 统计只放左侧纵向一级导航。
2. 不为了 HarmonyOS 的 HDS/沉浸光感保留底栏。侧栏没有对应系统沉浸光感能力时就使用普通系统 token / 自绘选中态。
3. 作品页的“+”是作品网格里的卡片，不是右下角 FAB。
4. 写作页顶部是贯通工具条：左侧返回/撤销/重做，中间字体/段落/替换等写作工具，右侧同步/搜索/设置。
5. 写作页内容区固定为：左章节树 | 中间正文 | 右工具面板 | 最右工具 rail。左右内容面板都可以收起，收起后只留贴边把手/rail。
6. 正文底部只放字数、进度、时间、保存状态等状态信息，不承担页面导航。
7. 设置在宽屏下必须悬浮覆盖在打开设置之前的页面上；关闭后原页面状态和导航栈不变化。
8. Linux_Qt 与 HarmonyOS 使用同一套空间骨架；平台可以各用自己的控件和颜色 token，但不能改变区域关系。
9. 窄屏保留自己的布局，不把宽屏骨架硬塞进手机。
10. 后续调整尺寸、间距、卡片比例时，以本目录 PNG 为准；现有实现只作为功能来源，不作为视觉基准。

## 对应代码

HarmonyOS：
- `apps/harmony/entry/src/main/ets/app/navigation/PrimaryTabShell.ets`
- `apps/harmony/entry/src/main/ets/feature/project/ui/HomeScreen.ets`
- `apps/harmony/entry/src/main/ets/feature/editor/ui/WritingScreen.ets`
- `apps/harmony/entry/src/main/ets/feature/editor/ui/WritingToolRail.ets`
- `apps/harmony/entry/src/main/ets/feature/settings/presentation/SettingsOverlayController.ets`

Linux_Qt：
- `apps/Linux_qt/qml/CreativeHub.qml`
- `apps/Linux_qt/qml/ProjectHomePage.qml`
- `apps/Linux_qt/qml/WritingWorkspace.qml`
- `apps/Linux_qt/qml/WritingToolRail.qml`
- `apps/Linux_qt/qml/SettingsDialog.qml`

对应 Issue：#829。

## 当前骨架

Issue #829 第一轮只把结构钉死：一级导航固定左侧 rail，首页/工作区同步使用宽屏骨架，设置在宽屏下使用两列悬浮布局。写作页继续复用现有七角色 Workbench（顶部三段 + 章节树 + 正文 + 工具面板 + 工具轨），后续只在这些插槽里继续对齐手稿。

## 宽屏判定读哪个 Core 字段

一级导航与宽屏面板（左侧 rail、两列设置浮层）一律读 Core 下发的
`PrimaryNavigationPlacement.Side`，**不**读 `WorkspaceLayoutMode.Workbench`。

反例（`600–839vp` 这一档）：Core 认为这一档已经是 Workbench，但一级导航仍给
`Bottom`。若用 `workspaceLayoutMode` 判「是不是宽屏」，页面会先进入宽屏骨架、
隐藏自己的标题栏，而壳层仍在底部渲染 HDS Tabs —— 于是出现「页面已经是宽屏
骨架，底下却还挂着一个沉浸光感底栏」的双导航。沉浸光感不是布局理由。

反过来 `SettingsDialog.qml` 里 `widePanel`（Side）与 `overlayPanel`（Workbench）
是两个不同概念，不是同一件事的两种写法：`overlayPanel` 决定设置是不是悬浮层，
`widePanel` 决定悬浮层要不要拆两列。不要因为字段名相似就合成一个判定。

平台端不自己按宽度猜档位，也不为同一件事保留第二套判定。
