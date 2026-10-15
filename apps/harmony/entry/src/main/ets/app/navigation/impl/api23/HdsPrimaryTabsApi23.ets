// HdsPrimaryTabsApi23 — API23+ 一级底部导航壳层，使用 HdsTabs 悬浮页签。
// Issue #768：API23+ 改用 UI Design Kit 的 HdsTabs 替代普通 Tabs + BottomTabBarStyle。
// Issue #768 评论5835171266 第1项：不再自己创建 NavPathStack、不注册 coordinator、
// 不创建 @Link @Watch currentTab、不 @Provide activeStack。
// 三个 NavPathStack 由父组件 PrimaryTabShell 唯一持有并传入，currentTab 通过 @Link 双向绑定
// 回父组件（不加 @Watch，不写 coordinator），切 tab 时只有 PrimaryTabShell 的 @Watch 触发 syncActiveTab。
//
// Issue #768 评论5835171266 第3项：HdsTabs 加 barFloatingStyle + systemMaterialEffect(IMMERSIVE)
// 实现真正的悬浮沉浸光感效果；Navigation 改为 HdsNavigation。
//
// Issue #774 评论5847359998：单棵树改造——只保留一棵 HdsTabs，不再因底栏显隐重建整棵页面树。
// 底栏显隐通过 controller.applyHideAnimation/applyShowAnimation 控制，不销毁 TabContent。

import { Routes } from '../../Routes'
import { ScreenRole } from '../../../../corebridge/dto/PlatformDtos'
import { PrimaryActionChannel } from '../../PrimaryActionChannel'
import hilog from '@ohos.hilog'
import { HomeScreen } from '../../../../feature/project/ui/HomeScreen'
import { WorkspaceScreen } from '../../../../feature/project/ui/WorkspaceScreen'
import { WritingScreen } from '../../../../feature/editor/ui/WritingScreen'
import { StarMapScreen } from '../../../../feature/starmap/ui/StarMapScreen'
import { StarMapListScreen } from '../../../../feature/starmap/ui/StarMapListScreen'
import { SettingsScreen } from '../../../../feature/settings/ui/SettingsScreen'
import { StatsScreen } from '../../../../feature/stats/ui/StatsScreen'
import {
  HdsTabs,
  HdsNavigation,
  HdsTabsFloatingStyle,
  HdsTabsController,
  HdsBarStyle,
  HdsTabsBarChangeMode,
  BarStyleChangeCallback,
  HdsAnimationMode,
  hdsMaterial,
} from '@kit.UIDesignKit'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

// Issue #774 评论5847359998：悬浮栏底部固定设计间距（vp）。
// 不再用 safeBottomVp（系统导航指示条避让高度），那是另一个语义。
const FLOATING_BAR_BOTTOM_MARGIN: number = 28

@Component
export struct HdsPrimaryTabsApi23 {
  // Issue #768 评论5835171266 第1项：三个 NavPathStack 由父组件 PrimaryTabShell 传入，
  // 本组件不创建自己的 stack，不注册 coordinator。
  // 使用普通变量传递（非 @Prop），因为 NavPathStack 是引用类型，@Prop 会深拷贝导致
  // 子组件操作的是副本而非原始 stack。普通变量传递引用，Navigation(stack) 直接使用同一实例。
  homeStack: NavPathStack = new NavPathStack()
  starMapStack: NavPathStack = new NavPathStack()
  statsStack: NavPathStack = new NavPathStack()

  // Issue #768 评论5835171266 第1项：保留 @Link currentTab（用于 HdsTabs 的 index 和 onChange 写入），
  // 但不加 @Watch、不调 syncActiveTab、不写 coordinator。
  // HdsTabs.onChange 写 this.currentTab = index → @Link 双向绑定到父组件 →
  // 父组件 PrimaryTabShell 的 @Watch 触发 syncActiveTab → 只有一处写 coordinator。
  @Link currentTab: number

  // 来自 AdaptiveState 的底部安全区域高度（vp）。
  // Issue #773 评论5844518092 第2项：不再用于 barBottomMargin（改用固定设计值 FLOATING_BAR_BOTTOM_MARGIN）；
  // safeBottomVp 保留是因为 PrimaryTabShell 仍传入，后续若无人消费可移除。
  @Prop safeBottomVp: number = 0

