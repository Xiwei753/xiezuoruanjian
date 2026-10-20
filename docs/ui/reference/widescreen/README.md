# 宽屏 UI 手稿基准

本目录是 Linux_Qt 桌面 UI 的视觉基准。这里的 PNG 是用户手稿的等比例参考副本，用来约束空间结构、分区关系、展开/收起方式和相对尺寸。实现时不要根据现有手机界面自行发挥。

## 参考图

宽屏骨架（Issue #829）：

- `作品页.png`：宽屏作品首页。左侧一级导航，顶部右侧同步 / 搜索 / 设置，主内容区只放作品卡和“+”卡。
- `设置页.png`：宽屏设置。设置是覆盖当前页面的悬浮面板，不跳转成独立全屏页。
- `左右缩回.png`：写作工作台左右面板收起时的骨架。
- `宽屏全打开.png`：写作工作台完全展开时的骨架，左章节树 + 中间正文 + 右工具面板 + 最右工具 rail。

统计页（Issue #946，Linux Qt 桌面基准）：

- `统计页_202610181553_29245.png`：统计页宽屏总览。顶部生涯条（累积人工码字、首末记录日期、平均速度、平均每日字数、已持续自然日历天数，标注本机记录），下方近七天 / 近三十天 / 近半年 / 自选区间四卡横排，底部“素笺写作已陪伴您 N 天”。
- `近七天_202610181553_29244.png`：时段卡原位展开。四卡区域原位替换为“左摘要 / 中纪录（最多码字日、最快码字、最长码字、连续码字）/ 右趋势图”，可返回四卡总览。
- `统计页生涯_202610181553_29243.png`：生涯原位展开。顶部生涯条展开为“左纪录 / 右趋势图”，四卡隐藏，底部陪伴条保留。
- `手机统计七天_202610181553_29245.png`、`手机统计_202610181553_29246.png`：窄屏统计单页。范围切换 → 总量与区间 → 2×2 纪录 → 趋势图，纵向滚动。

两张“手机统计”图保留作移动端历史参考，只约束 Android/HarmonyOS 的真实移动设备布局，不用于 Linux Qt 的窗口缩放策略。

以上手稿里的数字全部是示意，实现不得写死具体数值。

## 硬约束

1. 宽屏禁止底部一级导航。作品 / 星图 / 统计只放左侧纵向一级导航。
2. 不为了 HarmonyOS 的 HDS/沉浸光感保留底栏。侧栏没有对应系统沉浸光感能力时就使用普通系统 token / 自绘选中态。
3. 作品页的“+”是作品网格里的卡片，不是右下角 FAB。
4. 写作页顶部是贯通工具条：左侧返回/撤销/重做，中间字体/段落/替换等写作工具，右侧同步/搜索/设置。
5. 写作页内容区固定为：左章节树 | 中间正文 | 右工具面板 | 最右工具 rail。左右内容面板都可以收起，收起后只留贴边把手/rail。
6. 正文底部只放字数、进度、时间、保存状态等状态信息，不承担页面导航。
7. 设置在宽屏下必须悬浮覆盖在打开设置之前的页面上；关闭后原页面状态和导航栈不变化。
8. Linux Qt 始终使用本目录规定的桌面宽屏骨架。HarmonyOS/Android 的多布局由各自终端设备形态和可显示区域决定，不反向影响 Linux Qt。
9. Linux Qt 窗口变窄时只折叠次要面板、隐藏次要信息或局部滚动；不按窗口宽度切换手机导航、单列统计页或单栏写作工作台。
10. 后续调整尺寸、间距、卡片比例时，以本目录 PNG 为准；现有实现只作为功能来源，不作为视觉基准。
11. 统计页宽屏总览固定为“生涯条 + 四卡横排”；时段/生涯都是原位展开/收回，不跳新页、不弹第二层页面。
12. 统计趋势图是按业务日期聚合的人工码字量（单位：字），与按字/分绘制的本周速度曲线语义不同，两者不互相换标题冒用；速度曲线只能放在次级明细。
13. Linux Qt 统计页固定使用生涯条、四卡横排和原位三栏展开；可用宽度不足时保留卡片可读最小宽度并让该区域横向滚动，不改变卡片列数或切成手机统计页。
14. 统计页在左侧一级导航与顶部工具栏之外横向填满主画布，不使用 Hub 的限宽阅读栏。新增的每日码字日历热力图位于手稿既有“生涯条 + 四卡横排 / 原位展开 + 陪伴条”结构之外，按主内容纵向流排布，不改变原有区域关系。

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
- `apps/Linux_qt/qml/SyncConflictPanel.qml`
- `apps/Linux_qt/qml/WritingToolRail.qml`
- `apps/Linux_qt/qml/SettingsDialog.qml`
- `apps/Linux_qt/qml/StatsPreviewPage.qml`（统计页主体：生涯条 + 四卡 + 原位展开）
- `apps/Linux_qt/qml/StatsPeriodCard.qml`、`StatsRecordPanel.qml`、`StatsTrendChart.qml`（统计页组件）
- `apps/Linux_qt/qml/StatsCalendarHeatmap.qml`（每日码字日历热力图，Issue #960）

对应 Issue：#829、#946、#960、#971。

## 当前骨架

Issue #829 第一轮只把结构钉死：一级导航固定左侧 rail，首页/工作区同步使用宽屏骨架，设置在宽屏下使用两列悬浮布局。写作页继续复用现有七角色 Workbench（顶部三段 + 章节树 + 正文 + 工具面板 + 工具轨），后续只在这些插槽里继续对齐手稿。

Issue #946 按统计页手稿重做统计主体：顶部“生涯 + 四卡横排”总览；时段卡/生涯条原位展开纪录与趋势图。作品 / 章节 / 设备排行与本周速度曲线保留为可折叠“详细统计”，数据口径与 Explorer 时段分开显示。

Issue #960 让统计主体填满导航与顶栏之外的主画布，并按手稿恢复生涯条、四卡横排与原位展开的比例；在此结构下方新增独立的每日码字日历热力图（52/53 周 × 7 天，按 Core 每日人工输入真值分级着色），按字数的趋势折线图继续保留。

## HarmonyOS 宽屏判定读哪个 Core 字段

本节只适用于 HarmonyOS 的设备适配。Linux Qt 始终使用桌面骨架，不读取 Core 布局字段来按窗口宽度切换导航或工作区外壳。

HarmonyOS 一级导航与宽屏面板（左侧 rail、两列设置浮层）一律读 Core 下发的
`PrimaryNavigationPlacement.Side`，**不**读 `WorkspaceLayoutMode.Workbench`。

反例（`600–839vp` 这一档）：Core 认为这一档已经是 Workbench，但一级导航仍给
`Bottom`。若用 `workspaceLayoutMode` 判「是不是宽屏」，页面会先进入宽屏骨架、
隐藏自己的标题栏，而壳层仍在底部渲染 HDS Tabs —— 于是出现「页面已经是宽屏
骨架，底下却还挂着一个沉浸光感底栏」的双导航。沉浸光感不是布局理由。

反过来 `SettingsDialog.qml` 里 `widePanel`（Side）与 `overlayPanel`（Workbench）
是两个不同概念，不是同一件事的两种写法：`overlayPanel` 决定设置是不是悬浮层，
`widePanel` 决定悬浮层要不要拆两列。不要因为字段名相似就合成一个判定。

HarmonyOS 根据实际设备形态和可显示区域适配，不为同一件事保留第二套判定；这些适配规则不控制 Linux Qt。
