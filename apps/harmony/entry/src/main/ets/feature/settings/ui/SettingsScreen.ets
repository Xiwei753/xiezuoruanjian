// SettingsScreen — 设置页路由外壳（窄屏专用）。
// Issue #825：设置内容已抽到 SettingsContent.ets。
// - 宽屏（WorkspaceLayoutMode.Workbench）不再走这条路由：
//   设置由 SettingsOverlayPanel 作为窗口层级 custom dialog 挂在当前页面之上，
//   背景仍是打开设置前的页面；见 SettingsOverlayController。
// - 窄屏继续保持原来的 Routes.Settings 路由页行为，只是内容换成 SettingsContent。
// 内容、保存逻辑、主题逻辑都仍然在 SettingsContent / SettingsViewModel 里，这里不复制业务状态。

import hilog from '@ohos.hilog'
import { SettingsContent } from './SettingsContent'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { ScreenRole } from '../../../corebridge/dto/PlatformDtos'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsSettingsDestinationApi23 } from '../../../app/navigation/impl/api23/HdsSettingsDestinationApi23'

@ComponentV2
export struct SettingsScreen {
  // Issue #768 评论5838296806：通过 @Param 从父组件接收 NavPathStack，不再依赖全局 activeTabStack
  @Param navPathStack: NavPathStack = new NavPathStack()
  // Issue #784 评论5857856673 问题1：通过 @Param 从父组件接收 tabIndex，
  // 用于向 NavigationChromeStore 上报正确的 tab，不再硬编码为 0。
  // 属性名避开 ArkUI 全局属性 tabIndex(index: number)。
  @Param screenTabIndex: number = 0
  // 保留 navCoordinator 用于 safePopWithStack 调用
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()

  // Issue #768 评论5835171266 第3项：API23+ 时用 HdsNavDestination 替代 NavDestination，
  // 获得 HDS 沉浸光感材质效果。
  @Local useHdsNav: boolean = false

  aboutToAppear() {
    hilog.info(0x0000, 'SettingsScreen', 'route name=Settings')
    // Issue #768 评论5835171266 第3项：检测 API Level，决定是否使用 HdsNavDestination。
    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
  }

  // ── 安全返回 ──
  // Issue #768 评论5838296806：使用 @Param 传入的 NavPathStack，不再依赖全局 activeTabStack。
  private goBack(): void {
    this.navCoordinator.safePopWithStack(this.navPathStack)
  }

  build() {
    // Issue #752：NavDestination 作为 Navigation 目标页根节点，否则实机白屏。
    // Issue #768 评论5835171266 第3项：API23+ 用 HdsNavDestination 替代 NavDestination，
    // 获得 HDS 沉浸光感材质效果；API < 23 保持普通 NavDestination。
    if (this.useHdsNav) {
      // Issue #773 评论5844518092：改用 V2 官方尾随闭包传 @BuilderParam，
      // 让内容始终在页面上下文执行，避免 this 变成壳组件导致 adaptiveState undefined。
      HdsSettingsDestinationApi23({
        title: $r('app.string.settings'),
        onBack: () => { this.goBack() },
        onShown: () => { NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Settings, this.screenTabIndex) }
      }) {
        SettingsContent({ onRequestClose: () => { this.goBack() } })
      }
    } else {
      NavDestination() {
        SettingsContent({ onRequestClose: () => { this.goBack() } })
      }
      // Issue #768：启用 NavDestination 原生标题栏（含系统返回按钮），删除手写返回 Image。
      .hideTitleBar(false)
      .title($r('app.string.settings'))
      .onBackPressed(() => {
        this.goBack()
        return true
      })
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Settings, this.screenTabIndex)
      })
    }
  }
}