  // 是否显示一级导航栏，由外部 ScreenPolicy 决定。
  // Issue #774 评论5847359998：加 @Watch 回调，通过 controller.applyHideAnimation/applyShowAnimation
  // 控制底栏显隐，不再靠 if/else 两套 HdsTabs 重建整棵页面树。
  @Prop @Watch('onShowPrimaryNavChange') currentShowPrimaryNav: boolean = true

  // Issue #774 评论5847638284 第2项：交互锁——异步查 policy 期间禁止切 tab，
  // 但不触发 @Watch 导致底栏无意义 hide→show。与视觉显示分开。
  @Prop currentPrimaryNavInteractive: boolean = true

  // Issue #906：用户点击一级页签时通知父组件登记 nav.tab_requested（User）。
  // 与 onChange（只提交 currentTab，程序化切换也会触发）区分，避免把恢复伪装成点击。
  onUserTabClick: (index: number) => void = (): void => {}

  // Issue #905：当前屏幕 ScreenRole（由 PrimaryTabShell 从 NavigationChromeStore 传入）。
  // 决定原生 MiniBar 这个「+」是不是当前页的主操作，以及点按派发到哪一页的回调。
  @Prop @Watch('onScreenRoleChange') currentScreenRole: ScreenRole = ScreenRole.ProjectList

  // Issue #905：当前 role 是否登记了可用的主操作（由 PrimaryActionChannel 推送）。
  // 为 true 且底部 TabBar 可见时，用 HDS 原生 MiniBar 承载「+」。
  @State currentPrimaryActionAvailable: boolean = false

  private primaryActionObserver: (() => void) | null = null

  // Issue #773 评论5844518092 第2项：长期持有的 HdsTabsController，创建 HdsTabs 时经 options 传入，
  // 修复真机日志 "GetHdsTabsController no hdsTabsControllerClass prop"。
  private controller: HdsTabsController = new HdsTabsController()

  // Issue #905 复核（MiniBar 折叠态点击的事件语义）：按官方 d.ts，宽度 <600vp 时首次创建的
  // MiniBar 默认 COLLAPSE；「点击折叠的 MiniBar 或 TabBar」以 USER_CLICK 模式切换折叠/展开，
  // 一次点击可能同时改 miniBarStyle 与 tabBarStyle；只有 controller.applyMiniBarStyle()
  // 触发的切换才是 APP_TRIGGER。
  // 关键点：点折叠态 MiniBar 是把它从 COLLAPSE 切成 EXPAND，回调拿到的是**变化后**的状态
  // （miniBarStyle === EXPAND），所以「只在回调里看到 COLLAPSE 才拉回展开」恰好漏掉这次点击，
  // 也不能声称那一次点按就等于新建。
  // 本组件把「加号点击」统一收敛到 PrimaryActionChannel，不重复实现新建方法：
  //   - 展开态 MiniBar 里自绘加号的点击由 primaryActionMiniBar().onClick 派发；
  //   - 折叠态 MiniBar 被用户点开（USER_CLICK + 前值 COLLAPSE + 现值 EXPAND + tabBarStyle 仍折叠，
  //     说明用户点的是 MiniBar 而不是底部页签栏）视为同一次「加号点击」，这里派发一次主操作；
  //   - NORMAL（旋转/开关屏等尺寸变化）与 APP_TRIGGER（代码主动切换）都不是用户点加号，不派发；
  //   - 不再调 controller.applyMiniBarStyle()：页签栏与 MiniBar 的互斥展开交回官方 HDS 状态管理，
  //     应用不再和用户展开底栏的动作抢控制权。
  // previousMiniBarStyle 必须在每次回调最前面更新（含 NORMAL / APP_TRIGGER 提前返回之前），
  // 否则记录的「前值」会和 HDS 实际状态失步。
  // 注意：折叠态点击与自绘 Row.onClick 是否会在同一次指针事件里都触发，SDK 文档没有承诺；
  // 真机若确认双触发，应在唯一主操作入口按同一输入事件去重，不靠延时过滤，也不改成点两下。
  private previousMiniBarStyle: HdsBarStyle = HdsBarStyle.EXPAND

  private miniBarStyleChange: BarStyleChangeCallback = (
    miniBarStyle: HdsBarStyle,
    tabBarStyle: HdsBarStyle,
    miniBarWidth: number,
    tabBarWidth: number,
    mode: HdsTabsBarChangeMode
  ): void => {
    const wasCollapsed: boolean = this.previousMiniBarStyle === HdsBarStyle.COLLAPSE
    this.previousMiniBarStyle = miniBarStyle
    if (
      mode === HdsTabsBarChangeMode.USER_CLICK &&
      wasCollapsed &&
      miniBarStyle === HdsBarStyle.EXPAND &&
      tabBarStyle === HdsBarStyle.COLLAPSE &&
      this.currentPrimaryActionAvailable &&
      this.currentShowPrimaryNav
    ) {
      PrimaryActionChannel.getInstance().dispatch(this.currentScreenRole)
    }
  }

