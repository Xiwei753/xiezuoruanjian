// NavigationHeaderActionDispatcher — ActionRole → 行为的唯一映射。
// Sync 调共享 bridge 的 performSync()，Settings 从当前活动 NavPathStack 进入 Routes.Settings，
// Search 走全局搜索入口（功能未接完也不删 slot）。
//
// Issue #784 评论 5856805657：同步行为由全局 dispatcher 持有，不再各页面各写一份。

import hilog from '@ohos.hilog'
import promptAction from '@ohos.promptAction'
import { ActionRole } from '../../corebridge/dto/PlatformDtos'
import { Routes } from './Routes'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { SyncOperationController } from '../../feature/settings/presentation/SyncOperationController'
import { SettingsOverlayController } from '../../feature/settings/presentation/SettingsOverlayController'

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

  /** 同步 — 使用 SyncOperationController 共用同步事务 */
  private handleSync(): void {
    const controller = SyncOperationController.getInstance()
    if (controller.isSyncing()) {
      hilog.info(DOMAIN, TAG, 'sync already in progress')
      promptAction.showToast({ message: '同步正在进行中…' })
      return
    }
    promptAction.showToast({ message: '同步开始…' })
    controller.performSync().then((outcome) => {
      if (outcome.phase === 'success') {
        hilog.info(DOMAIN, TAG, 'sync success: %{public}s', outcome.message)
        promptAction.showToast({ message: outcome.message })
      } else {
        hilog.error(DOMAIN, TAG, 'sync failed: errorCode=%{public}s message=%{public}s rawError=%{public}s',
          outcome.errorCode ?? '', outcome.message, outcome.rawError ?? '')
        promptAction.showToast({ message: outcome.message })
      }
    }).catch((e: Error) => {
      hilog.error(DOMAIN, TAG, 'sync exception: %{public}s', String(e))
      promptAction.showToast({ message: `同步异常: ${e}` })
    })
  }

  /** 设置 — 宽屏开模态浮层，窄屏进 Routes.Settings 路由页 */
  // Issue #825 复核第2点：宽屏（WorkspaceLayoutMode.Workbench）设置悬浮在打开前页面之上，
  // 走 SettingsOverlayController 的窗口层级 custom dialog——系统返回 / 遮罩都由对话框容器保证；
  // 窄屏保持原来的 pushPath(Routes.Settings) 行为，不留第二套入口。
  private navigateToSettings(): void {
    if (SettingsOverlayController.getInstance().isOverlayLayout()) {
      SettingsOverlayController.getInstance().open()
      return
    }

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
