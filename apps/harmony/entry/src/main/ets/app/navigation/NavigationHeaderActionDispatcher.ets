// NavigationHeaderActionDispatcher — ActionRole → 行为的唯一映射。
// Sync 调共享 bridge 的 performSync()，Settings 从当前活动 NavPathStack 进入 Routes.Settings，
// Search 走全局搜索入口（功能未接完也不删 slot）。
//
// Issue #784 评论 5856805657：同步行为由全局 dispatcher 持有，不再各页面各写一份。

import hilog from '@ohos.hilog'
import promptAction from '@ohos.promptAction'
import { ActionRole } from './ScreenSemantics'
import { Routes } from './Routes'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { SyncOperationController, SyncOutcome } from '../../feature/settings/presentation/SyncOperationController'
import { SyncOutcomePresenter } from '../../feature/settings/presentation/SyncOutcomePresenter'
import { SettingsOverlayController } from '../../feature/settings/presentation/SettingsOverlayController'
import { getAppContext } from '../AppContext'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../../platform/diagnostics/HarmonyDiagnosticEvents'

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
    // Issue #906：每个角色的点击都记一次 nav.action，事件名稳定可检索。
    const actionField: DiagnosticField = { key: 'role', value: String(role) }
    HarmonyDiagnosticEvents.record('Info', 'User', 'nav.action', 'NavigationHeaderActionDispatcher', [actionField])
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
    controller.performSync().then((outcome: SyncOutcome) => {
      // Issue #906：同步完成只记非敏感状态字段，绝不记 rawError（可能含 URL/Token）。
      const finishedFields: DiagnosticField[] = [
        { key: 'phase', value: String(outcome.phase) },
        { key: 'overallStatus', value: String(outcome.overallStatus ?? '') },
        { key: 'errorCode', value: String(outcome.errorCode ?? '') },
        { key: 'totalDownloaded', value: String(outcome.totalDownloaded ?? 0) }
      ]
      HarmonyDiagnosticEvents.record('Info', 'App', 'sync.finished', 'NavigationHeaderActionDispatcher', finishedFields)
      // 顶栏与设置页共用同一展示映射（MessageKeyMapper），不各自维护中文错误字典。
      const context = getAppContext().getAbilityContext()
      const message: string = context !== null
        ? SyncOutcomePresenter.present(context, outcome)
        : outcome.message
      if (outcome.phase === 'success') {
        hilog.info(DOMAIN, TAG, 'sync success: %{public}s', outcome.message)
        promptAction.showToast({ message: message })
      } else {
        // Issue #906：HiLog 只记稳定状态码，不记 rawError/message（可能含 URL/Token/正文）。
        hilog.error(DOMAIN, TAG, 'sync failed: phase=%{public}s errorCode=%{public}s',
          String(outcome.phase), String(outcome.errorCode ?? ''))
        promptAction.showToast({ message: message })
      }
    }).catch((e: Error) => {
      // Issue #906：异常只记非敏感分类（e.name），不记 e.message / String(e)（可能含 URL/Token）。
      const exceptionField: DiagnosticField = { key: 'errorType', value: e.name }
      HarmonyDiagnosticEvents.record('Error', 'App', 'sync.exception', 'NavigationHeaderActionDispatcher', [exceptionField])
      // Issue #906：异常只记分类（e.name），不记 String(e)（可能含 URL/Token）。
      hilog.error(DOMAIN, TAG, 'sync exception: %{public}s', e.name)
      promptAction.showToast({ message: `同步异常: ${e}` })
    })
  }

  /** 设置 — 宽屏开模态浮层，窄屏进 Routes.Settings 路由页 */
  // Issue #825 复核第2点：宽屏（平台工作台档 isWorkbenchLayout）设置悬浮在打开前页面之上，
  // 走 SettingsOverlayController 的窗口层级 custom dialog——系统返回 / 遮罩都由对话框容器保证；
  // 窄屏保持原来的 pushPath(Routes.Settings) 行为，不留第二套入口。
  private navigateToSettings(): void {
    // Issue #906：记录打开设置的入口形态（浮层/路由），只用于诊断，不改变 UI 分支逻辑。
    const path: string = SettingsOverlayController.getInstance().isOverlayLayout() ? 'overlay' : 'route'
    const pathField: DiagnosticField = { key: 'path', value: path }
    HarmonyDiagnosticEvents.record('Info', 'User', 'settings.open_requested', 'NavigationHeaderActionDispatcher', [pathField])
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