  // Issue #774 评论5847359998：@Watch 回调——currentShowPrimaryNav 变化时通过 controller 动画控制底栏显隐，
  // 不销毁 TabContent，避免 Settings 进入两次、Home 重新 appear、返回首页 FAB 乱跳。
  onShowPrimaryNavChange(): void {
    if (this.currentShowPrimaryNav) {
      this.controller.applyShowAnimation(HdsAnimationMode.CLICK_ANIMATION)
    } else {
      this.controller.applyHideAnimation(HdsAnimationMode.CLICK_ANIMATION)
    }
  }

  // Issue #905：订阅主操作通道。页面加载完成后登记/更新可用状态会触发这里，
  // 据此决定是否把「+」交给 HDS 原生 MiniBar 承载。
  aboutToAppear(): void {
    const self = this
    this.primaryActionObserver = (): void => {
      self.currentPrimaryActionAvailable = PrimaryActionChannel.getInstance().hasPrimaryAction(self.currentScreenRole)
    }
    PrimaryActionChannel.getInstance().subscribe(this.primaryActionObserver)
  }

  aboutToDisappear(): void {
    if (this.primaryActionObserver) {
      PrimaryActionChannel.getInstance().unsubscribe(this.primaryActionObserver)
    }
  }

  // 当前页面 role 变化时重新判定该页是否有可用的主操作。
  onScreenRoleChange(): void {
    this.currentPrimaryActionAvailable = PrimaryActionChannel.getInstance().hasPrimaryAction(this.currentScreenRole)
  }

  // Issue #784 评论5857397503 第3点：PageMapForStack 按 ownerStack 传入 navPathStack，
  // 不再写死 homeStack，修复从星图/统计进入 Settings 时操作错 NavPathStack 的问题。
  // Issue #784 评论5857856673 问题1：增加 ownerTabIndex 参数，SettingsScreen 用它上报正确的 tab。
  @Builder
  PageMapForStack(name: string, ownerStack: NavPathStack, ownerTabIndex: number) {
    if (name === Routes.Home) {
      // Issue #768 评论5838624438：星图快捷入口改为切换一级 tab，不再 push 进 homeStack。
      HomeScreen({
        onOpenStarMap: (): void => {
          // Issue #906：作品页「打开星图」是明确的用户入口，先记用户意图再提交 currentTab。
          this.onUserTabClick(TAB_STARMAP)
          this.currentTab = TAB_STARMAP
        }
      })
    } else if (name === Routes.Workspace) {
      WorkspaceScreen({ navPathStack: ownerStack })
    } else if (name === Routes.Writing) {
      WritingScreen({ navPathStack: ownerStack })
    } else if (name === Routes.StarMapList) {
      StarMapListScreen({ navPathStack: ownerStack })
    } else if (name === Routes.StarMap) {
      StarMapScreen()
    } else if (name === Routes.Settings) {
      SettingsScreen({ navPathStack: ownerStack, screenTabIndex: ownerTabIndex })
    } else if (name === Routes.Stats) {
      StatsScreen()
    }
  }

  // ArkUI navDestination 契约：只接受「单参 @Builder」。#784 为了把 ownerStack / ownerTabIndex
  // 传进页面，把 @Builder 包成箭头 lambda 传给 navDestination，框架解析不到 builder
  // （真机日志 "AceNavigation: Builder function is empty" / "load page failed: StarMap"），
  // destination 退化成空节点，页面内容不渲染。
  // 这里为三个 stack 各保留一个单参 @Builder 作为 navDestination 入口，内部再委托给 PageMapForStack。
  @Builder
  HomePageMap(name: string) {
    this.PageMapForStack(name, this.homeStack, TAB_HOME)
  }

  @Builder
  StarMapPageMap(name: string) {
    this.PageMapForStack(name, this.starMapStack, TAB_STARMAP)
  }

  @Builder
  StatsPageMap(name: string) {
    this.PageMapForStack(name, this.statsStack, TAB_STATS)
  }

