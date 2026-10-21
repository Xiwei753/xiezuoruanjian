// HdsAppDestinationApi23 — API23+ Home/StarMap/Stats 共享根页面壳。
// Issue #784 评论 5857397503 第 1 点：Home 删除旧顶栏后没有新标题栏接上，
// 新建此共享根页面壳，让 Home / StarMap / Stats 共用 HdsNavDestination + 标题栏。
// 通过平台动作表加载 ActionSlot[]，交给 HeaderTitleBarStack 自绘
// 标题 + 页头动作胶囊，不再在平台端硬编码"同步/搜索/设置"三个 item。
// onShown 时通过 NavigationChromeStore.reportScreenRole 上报当前 ScreenRole + tabIndex。

import { HdsNavDestination, HdsNavigationTitleBarOptions, hdsMaterial, ScrollEffectType } from '@kit.UIDesignKit'
import { HeaderTitleBarStack } from './HeaderTitleBarStack'
import { NavigationChromeStore } from '../../NavigationChromeStore'
import { ScreenRole, ActionSlot, resolveScreenPolicy } from '../../ScreenSemantics'
import { handleRootBack } from '../../RootBackHandler'

@ComponentV2
export struct HdsAppDestinationApi23 {
  @BuilderParam contentBuilder: () => void
  @Param title: string | Resource = ''
  @Param screenRole: ScreenRole = ScreenRole.ProjectList
  @Param onShown: (() => void) | null = null
  @Param screenTabIndex: number = 0
  // 是否根页：根页隐藏返回按钮、返回退出 Ability；非根页显示返回按钮、返回 pop 导航栈
  @Param isRoot: boolean = true
  // 非根页返回回调：返回 true 表示已处理，false 表示未处理（继续默认行为）
  @Param onBack: (() => boolean) | null = null
  // 宽屏（Expanded）由 PrimaryTabShell 的 WideAppShell 统一提供顶部区域，
  // 页面再画一条 HDS 标题栏就会在内容上方留出一条整宽空白（#827 评论 1 第 6 点）。
  @Param showTitleBar: boolean = true
  @Local headerActions: ActionSlot[] = []

  aboutToAppear() {
    this.headerActions = resolveScreenPolicy(this.screenRole).actionSlots
  }

  // stackBuilder 是 CustomBuilder，组件调用必须写在 @Builder 里。
  // 直接在箭头函数体里写 HeaderTitleBarStack({...}) 会被当普通函数调用，运行时崩：
  // TypeError: class constructor cannot called without 'new'。
  @Builder
  headerTitleBar() {
    HeaderTitleBarStack({
      title: this.title,
      headerActions: this.headerActions,
      showBackButton: !this.isRoot
    })
  }

  build() {
    // 页面底色由各页自己的根容器设置（HomeScreen / StarMapScreen / StatsScreen 用
    // sys.color.background_secondary），这里不再包一层 Stack：
    // 实测 HdsNavDestination 会用自己的白色 surface 覆盖设在它上面的 backgroundColor，
    // 而包在 contentBuilder 外面时这个 Stack 又拿不到确定高度，backgroundColor 同样不生效。
    // 只改背景色，不碰 systemMaterialEffect，材质类型与等级仍完全由系统决定。
    // Issue #932 复核：本条说的是不能给 HdsNavDestination 本体设 backgroundColor；
    // 本次改的是 titleBar.style.originalStyle 里的标题栏背板，背板是标题栏自己的
    // 样式入口，与这条历史结论不冲突。
    HdsNavDestination() {
      this.contentBuilder()
    }
    .hideTitleBar(!this.showTitleBar)
    .hideBackButton(this.isRoot)
    .onBackPressed(() => {
      if (!this.isRoot && this.onBack) {
        return this.onBack()
      }
      return handleRootBack()
    })
    .titleBar({
      enableComponentSafeArea: true,
      // 窗口开了 setWindowLayoutFullScreen 之后，标题栏必须显式继续避让顶部状态栏，
      // 否则「素笺」这类标题会被压到 18px 高（实测 [200,157][325,230] → [200,106][325,124]）。
      // 本文件只在 API >= 23 渲染（见各 Screen 的 useHdsNav 判定），该字段 API 20+ 恒可用。
      avoidLayoutSafeArea: true,
      content: {
        // mainTitle 传空串：标题文字改由 stackBuilder 自绘，才能和动作胶囊排在同一行。
        // 标题栏自绘的三个坑（内边距、返回按钮不避让、返回按钮自带 label）都在
        // HeaderTitleBarStack 里统一处理了。
        title: { mainTitle: '' },
        backIcon: { label: '' },
        stackBuilder: (): void => {
          this.headerTitleBar()
        }
      },
      style: {
        // Issue #932 复核：官方文档明确模糊样式类型（COMMON_BLUR / GRADUAL_BLUR）
        // 下，标题栏背板默认值本来就是透明色——原先这里写 Color.Transparent 属于 no-op，
        // 不改变任何可见像素，也去不掉白条；白条来自标题栏区域背后没有承接层、
        // 直接露出 HDS 自己的 surface。现改为把背板填成页面底色
        // （sys.color.background_secondary），让标题栏与页面同底，HDS surface 不再透出。
        // 仍然保留真正的 HDS 标题栏节点——自绘标题与动作胶囊挂在
        // titleBar.content.stackBuilder 下，不隐藏标题栏、不把胶囊挪到内容层 Stack。
        // 历史真机结论（Issue #932）：backgroundColor 不能设在 HdsNavDestination 本体上，
        // 会被 HDS surface 覆盖；背板是标题栏自己的样式入口，不是给 NavDestination 设 backgroundColor。
        scrollEffectOpts: {
          enableScrollEffect: false,
          scrollEffectType: ScrollEffectType.COMMON_BLUR
        },
        originalStyle: {
          backgroundStyle: { backgroundColor: $r('sys.color.background_secondary') }
        },
        // 官方《沉浸光感》（ui-design-hds-component-material）示例用的是
        // MaterialType.ADAPTIVE，不是 IMMERSIVE：ADAPTIVE 表示自适应系统材质，
        // 材质等级 ADAPTIVE 表示生效策略由系统按设备算力自适应决定。
        // 之前这里写 IMMERSIVE，与底栏一致，但底栏那圈跟手的光只在 HDS 自己
        // 渲染的 bar 上出现，标题栏实测按与不按都没有变化。
        systemMaterialEffect: {
          materialType: hdsMaterial.MaterialType.ADAPTIVE,
          materialLevel: hdsMaterial.MaterialLevel.ADAPTIVE
        }
      }
    } as HdsNavigationTitleBarOptions)
    .onShown(() => {
      NavigationChromeStore.getInstance().reportScreenRole(this.screenRole, this.screenTabIndex)
      if (this.onShown) {
        this.onShown()
      }
    })
  }
}
