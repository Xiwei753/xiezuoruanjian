// NavigationHeaderActionDispatcher — ActionRole → 行为的唯一映射。
// Sync 调共享 bridge 的 performSync()，Settings 从当前活动 NavPathStack 进入 Routes.Settings，
// Search 走全局搜索入口（功能未接完也不删 slot）。
//
// Issue #784 评论 5856805657：同步行为由全局 dispatcher 持有，不再各页面各写一份。

import hilog from '@ohos.hilog'
import { ActionRole } from '../../corebridge/dto/PlatformDtos'
import { Routes } from './Routes'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { NativeWriterCoreBridge } from '../../corebridge/NativeWriterCoreBridge'

const TAG = 'NavigationHeaderActionDispatcher'
const DOMAIN = 0x0000

export class NavigationHeaderActionDispatcher {
  private static instance: NavigationHeaderActionDispatcher
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()

  static getInstance(): NavigationHeaderActionDispatcher {
    if (!NavigationHeaderActionDispatcher.instance) {
      NavigationHeaderActionDispatcher.instance = new NavigationHeaderActionDispatcher()
    }
    return NavigationHeaderActionDispatcher.instance
  }

  /** 执行页头尾部动作 */
  dispatchAction(role: ActionRole): void {
    if (role === ActionRole.Sync) {
      this.handleSync()
    } else if (role === ActionRole.Settings) {
      this.navigateToSettings()
    } else if (role === ActionRole.Search) {
      this.handleSearch()
    }
  }

  /** 同步 — 调共享 bridge 的 performSync() */
  private handleSync(): void {
    const bridge = NativeWriterCoreBridge.getInstance()
    bridge.performSync().then(() => {
      hilog.info(DOMAIN, TAG, 'performSync completed')
    }).catch((e: Error) => {
      hilog.error(DOMAIN, TAG, 'performSync failed: %{public}s', String(e))
    })
  }

  /** 设置 — 从当前活动 NavPathStack 进入 Routes.Settings */
  private navigateToSettings(): void {
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      hilog.warn(DOMAIN, TAG, 'navPathStack is null; cannot navigate to settings')
      return
    }
    navPathStack.pushPath({ name: Routes.Settings })
  }

  /** 搜索 — 全局搜索入口（功能未接完，保留 slot 不删） */
  private handleSearch(): void {
    hilog.info(DOMAIN, TAG, 'Search action triggered (not yet implemented)')
  }
}