  // Issue #774 评论5847359998：三个 TabContent 提取为 @Builder，单棵树唯一复用。
  @Builder
  HdsTabContents() {
    // 作品 tab（TAB_HOME=0）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.homeStack) {
        // 内容由 homeStack 驱动
      }
      .navDestination(this.HomePageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
    }
    .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_text'), '作品'))

    // 星图 tab（TAB_STARMAP=1）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.starMapStack) {
        // 内容由 starMapStack 驱动
      }
      .navDestination(this.StarMapPageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
    }
    .tabBar(new BottomTabBarStyle($r('app.media.ic_star'), '星图'))

    // 统计 tab（TAB_STATS=2）— 保留图标 + 文字
    TabContent() {
      HdsNavigation(this.statsStack) {
        // 内容由 statsStack 驱动
      }
      .navDestination(this.StatsPageMap)
      .mode(NavigationMode.Stack)
      .hideTitleBar(true)
    }
    .tabBar(new BottomTabBarStyle($r('sys.media.ohos_ic_public_more'), '统计'))
  }

  // Issue #905：MiniBar 内唯一的触发组件——原生「+」。
  // 透过 HDS 宿主 MiniBar 的系统材质呈现光感，只派发当前页已登记的主操作，
  // 业务仍是页面原本的命名框 / 建卷 / 建星图入口，不在通道里另起一套创建逻辑。
  // 展开态下的加号点击走这里的 onClick；MiniBar 处于折叠态时，用户那次点按按官方语义
  // 先被 HDS 用来展开 MiniBar，由 miniBarStyleChange 识别为同一次「加号点击」后派发。
  @Builder
  primaryActionMiniBar() {
    Row() {
      SymbolGlyph($r('sys.symbol.plus'))
        .fontSize(24)
        .fontColor([$r('sys.color.icon_primary')])
    }
    .width(48)
    .height(48)
    .justifyContent(FlexAlign.Center)
    .alignItems(VerticalAlign.Center)
    .accessibilityText('新建')
    .onClick(() => {
      PrimaryActionChannel.getInstance().dispatch(this.currentScreenRole)
    })
  }

  // Issue #774 评论5847359998：悬浮页签样式按 SDK d.ts 真实字段结构写，不用 as 强转。
  // barWidth 中/大档调大匹配设计稿；补 gradientMask 实现沉浸光感。
  // Issue #905 复核（窄屏宽度预算）：smallWidth 用三页签官方小屏标准宽 76*3+8=236vp，
  // 不再自取 280vp。360vp 窗口下 236(页签) + 48(MiniBar) + 左右各 16vp 默认
  // barSideMargin = 316vp，给 HDS 自己的两栏间隔留出余量，避免人为挤满整个底栏。
  private buildFloatingStyle(): HdsTabsFloatingStyle {
    const style: HdsTabsFloatingStyle = {
      barWidth: {
        smallWidth: 236,
        mediumWidth: 400,
        largeWidth: 500,
      },
      barBottomMargin: FLOATING_BAR_BOTTOM_MARGIN,
      // 官方《设置页签栏的模糊样式》：gradientMask 就是手册里的"渐变模糊"，
      // maskColor 是"底部纯色、向上逐渐透明"的遮罩色，maskHeight 控制渐隐高度。
      // 官方 d.ts 确认 maskColor 只能取单色 ResourceColor，做不了渐变。
      // 之前把它改成 Color.Transparent 等于关掉渐变模糊，且像素实测毫无收益，已回退。
      gradientMask: {
        maskColor: $r('sys.color.background_secondary'),
        maskHeight: 32,
      },
      systemMaterialEffect: {
        materialType: hdsMaterial.MaterialType.IMMERSIVE,
        materialLevel: hdsMaterial.MaterialLevel.ADAPTIVE,
      }
    }
    // Issue #905：当前页确有 Core 主操作、且底部 TabBar 可见时，用 HDS 原生 MiniBar
    // 承载「+」，由宿主 MiniBar 透出系统沉浸光感。不再对内容流 Button 挂无效的
    // .systemMaterial，也不用 backgroundBlurStyle + shadow 伪造原生光。
    // Issue #905 复核：这里只切片 MiniBar 的存在性，不碰 index/onChange，也不改
    // 三个 TabContent 的选中项，所以 MiniBar 随 role/加载结果出现或消失时，
    // 原生页签的选中与布局不会被这次改动反复翻转（可用状态也只在真实变化时通知）。
    if (this.currentPrimaryActionAvailable && this.currentShowPrimaryNav) {
      style.miniBar = {
        miniBarBuilder: this.primaryActionMiniBar,
        enableMiniBarBackground: true,
        // 主操作「+」的宿主：首次创建首选展开着落；折叠态被用户点开也派发一次，
        // 具体状态转换与去重边界见 miniBarStyleChange 注释。
        miniBarStyle: HdsBarStyle.EXPAND,
        onBarStyleChange: this.miniBarStyleChange,
        // 宽度钉成一个按钮尺寸，不随屏幕宽度档位（440/600/840vp）改档重排。
        // 注意：#905 复核已更正上一轮「48vp 就不会重排」的说法——48vp 只保证 MiniBar
        // 自身不换档；窄屏下页签栏与 MiniBar 是否都能常驻展开由官方 HDS 双栏状态管理决定，
        // 且 <440vp 时 USER_CLICK 可以展开页签栏并折叠 MiniBar（见 miniBarStyleChange）。
        miniBarWidth: {
          smallWidth: 48,
          mediumWidth: 48,
          largeWidth: 48,
        },
      }
    }
    return style
  }

  build() {
    // Issue #774 评论5847359998：单棵树——只保留一棵 HdsTabs，不再因底栏显隐重建整棵页面树。
    // 底栏显隐通过 @Watch 回调调 controller.applyHideAnimation/applyShowAnimation 控制。
    // 正常显示时不调 barHeight，让 HDS 自己决定悬浮栏高度。
    HdsTabs({
      index: this.currentTab,
      controller: this.controller,
      barPosition: BarPosition.End
    }) {
      this.HdsTabContents()
    }
    .barOverlap(true)
    .barPosition(BarPosition.End)
    .vertical(false)
    // Issue #794 评论5885532765/5886073378：显式配置 animationDuration，确保每次点击底栏切换都播放页签切换动画。
    // BottomTabBarStyle 下 animationDuration 默认值为 0（无动画），不显式设置则点击切换无动画。
    // 300ms 是 ArkUI Tabs 非 BottomTabBarStyle 的默认动画时长，与官方 TabBar 动画示例一致。
    // 横向滑动的跟手动画由 interpolatingSpring 弹性曲线驱动，不受 animationDuration 影响。
    // 评论5886073378 复核确认：此改动直接作用于 TabContent 切换，未引入第二套切换逻辑
    // （onChange 仍只写 this.currentTab = index，不在 onChange 里调 controller.changeIndex()，无一次性动画状态）。
    .animationDuration(300)
    // 官方《设置页签栏的模糊样式》给的"背景模糊"毛玻璃方案，官方定位是沉浸光感的
    // 轻量兜底：沉浸光感是对身后画面做材质折射，背后无内容就没内容可折；
    // 背景模糊是 BackdropBlur + 磨砂白，对任何背后像素都成立，所以观感更接近系统组件。
    // 前置条件 barOverlap(true) / barPosition(End) / vertical(false) 见上方三行，缺一则模糊不渲染。
    // 禁止再叠加原生 Tabs 的 barBackgroundColor / barBackgroundEffect / barBackgroundBlurStyle，
    // 那会顶掉 HdsTabs 自己的模糊渲染管线（其中 barBackgroundColor 只会留下固定 80vp 半径、颜色被忽略）。
    .barBackgroundBlurStyle(BlurStyle.Thin)
    .barFloatingStyle(this.buildFloatingStyle())
    // 正文/设置页禁止横向滑动切 tab，防止绕过离开事务。
    // Issue #774 评论5847638284 第2项：用交互锁而非视觉显示，避免 @Watch 触发底栏 hide→show。
    .scrollable(this.currentPrimaryNavInteractive)
    .onContentWillChange((currentIndex: number, comingIndex: number): boolean => {
      if (!this.currentPrimaryNavInteractive) {
        return false
      }
      return true
    })
    .onTabBarClick((index: number) => {
      // Issue #906：onTabBarClick 是确认的用户点击入口；只记用户意图，不改 currentTab，
      // 真正切换仍由 onChange 完成，避免绕过 onContentWillChange 交互锁。
      this.onUserTabClick(index)
    })
    .onChange((index: number) => {
      // Issue #768 评论5835171266 第1项：onChange 只写 currentTab（@Link 双向绑定到父组件），
      // 父组件 PrimaryTabShell 的 @Watch 会触发 syncActiveTab，不再在此处调 syncActiveTab。
      this.currentTab = index
    })
    .width('100%')
    .height('100%')
  }
}
