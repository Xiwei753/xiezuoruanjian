// NavigationChromeStore — 纯 presentation 层的导航 chrome 状态存储。
// 各 Screen 的根 NavDestination.onShown 上报自己的 ScreenRole；
// AppNavigation 订阅此 store，再 resolve ScreenPolicy 控制底栏显隐。
// 不塞回 NavigationTransactionCoordinator，保持 UI chrome 和导航事务分离。
//
// Issue #768：按 tab 保存最后一个可见 role，切 tab 时同步发布该 tab 当前 role。
// 修复全局 currentRole 在多 tab 间错乱的问题。
// Issue #768 评论5835958606：后台 tab 的 onShown 只更新缓存，不污染全局 currentRole；
// 只有 activeTab 的 role 变化才发布给观察者。

import { ScreenRole } from '../../corebridge/dto/PlatformDtos'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../../platform/diagnostics/HarmonyDiagnosticEvents'

type ScreenRoleObserver = (role: ScreenRole) => void

export class NavigationChromeStore {
  private static instance: NavigationChromeStore
  private currentRole: ScreenRole = ScreenRole.ProjectList
  private observers: ScreenRoleObserver[] = []
  // Issue #768：按 tab 保存最后一个可见 role。
  private tabRoles: Map<number, ScreenRole> = new Map()
  // Issue #768 评论5835958606：当前激活 tab 索引，只有激活 tab 的 role 变化才发布。
  private activeTab: number = 0

  private constructor() {}

  static getInstance(): NavigationChromeStore {
    if (!NavigationChromeStore.instance) {
      NavigationChromeStore.instance = new NavigationChromeStore()
    }
    return NavigationChromeStore.instance
  }

  // Issue #768 评论5835958606：切 tab 时调用，设置 activeTab 并发布该 tab 的 role。
  // 替代旧的 syncTabRole 方法。
  // Issue #906：这里记录的是实际状态提交（可能来自用户手势，也可能来自接续恢复等程序改 currentTab），
  // 因此 origin='App'。明确的用户切换意图由 PrimaryTabShell.recordUserTabRequest 记 nav.tab_requested（User）。
  setActiveTab(tabIndex: number): void {
    const previousTab = this.activeTab
    this.activeTab = tabIndex
    if (tabIndex !== previousTab) {
      const fields: DiagnosticField[] = [
        { key: 'fromTab', value: String(previousTab) },
        { key: 'toTab', value: String(tabIndex) }
      ]
      HarmonyDiagnosticEvents.record('Info', 'App', 'nav.tab_changed', 'NavigationChromeStore', fields)
    }
    const role = this.tabRoles.get(tabIndex) ?? ScreenRole.ProjectList
    this.publish(role)
  }

  // 各 Screen 的 NavDestination.onShown 调此方法上报自己的 ScreenRole。
  // Issue #768 评论5835958606：tabIndex 必选；后台 tab 只更新缓存，不发布全局 role。
  // Issue #906：按实际到达的 onShown 上报 nav.destination_shown。角色缓存相等不代表页面未重新显示
  // （连续两个同 ScreenRole 页面、或离开后重显同类型页面都是真实显示事件），不能用 previousRole === role 吞掉。
  reportScreenRole(role: ScreenRole, tabIndex: number): void {
    this.tabRoles.set(tabIndex, role)
    const fields: DiagnosticField[] = [
      { key: 'screenRole', value: String(role) },
      { key: 'tabIndex', value: String(tabIndex) },
      { key: 'isActiveTab', value: String(tabIndex === this.activeTab) }
    ]
    HarmonyDiagnosticEvents.record('Info', 'App', 'nav.destination_shown', 'NavigationChromeStore', fields)
    if (tabIndex === this.activeTab) {
      this.publish(role)
    }
  }

  // Issue #768 评论5835958606：抽取发布逻辑，更新 currentRole 并通知所有观察者。
  private publish(role: ScreenRole): void {
    this.currentRole = role
    for (const observer of this.observers) {
      observer(role)
    }
  }

  // Issue #768：返回指定 tab 的最后一个 role，默认 ScreenRole.ProjectList。
  getRoleForTab(tabIndex: number): ScreenRole {
    const role: ScreenRole | undefined = this.tabRoles.get(tabIndex)
    return role !== undefined ? role : ScreenRole.ProjectList
  }

  // AppNavigation 订阅 ScreenRole 变化
  subscribe(observer: ScreenRoleObserver): void {
    this.observers.push(observer)
    observer(this.currentRole)
  }

  // AppNavigation 取消订阅
  unsubscribe(observer: ScreenRoleObserver): void {
    const index = this.observers.indexOf(observer)
    if (index >= 0) {
      this.observers.splice(index, 1)
    }
  }

  getCurrentRole(): ScreenRole {
    return this.currentRole
  }
}
