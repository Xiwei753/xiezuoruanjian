// NavigationHeaderActionDispatcher — ActionRole → 行为的唯一映射。
// Sync 调共享 bridge 的 performSync()，Settings 交给 SettingsOverlayController：
// 宽屏由后者用 WideSettingsOverlay 打开窗口级浮层，窄屏进入 Routes.Settings，
// Search 走全局搜索：窄屏进 Routes.Search，宽屏由 SearchOverlayController 打开窗口内浮层。
//
// Issue #784 评论 5856805657：同步行为由全局 dispatcher 持有，不再各页面各写一份。

import hilog from '@ohos.hilog'
import { UIContext } from '@kit.ArkUI'
import { ActionRole, ScreenRole } from './ScreenSemantics'
import { Routes, SearchRouteParams, StarMapRouteParams, WorkspaceRouteParams, WritingRouteParams } from './Routes'
import { NavigationTransactionCoordinator } from './NavigationTransactionCoordinator'
import { NavigationChromeStore } from './NavigationChromeStore'
import { SearchContext, SearchOriginScreen } from '../../corebridge/dto/SearchDtos'
import { SearchOverlayController } from '../../feature/search/presentation/SearchOverlayController'
import { SettingsSectionContext } from '../../feature/settings/presentation/SettingsSectionContext'
import { SyncOperationController } from '../../feature/settings/presentation/SyncOperationController'
import { SettingsOverlayController } from '../../feature/settings/presentation/SettingsOverlayController'
import { SyncOutcomePresenter } from '../../feature/settings/presentation/SyncOutcomePresenter'
import { SyncOutcome } from '../../feature/settings/presentation/SyncRunTypes'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../../platform/diagnostics/HarmonyDiagnosticEvents'

const TAG = 'NavigationHeaderActionDispatcher'
const DOMAIN = 0x0000

export class NavigationHeaderActionDispatcher {
  private static instance: NavigationHeaderActionDispatcher
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()
  // Issue #952：手动请求合并进后台自动同步时，顶栏入口需要自己弹“进行中/结果”Toast，
  // UIContext 由壳层注册一次（与 SettingsOverlayController 同一模式）。
  private uiContext: UIContext | null = null

  static getInstance(): NavigationHeaderActionDispatcher {
    if (!NavigationHeaderActionDispatcher.instance) {
      NavigationHeaderActionDispatcher.instance = new NavigationHeaderActionDispatcher()
    }
    return NavigationHeaderActionDispatcher.instance
  }

  /** Issue #952：壳层注册窗口级 UIContext 供顶栏同步入口弹 Toast。 */
  attachUIContext(uiContext: UIContext): void {
    this.uiContext = uiContext
  }

  /** Issue #952：壳层销毁时摘除，避免持有失效 UIContext。 */
  detachUIContext(uiContext: UIContext): void {
    if (this.uiContext === uiContext) {
      this.uiContext = null
    }
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
    const controller: SyncOperationController = SyncOperationController.getInstance()
    // Issue #952：手动请求与后台自动同步重叠时，不去中断也不新起第二轮（保持同一运行任务的去重），
    // 但这次是用户主动点击：顶栏入口先提示“同步正在进行”，await 后展示本次手动请求收到的结果。
    // 自动同步自身不弹提示，结果照常由 SyncRunPresentationState 与 Core 记录。
    if (controller.currentRunningSource() === 'auto') {
      this.showSyncEntryToast('同步正在进行…')
      void controller.performSync('manual').then((outcome: SyncOutcome): void => {
        this.showSyncEntryOutcome(outcome)
      })
      return
    }
    // 无运行任务或已在手动任务中：进行中与终态由壳层按 manual 快照统一展示，入口不重复弹。
    void controller.performSync('manual')
  }

  /** Issue #952：顶栏入口 Toast。没有窗口 UIContext（壳层未注册）时静默跳过。 */
  private showSyncEntryToast(message: string): void {
    if (this.uiContext === null) {
      return
    }
    try {
      this.uiContext.getPromptAction().showToast({ message: message })
    } catch (_) {
      // Toast 失败不影响同步结果本身。
    }
  }

