// NavigationChromeStore — 纯 presentation 层的导航 chrome 状态存储。
// 各 Screen 的根 NavDestination.onShown 上报自己的 ScreenRole；
// AppNavigation 订阅此 store，再 resolve ScreenPolicy 控制底栏显隐。
// 不塞回 NavigationTransactionCoordinator，保持 UI chrome 和导航事务分离。

import { ScreenRole } from '../../corebridge/dto/PlatformDtos'

type ScreenRoleObserver = (role: ScreenRole) => void

export class NavigationChromeStore {
  private static instance: NavigationChromeStore
  private currentRole: ScreenRole = ScreenRole.ProjectList
  private observers: ScreenRoleObserver[] = []

  private constructor() {}

  static getInstance(): NavigationChromeStore {
    if (!NavigationChromeStore.instance) {
      NavigationChromeStore.instance = new NavigationChromeStore()
    }
    return NavigationChromeStore.instance
  }

  // 各 Screen 的 NavDestination.onShown 调此方法上报自己的 ScreenRole
  reportScreenRole(role: ScreenRole): void {
    this.currentRole = role
    for (const observer of this.observers) {
      observer(role)
    }
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