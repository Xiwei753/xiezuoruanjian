// HdsPrimaryTabsApi23 — API23+ 一级底部导航壳层，使用 HdsTabs 悬浮页签。
// Issue #768：API23+ 改用 UI Design Kit 的 HdsTabs 替代普通 Tabs + BottomTabBarStyle。
// Issue #768 评论5835171266 第1项：不再自己创建 NavPathStack、不注册 coordinator、
// 不创建 @Link @Watch currentTab、不 @Provide activeStack。
// 三个 NavPathStack 由父组件 PrimaryTabShell 唯一持有并传入，currentTab 通过 @Link 双向绑定
// 回父组件（不加 @Watch，不写 coordinator），切 tab 时只有 PrimaryTabShell 的 @Watch 触发 syncActiveTab。
//
// Issue #768 评论5835171266 第3项：HdsTabs 加 barFloatingStyle + systemMaterialEffect(IMMERSIVE)
// 实现真正的悬浮沉浸光感效果；Navigation 改为 HdsNavigation。

import { Routes } from '../../Routes'
import hilog from '@ohos.hilog'
import { HomeScreen } from '../../../feature/project/ui/HomeScreen'
import { WorkspaceScreen } from '../../../feature/project/ui/WorkspaceScreen'
import { WritingScreen } from '../../../feature/editor/ui/WritingScreen'
import { StarMapScreen } from '../../../feature/starmap/ui/StarMapScreen'
import { SettingsScreen } from '../../../feature/settings/ui/SettingsScreen'
import { StatsScreen } from '../../../feature/stats/ui/StatsScreen'
import { HdsTabsFloatingStyle, hdsMaterial } from '@kit.UIDesignKit'

// 一级导航 Tab 索引常量
const TAB_HOME: number = 0    // 作品
const TAB_STARMAP: number = 1 // 星图
const TAB_STATS: number = 2   // 统计

// 正常底栏高度（vp）
const NORMAL_BAR_HEIGHT: number = 56
// 隐藏底栏时的高度（vp）
const HIDDEN_BAR_HEIGHT: number = 0

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

  // 来自 AdaptiveState 的底部安全区域高度（vp），用于悬浮页签底部间距
  @Prop safeBottomVp: number = 0

  // 是否显示一级导航栏，由外部 ScreenPolicy 决定
  @Prop currentShowPrimaryNav: boolean = true

  @Builder
  PageMap(name: string) {
    if (name === Routes.Home) {
      HomeScreen()
    } else if (name === Routes.Workspace) {
      WorkspaceScreen()
    } else if (name === Routes.Writing) {
      WritingScreen()
    } else if (name === Routes.StarMap) {
      StarMapScreen()
    } else if (name === Routes.Settings) {
      SettingsScreen()
    } else if (name === Routes.Stats) {
      StatsScreen()
    }
  }

  build() {
    // Issue #768 评论5835171266 第3项：HdsTabs 加 barFloatingStyle 配置悬浮样式，
    // systemMaterialEffect 设为 IMMERSIVE 实现沉浸光感效果。
    HdsTabs({ barPosition: BarPosition.End, index: this.currentTab }) {
      // 作品 tab（TAB_HOME=0）
      TabContent() {
        HdsNavigation(this.homeStack) {
          // 内容由 homeStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar('作品')

      // 星图 tab（TAB_STARMAP=1）
      TabContent() {
        HdsNavigation(this.starMapStack) {
          // 内容由 starMapStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar('星图')

      // 统计 tab（TAB_STATS=2）
      TabContent() {
        HdsNavigation(this.statsStack) {
          // 内容由 statsStack 驱动
        }
        .navDestination(this.PageMap)
        .mode(NavigationMode.Stack)
        .hideTitleBar(true)
      }
      .tabBar('统计')
    }
    .barHeight(this.currentShowPrimaryNav ? NORMAL_BAR_HEIGHT : HIDDEN_BAR_HEIGHT)
    // 悬浮页签底部间距跟真实安全区走，不在页面里写死。
    .barMargin({ bottom: this.safeBottomVp })
    // Issue #768 评论5835171266 第3项：悬浮沉浸光感样式。
    .barFloatingStyle({
      systemMaterialEffect: { materialType: hdsMaterial.MaterialType.IMMERSIVE }
    } as HdsTabsFloatingStyle)
    // 正文/设置页禁止横向滑动切 tab，防止绕过离开事务。
    .scrollable(this.currentShowPrimaryNav)
    .onContentWillChange((currentIndex: number, comingIndex: number): boolean => {
      if (!this.currentShowPrimaryNav) {
        return false
      }
      return true
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