  /** Issue #952：顶栏入口展示本次手动请求收到的同步结果。 */
  private showSyncEntryOutcome(outcome: SyncOutcome): void {
    const context: Context | undefined = this.uiContext?.getHostContext()
    if (context === undefined) {
      return
    }
    this.showSyncEntryToast(SyncOutcomePresenter.present(context, outcome))
  }

  /** 设置 — 宽屏开模态浮层（SettingsOverlayController + WideSettingsOverlay），窄屏进 Routes.Settings 路由页 */
  // Issue #825 复核第2点：宽屏设置悬浮在打开前页面之上，
  // 走 SettingsOverlayController 的窗口层级 custom dialog——系统返回 / 遮罩都由对话框容器保证；
  // 窄屏保持原来的 pushPath(Routes.Settings) 行为，不留第二套入口。
  // Issue #972：浮层内容工厂已迁移到 ui/wide/WideSettingsOverlay，由 controller 独占创建与释放。
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

  /** 搜索 — 全局搜索入口；窄屏进 Routes.Search，宽屏打开窗口内搜索浮层。 */
  private handleSearch(): void {
    const context: SearchContext = this.buildSearchContext()
    if (SearchOverlayController.getInstance().isOverlayLayout()) {
      SearchOverlayController.getInstance().open(context)
      return
    }

    const navPathStack: NavPathStack | null = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      hilog.warn(DOMAIN, TAG, 'navPathStack is null; cannot navigate to search')
      return
    }
    const params: SearchRouteParams = { context: context }
    navPathStack.pushPath({ name: Routes.Search, param: params })
  }

  // 发起搜索的页面上下文：只用于 Core 排序。优先取当前活动栈的真实路由参数，
  // 不额外维护一份 project/chapter/starmap 全局状态。
  private buildSearchContext(): SearchContext {
    const role: ScreenRole = NavigationChromeStore.getInstance().getCurrentRole()
    const context: SearchContext = { originScreen: this.resolveSearchOrigin(role) }
    const stack: NavPathStack | null = this.navCoordinator.getNavPathStack()
    if (stack !== null) {
      const writingArr = stack.getParamByName(Routes.Writing) as WritingRouteParams[]
      const writing = writingArr && writingArr.length > 0 ? writingArr[writingArr.length - 1] : null
      const workspaceArr = stack.getParamByName(Routes.Workspace) as WorkspaceRouteParams[]
      const workspace = workspaceArr && workspaceArr.length > 0 ? workspaceArr[workspaceArr.length - 1] : null
      const starmapArr = stack.getParamByName(Routes.StarMap) as StarMapRouteParams[]
      const starmap = starmapArr && starmapArr.length > 0 ? starmapArr[starmapArr.length - 1] : null

      if (writing !== null && writing.projectId.length > 0) {
        context.currentProjectId = writing.projectId
        if (writing.volumeId !== undefined && writing.volumeId.length > 0) {
          context.currentVolumeId = writing.volumeId
        }
        if (writing.chapterId !== undefined && writing.chapterId.length > 0) {
          context.currentChapterId = writing.chapterId
        }
      } else if (workspace !== null && workspace.projectId.length > 0) {
        context.currentProjectId = workspace.projectId
      }
      if (starmap !== null && starmap.starmapId.length > 0) {
        context.currentStarmapId = starmap.starmapId
      }
    }
    if (context.originScreen === 'settings') {
      context.currentSettingsSection = SettingsSectionContext.getCurrentSection()
    }
    return context
  }

  private resolveSearchOrigin(role: ScreenRole): SearchOriginScreen {
    if (role === ScreenRole.Home) { return 'home' }
    if (role === ScreenRole.ProjectList) { return 'home' }
    if (role === ScreenRole.ProjectWorkspace) { return 'project' }
    if (role === ScreenRole.Writing) { return 'writing' }
    if (role === ScreenRole.StarMap || role === ScreenRole.StarMapDetail) { return 'starmap' }
    if (role === ScreenRole.Settings) { return 'settings' }
    return 'general'
  }
}
