// HomeScreen — 作品主页。
// Issue #760 评论 5822350374 问题1/3：
// - 改为 @ComponentV2 + @Local viewModel（与 WorkspaceScreen 一致）
// - 页面只派发事件，ViewModel 内部管理 async 和状态
// - 顶栏同步调用 viewModel.performSync()，不跳设置页
// - 作品菜单按 ScreenPolicy 的 Project Context actions 渲染
// - 删除 Home 内部的 BottomTabBar（全局只有 AppNavigation 一层底栏）
// Issue #827 评论 2：宽屏布局按结构图重做，删除 #760 恢复回来的
// WritingStatsCard / RecentEditsSection / ProjectsSection / QuickActions 四块统计树。
// 宽屏首页只剩作品卡流：作品卡 + 「+」卡从内容区左上开始排，
// 「+」是网格里的一张卡而不是 FAB / 按钮，永久跟在最后一张作品卡后面。
// Issue #767：FAB 改原生 Button + 去 M3 颜色改系统 token
// Issue #932：窄屏列表 / FAB / 对话框统一走共享构件
// （CompactPrimaryActionLayer / CompactPageState / CompactDialogs），
// 页面不再自绘遮罩、对话框卡片与 FAB 定位。
// Issue #972：宽屏作品页拆到 wide/WideProjectHome + wide/WideProjectCoverCard，
// 页面只把列表数据和现有 ViewModel 命令接过去；本文件不再保留旧宽屏布局 / 卡片几何。

import hilog from '@ohos.hilog'
import emitter from '@ohos.events.emitter'
import { Routes, WorkspaceRouteParams, WritingRouteParams } from '../../../app/navigation/Routes'
import { ProjectSummary } from '../../../corebridge/dto/ProjectDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { SYNC_CONTENT_CHANGED_EVENT_ID } from '../../../feature/settings/presentation/SyncOperationController'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  COMPACT_CARD_PADDING_VP,
  COMPACT_CARD_SPACING_VP,
  COMPACT_CARD_SUPPORTING_FONT_VP,
  COMPACT_CARD_TITLE_FONT_VP,
  COMPACT_LIST_BOTTOM_INSET_VP
} from '../../../ui/theme/LayoutTokens'
// Issue #932：窄屏共享构件 — 内容层 + 右下角 FAB、公共状态视图、窗口级对话框。
import { CompactPrimaryActionLayer } from '../../../ui/components/CompactPrimaryActionLayer'
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'
import { CompactConfirmDialogArgs, CompactTextDialogArgs, openCompactConfirmDialog, openCompactTextDialog } from '../../../ui/components/CompactDialogs'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ProjectListViewModel, ProjectOpenTarget } from '../presentation/ProjectListViewModel'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { ActionSlot, ActionTarget, ActionRole, ScreenRole, resolveScreenPolicy } from '../../../app/navigation/ScreenSemantics'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { NavigationHeaderActionDispatcher } from '../../../app/navigation/NavigationHeaderActionDispatcher'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { handleRootBack } from '../../../app/navigation/RootBackHandler'
// Issue #972：宽屏作品页正文（书封卡片网格 + 同尺寸「+」卡）。
import { WideProjectHome } from './wide/WideProjectHome'

// Issue #827 评论 1 第 7 点：命名框 / 确认框的内容最大宽度（vp）。
// 原来写 .width('80%')，在 1244vp 宽屏上对话框会被拉到近 1000vp 宽，
// 一行输入框横跨整屏。改成「手机上能缩、宽屏有上限」：
// 宽度 100% 加上限，窄屏按屏宽收缩，宽屏停在 DIALOG_CONTENT_MAX_WIDTH。
// Issue #932 后创建/改名/删除框已迁到窗口级 CompactDialogs（COMPACT_DIALOG_MAX_WIDTH_VP），
// 这个常量只服务留在页面内的「目录读取失败」提示框。
const DIALOG_CONTENT_MAX_WIDTH: number = 420

@ComponentV2
export struct HomeScreen {
  // Issue #768 评论5838624438：星图快捷入口改为回调切换一级 tab，不再 push 进 homeStack。
  // 星图已是一级 tab，push 进作品栈会制造 currentTab/screenRole 分叉。
  @Param onOpenStarMap: () => void = (): void => {}

  // navPathStack 通过 NavigationTransactionCoordinator 获取，不使用 @Consume（V1 装饰器，V2 中不可用）
  private navCoordinator: NavigationTransactionCoordinator = NavigationTransactionCoordinator.getInstance()

  // ViewModel — 所有 Core/NAPI 调用和状态管理都在这里（@ObservedV2 + @Trace）
  @Local viewModel: ProjectListViewModel = new ProjectListViewModel()

  // 页面 UI 状态（V2 @Local）
  @Local themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @Local adaptiveState: AdaptiveState = getAdaptiveState()
  @Local useHdsNav: boolean = false
  @Local headerActions: ActionSlot[] = []

  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  // 导航意图序号：每次点击作品或重试时递增，await 返回后只有最新序号才允许 push。
  // 解决同作品复用 Promise 时两个等待者各执行一次 pushWritingRoute 的问题。
  private openNavigationGeneration: number = 0

  async aboutToAppear() {
    hilog.info(0x0000, 'HomeScreen', 'route name=Home')
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)

    // 注册同步完成事件监听
    this.subscribeSyncEvent()

    AppContinuationService.getInstance().clear()
    this.viewModel.loadData()

    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    // Issue #905：窄屏恢复 HDS 原生标题栏，页头动作由 HdsAppDestinationApi23 的
    // stackBuilder 自绘。这里的 headerActions 只供 API < 23 的 NavDestination.menus 兜底。
    this.headerActions = resolveScreenPolicy(ScreenRole.ProjectList).actionSlots
  }

  aboutToDisappear() {
    // 页面离开后使进行中的导航意图失效，防止异步目录读取回来后仍执行 pushWritingRoute。
    this.openNavigationGeneration++
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    // 取消同步完成事件监听
    this.unsubscribeSyncEvent()
  }

  // ─── UI 事件处理（只做事件派发，不写业务逻辑） ───

  private handleRetry(): void {
    this.viewModel.retry()
  }

  private navigateToWorkspace(projectId: string, projectName: string): void {
    const navPathStack = this.navCoordinator.getNavPathStack()
    if (!navPathStack) {
      return
    }
    if (this.adaptiveState.prefersWideLayout) {
      // 宽屏：直接进入写作工作台
      this.navigateToWritingFromProject(projectId, projectName)
    } else {
      // 窄屏：保留原有卷章管理页面
      const workspaceParam: WorkspaceRouteParams = { projectId, projectName }
      navPathStack.pushPath({ name: Routes.Workspace, param: workspaceParam })
    }
  }

  // 宽屏从首页直接进入写作工作台：先读目录再决定打开哪个章节。
  // 目录读取失败时留在作品首页（ViewModel 置错误状态，页面弹「加载目录失败 + 重试」），
  // 不跳 Workspace，也不把读取失败伪装成空作品。
  // 空作品只由「成功返回的空目录」决定：chapterId 为空即 Writing 的未选章节工作台。
  // prepareOpenProject 返回确定值，页面只对当前请求结果做一次 pushPath。
  private async navigateToWritingFromProject(projectId: string, projectName: string): Promise<void> {
    this.openNavigationGeneration++
    const myGeneration: number = this.openNavigationGeneration
    const target: ProjectOpenTarget | null = await this.viewModel.prepareOpenProject(projectId, projectName)
    if (target !== null && myGeneration === this.openNavigationGeneration) {
      this.pushWritingRoute(target)
    }
  }

  // 重试入口：只重跑 getProjectTree(projectId)，成功后走同一段进 Writing 的逻辑。
  private async retryTreeLoad(): Promise<void> {
    this.openNavigationGeneration++
    const myGeneration: number = this.openNavigationGeneration
    const target: ProjectOpenTarget | null = await this.viewModel.retryOpenProject()
    if (target !== null && myGeneration === this.openNavigationGeneration) {
      this.pushWritingRoute(target)
    }
  }

  // 目录读取成功后的统一入栈：用作品 Tab 的固定栈（getTabStack(0)）而非全局活动栈，
  // 避免异步等待期间用户切到星图/统计 Tab 后 push 到错误的一级导航。
  // push 前确认当前仍在作品 Tab（getNavPathStack === getTabStack(0)），不在则放弃打开。
  // push 后立即递增 generation 消费本次意图，防止动画未完成时再次点击重复入栈。
  private pushWritingRoute(target: ProjectOpenTarget): void {
    const homeStack: NavPathStack | null = this.navCoordinator.getTabStack(0)
    if (!homeStack) {
      return
    }
    // 确认当前活动 Tab 仍是作品 Tab，否则放弃（不把写作页留在后台悄悄打开）
    const activeStack: NavPathStack | null = this.navCoordinator.getNavPathStack()
    if (activeStack !== homeStack) {
      hilog.info(0x0000, 'HomeScreen', 'pushWritingRoute: active tab is not Home, aborting push')
      return
    }
    const writingParam: WritingRouteParams = {
      projectId: target.projectId,
      chapterId: target.chapterId.length > 0 ? target.chapterId : undefined,
      chapterTitle: target.chapterTitle.length > 0 ? target.chapterTitle : undefined,
      volumeId: target.volumeId.length > 0 ? target.volumeId : undefined
    }
    homeStack.pushPath({ name: Routes.Writing, param: writingParam })
    // 消费本次导航意图，防止 push 已执行但 Navigation 动画尚未切走时再次点击又入栈
    this.openNavigationGeneration++
  }

  // Issue #768 评论5838624438：星图快捷入口改为回调切换一级 tab。
  // 星图已是一级 tab，不再 push 进 homeStack（会制造 currentTab/screenRole 分叉）。
  private navigateToStarMap(): void {
    this.onOpenStarMap()
  }

  // Issue #827 评论 1 第 3 点：删除宽屏「最近编辑」区后，navigateToRecentEdit()、
  // deleteProjectWithConfirm()、extractWorkspaceName() 已无调用方
  // （删除入口走 ViewModel 的 DeleteConfirmDialog，作品名直接取 ProjectSummary.title）。
  // 按仓库规则直接删掉，不留死代码，也不留第二套入口。

  // Issue #827 评论 1 第 3 点 / 评论 2：新建作品的唯一入口。
  // 「+」卡、窄屏 FAB、空状态入口都只调它，它只打开命名框；
  // 真正的 createProject(name) 只能从命名框的「创建」回调进入，
  // 不允许任何地方拿默认名直接建作品。
  // Issue #932：命名框改为窗口级 CompactDialogs.openCompactTextDialog，
  // ViewModel 的 showCreateDialog / newProjectName 设置与清理时机保持不变，
  // 但不再作为页面渲染开关。点遮罩关闭由 onDidDisappear 负责清理。
  private requestCreateProject(): void {
    this.viewModel.newProjectName = ''
    this.viewModel.showCreateDialog = true
    const args: CompactTextDialogArgs = {
      title: '新建作品',
      placeholder: '请输入作品名称',
      initialText: '',
      secondaryLabel: '',
      secondaryPlaceholder: '',
      secondaryInitialText: '',
      confirmLabel: '创建',
      onConfirm: (text: string, _secondaryText: string): void => {
        this.viewModel.newProjectName = text
        this.viewModel.onCreateProject()
      },
      onCancel: (): void => {
        this.viewModel.showCreateDialog = false
        this.viewModel.newProjectName = ''
      }
    }
    // 点遮罩 / 返回键关闭时同样清理创建状态。
    openCompactTextDialog(this.getUIContext(), args, (): void => {
      this.viewModel.showCreateDialog = false
      this.viewModel.newProjectName = ''
    })
  }

  // Issue #932：改名框迁到窗口级，trigger 仍是菜单里的 Rename Context action；
  // 初始文本取 ViewModel 已设置的 renameInput，确认/取消回调照旧走 ViewModel。
  private openRenameDialog(): void {
    if (this.viewModel.renameTarget === null) {
      return
    }
    const args: CompactTextDialogArgs = {
      title: '重命名作品',
      placeholder: '请输入新名称',
      initialText: this.viewModel.renameInput,
      // 作品改名只有一个输入栏。
      secondaryLabel: '',
      secondaryPlaceholder: '',
      secondaryInitialText: '',
      confirmLabel: '确认',
      onConfirm: (text: string, _secondaryText: string): void => {
        this.viewModel.renameInput = text
        this.viewModel.confirmRename()
      },
      onCancel: (): void => {
        this.viewModel.cancelRename()
      }
    }
    // 点遮罩 / 返回键关闭时同样清理 ViewModel 的重命名状态。
    openCompactTextDialog(this.getUIContext(), args, (): void => {
      this.viewModel.cancelRename()
    })
  }

  // Issue #932：删除确认框迁到窗口级；只有 ViewModel 真的拿到了 deleteTarget
  // （requiresConfirmation=true）才弹框，不需要确认的删除仍由 ViewModel 直接执行。
  private openDeleteConfirmDialog(): void {
    const target: ProjectSummary | null = this.viewModel.deleteTarget
    if (target === null) {
      return
    }
    const args: CompactConfirmDialogArgs = {
      title: '确认删除',
      message: `确定要删除"${target.title}"吗？此操作不可撤销。`,
      destructive: true,
      confirmLabel: '删除',
      onConfirm: (): void => {
        this.viewModel.confirmDelete()
      },
      onCancel: (): void => {
        this.viewModel.cancelDelete()
      }
    }
    // 点遮罩 / 返回键关闭时同样清理 ViewModel 的删除目标。
    openCompactConfirmDialog(this.getUIContext(), args, (): void => {
      this.viewModel.cancelDelete()
    })
  }

  // ─── Context Action 处理 ───

  /** 处理项目的 Context action（按 action slot 渲染的菜单项点击） */
  private handleProjectContextAction(slot: ActionSlot): void {
    const projectId = this.viewModel.currentMenuProjectId
    if (slot.role === ActionRole.Rename) {
      this.viewModel.startRenameProject(projectId)
      // Issue #932：ViewModel 设置 renameTarget 后立即打开窗口级改名框。
      this.openRenameDialog()
    } else if (slot.role === ActionRole.Delete) {
      this.viewModel.startDeleteProject(projectId, slot.requiresConfirmation)
      this.openDeleteConfirmDialog()
    }
  }

  // Issue #972：宽屏书封卡的长按菜单只回传被操作的作品，
  // 这里沿用同一组 ViewModel 命令与窗口级对话框，不新增第二套改名 / 删除流程。
  private handleWideRename(project: ProjectSummary): void {
    this.viewModel.startRenameProject(project.id)
    this.openRenameDialog()
  }

  private handleWideDelete(project: ProjectSummary): void {
    const slot: ActionSlot | null = this.viewModel.findContextAction(ActionTarget.Project, ActionRole.Delete)
    const requiresConfirmation: boolean = slot === null ? true : slot.requiresConfirmation
    this.viewModel.startDeleteProject(project.id, requiresConfirmation)
    this.openDeleteConfirmDialog()
  }

  /** 获取 action 的显示标签 */
  private getActionLabel(role: ActionRole): string {
    if (role === ActionRole.Rename) {
      return '重命名'
    } else if (role === ActionRole.Delete) {
      return '删除'
    } else if (role === ActionRole.MoveEarlier) {
      return '上移'
    } else if (role === ActionRole.MoveLater) {
      return '下移'
    }
    return role
  }

  // ─── 格式化工具 ───

  /** 检查 PrimaryAction 中是否有 CreateProject（决定 FAB 是否显示） */
  private hasCreateProjectPrimaryAction(): boolean {
    for (const slot of this.viewModel.primaryActions) {
      if (slot.role === ActionRole.CreateProject) {
        return true
      }
    }
    return false
  }

  private formatWordCount(count: number): string {
    if (count < 10000) {
      return `${count} 字`
    }
    const wan = count / 10000.0
    return `${wan.toFixed(1)} 万字`
  }

  private formatDate(isoString: string): string {
    if (!isoString) {
      return ''
    }
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)

    if (minutes < 1) {
      return '刚刚'
    } else if (minutes < 60) {
      return `${minutes}分钟前`
    } else if (hours < 24) {
      return `${hours}小时前`
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }

  private formatTime(isoString: string): string {
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)
    const days = Math.floor(diff / 86400000)

    if (minutes < 1) {
      return '刚刚'
    } else if (minutes < 60) {
      return `${minutes}分钟前`
    } else if (hours < 24) {
      return `${hours}小时前`
    } else if (days < 7) {
      return `${days}天前`
    } else {
      return `${date.getMonth() + 1}/${date.getDate()}`
    }
  }

  // ─── 同步完成事件监听 ───

  // emitter.on(event, callback) 返回 void，没有订阅 ID 可保存。
  // 保存回调函数本身，才能用 emitter.off(eventId, callback) 精确解绑本页面，
  // 不会像 emitter.off(eventId) 那样一次取消所有页面（首页 / 工作区）的订阅。
  private syncChangedHandler: ((data: emitter.EventData) => void) | null = null

  /**
   * 注册同步完成事件监听
   * 收到通知后刷新项目列表
   */
  private subscribeSyncEvent(): void {
    if (this.syncChangedHandler !== null) {
      return
    }
    const handler = (_data: emitter.EventData): void => {
      hilog.info(0x0000, 'HomeScreen', 'received sync content changed event, refreshing data')
      // 同步完成后刷新项目列表
      this.viewModel.loadData()
    }
    this.syncChangedHandler = handler
    try {
      emitter.on({ eventId: SYNC_CONTENT_CHANGED_EVENT_ID }, handler)
      hilog.info(0x0000, 'HomeScreen', 'subscribed to sync content changed event')
    } catch (e) {
      this.syncChangedHandler = null
      hilog.error(0x0000, 'HomeScreen', 'failed to subscribe sync event: %{public}s', String(e))
    }
  }

  /**
   * 取消同步完成事件监听
   */
  private unsubscribeSyncEvent(): void {
    const handler = this.syncChangedHandler
    if (handler !== null) {
      try {
        emitter.off(SYNC_CONTENT_CHANGED_EVENT_ID, handler)
        hilog.info(0x0000, 'HomeScreen', 'unsubscribed from sync content changed event')
      } catch (e) {
        hilog.error(0x0000, 'HomeScreen', 'failed to unsubscribe sync event: %{public}s', String(e))
      }
      this.syncChangedHandler = null
    }
  }

  // ========== 构建 UI ==========

  @Builder
  HomeContent() {
    Stack() {
      Column() {
        if (this.adaptiveState.prefersWideLayout) {
          // ── 宽屏：书封卡片网格，从内容区左上开始排（Issue #972） ──
          WideProjectHome({
            projects: this.viewModel.projects,
            isLoading: this.viewModel.isLoading,
            errorMessage: this.viewModel.errorMessage,
            onOpenProject: (projectId: string, projectTitle: string): void => {
              this.navigateToWorkspace(projectId, projectTitle)
            },
            onCreateProject: (): void => {
              this.requestCreateProject()
            },
            onRenameProject: (project: ProjectSummary): void => {
              this.handleWideRename(project)
            },
            onDeleteProject: (project: ProjectSummary): void => {
              this.handleWideDelete(project)
            },
            onRetry: (): void => {
              this.handleRetry()
            }
          })
        } else {
          // ── 窄屏 / 中屏：作品卡列表 + 右下 FAB ──
          this.SinglePaneLayout()
        }
      }
      .width('100%')
      .height('100%')

      // ── 对话框层 ──
      // Issue #932：创建 / 改名 / 删除框已改为窗口级 CompactDialogs（在各自 trigger 打开），
      // 页面这里不再自绘全屏遮罩层；只剩仍走页面内的「目录读取失败」提示层。

      // ── 目录读取失败提示层（宽屏点作品后 getProjectTree 失败） ──
      if (this.viewModel.openProjectError.length > 0) {
        this.OpenProjectErrorDialog()
      }
    }
    .width('100%')
    .height('100%')
    // 页面底色用系统 background_secondary。
    // 实机实测本机浅色模式：background_primary = #FFFFFF（纯白，与背景融为一体，悬浮底栏
    // 背后没有任何可折射内容）；background_secondary = #F2F3F5（带蓝相的浅冷白，
    // 正是"我的华为"那一类系统页面的底色）。书卡用 ohos_id_color_card_bg = #FFFFFF，
    // 与底色拉开 13/255 色阶。
    // 官方《组件颜色与界面背景颜色相近》faqs-arkui-1119：组件背景色要与页面背景色区分，
    // 否则组件区域无法突出显示，悬浮底栏的沉浸光感/背景模糊也无处显色。
    // 用系统 token 而非硬编码色值，深浅色模式与系统主题自动跟随。
    .backgroundColor($r('sys.color.background_secondary'))
  }

  build() {
    if (this.useHdsNav) {
      // Issue #827 评论 1 第 6 点：宽屏顶部由 PrimaryTabShell 的 WideAppShell
      // 统一提供（同步 / 搜索 / 设置 常驻右上），页面再画一条 HDS 标题栏
      // 就会在内容上方留出一条整宽空白。
      // Issue #905：窄屏恢复 HDS 原生标题栏（showTitleBar = !prefersWideLayout），
      // 与星图页 / 统计页共用同一个壳，标题栏外观保持一致。
      HdsAppDestinationApi23({
        title: $r('app.string.app_title'),
        screenRole: ScreenRole.ProjectList,
        screenTabIndex: 0,
        showTitleBar: !this.adaptiveState.prefersWideLayout
      }) {
        this.HomeContent()
      }
    } else {
      NavDestination() {
        this.HomeContent()
      }
      .hideTitleBar(this.adaptiveState.prefersWideLayout)
      .hideBackButton(true)
      .onBackPressed(() => handleRootBack())
      .title($r('app.string.app_title'))
      .menus(buildNavMenuItems(this.headerActions))
      .onShown((_reason: VisibilityChangeReason) => {
        hilog.info(0x0000, 'HomeScreen', 'Home NavDestination onShown')
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.ProjectList, 0)
      })
    }
  }

  // ────────────────────────────────────────
  // 窄屏布局（保留当前的新作品卡布局）
  // ────────────────────────────────────────

  // Issue #932：窄屏内容层 + 右下角 FAB 统一由 CompactPrimaryActionLayer 定位，
  // 删除页面自己写的 Stack(BottomEnd) + FAB margin 计算；业务回调仍是 requestCreateProject()。
  // Issue #827 评论 2：FAB 只在非宽屏出现。宽屏的「+」卡是网格成员，
  // 两边不能同时留着两套新建入口。
  @Builder
  SinglePaneLayout() {
    CompactPrimaryActionLayer({
      visible: !this.viewModel.isLoading && !this.viewModel.errorMessage && this.hasCreateProjectPrimaryAction(),
      pagePadding: this.adaptiveState.pagePadding,
      onTap: () => {
        this.requestCreateProject()
      }
    }) {
      this.SinglePaneBody()
    }
  }

  // 窄屏主体：加载 / 出错重试 / 作品列表。
  // Issue #905：窄屏标题栏恢复由 HDS 原生标题栏承载，内容区不再人工叠加安全区。
  @Builder
  SinglePaneBody() {
    Column() {
      if (this.viewModel.isLoading) {
        this.LoadingView()
      } else if (this.viewModel.errorMessage) {
        this.ErrorView()
      } else {
        this.ProjectListContent()
      }
    }
    .width('100%')
    .height('100%')
  }

  // ────────────────────────────────────────
  // 共享组件
  // ────────────────────────────────────────

  // ── 加载中 ──
  // Issue #932：加载 / 错误状态改用公共 CompactPageState，宽屏与窄屏共用同一份视觉。
  @Builder
  LoadingView() {
    CompactPageState({
      kind: CompactPageStateKind.Loading,
      title: '加载中...'
    })
  }

  // ── 错误视图 ──
  @Builder
  ErrorView() {
    CompactPageState({
      kind: CompactPageStateKind.Error,
      title: this.viewModel.errorMessage,
      onRetry: () => {
        this.handleRetry()
      }
    })
  }

  // ── 作品列表内容（窄屏） ──
  // Issue #932：空状态改用公共 CompactPageState（文案不变），
  // 作为 SinglePaneBody Column 的直接子项占满剩余高度，不再放在 Scroll 里自绘。
  // Issue #827 评论 1 第 3 点：空状态也是一个新建入口，走同一个 requestCreateProject()，
  // 不在这里直接拿默认名建作品。文案不再写死「右下角按钮」，因为宽屏根本没有 FAB，
  // 宽屏的空状态就是那张「+」卡。
  @Builder
  ProjectListContent() {
    if (this.viewModel.projects.length === 0) {
      CompactPageState({
        kind: CompactPageStateKind.Empty,
        title: '暂无作品',
        description: '点击「+」创建第一个作品'
      })
    } else {
      Scroll() {
        Column() {
          ForEach(this.viewModel.projects, (project: ProjectSummary) => {
            this.ProjectCard(project)
          }, (project: ProjectSummary) => project.id)
        }
        .width('100%')
        .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
        .padding({
          left: this.adaptiveState.pagePadding,
          right: this.adaptiveState.pagePadding,
          // Issue #905：HDS 原生标题栏自行避让状态栏（titleBar.avoidLayoutSafeArea），
          // 内容区不再人工叠加安全区 + 悬浮 Header 高度，只保留内容列表自身的内边距。
          top: this.adaptiveState.pagePadding,
          // Issue #932：列表末端留白统一用公共 token，避免最后一张卡贴住悬浮底栏。
          bottom: COMPACT_LIST_BOTTOM_INSET_VP
        })
      }
      .layoutWeight(1)
      .scrollBar(BarState.Off)
      .align(Alignment.Top)
    }
  }

  // ── 作品卡片（窄屏）：左上名/左下字数/右下日期/右上更多菜单 ──
  // Issue #932：卡片度量统一到公共 token（内边距 / 圆角 / 标题字号 / 辅助字号 / 卡间距）。
  @Builder
  ProjectCard(project: ProjectSummary) {
    Row() {
      // 左侧：作品名 + 总字数
      Column() {
        Text(project.title)
          .fontSize(COMPACT_CARD_TITLE_FONT_VP)
          .fontWeight(FontWeight.Medium)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        Text(this.formatWordCount(project.totalWordCount))
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .margin({ top: 6 })
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)

      // 右侧：更多菜单 + 最后编辑日期
      Column() {
        // 右上：更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染
        Row() {
          Image($r('sys.media.ohos_ic_public_more'))
            .width(20)
            .height(20)
            .fillColor($r('sys.color.icon_primary'))
            .onClick(() => {
              this.viewModel.currentMenuProjectId = project.id
            })
            .bindMenu(this.ProjectMoreMenu)
        }
        .width('100%')
        .justifyContent(FlexAlign.End)

        // 右下：最后编辑日期
        Text(this.formatDate(project.updatedAt))
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .margin({ top: 6 })
      }
      .width(80)
      .alignItems(HorizontalAlign.End)
    }
    .width('100%')
    .padding(COMPACT_CARD_PADDING_VP)
    // Issue #932：卡片圆角用统一的 DesignTokens.cardRadius。
    .borderRadius(DesignTokens.cardRadius)
    // 卡片用系统卡片面色（浅色模式实测 #FFFFFF），与页面底色 background_secondary 拉开色阶。
    // 官方 faqs-arkui-1119：组件背景色要与页面背景色区分，否则组件区域无法突出显示。
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .margin({ bottom: COMPACT_CARD_SPACING_VP })
    .onClick(() => this.navigateToWorkspace(project.id, project.title))
  }

  // ── 作品更多菜单 — 按 ScreenPolicy 的 Context action slot 渲染 ──
  // bindMenu 要求 CustomBuilder 无参数，通过 viewModel.currentMenuProjectId 传递上下文
  @Builder
  ProjectMoreMenu() {
    Menu() {
      ForEach(this.viewModel.getContextActions(ActionTarget.Project), (slot: ActionSlot) => {
        MenuItem({ content: this.getActionLabel(slot.role) })
          .onClick(() => this.handleProjectContextAction(slot))
      }, (slot: ActionSlot) => `${slot.role}_${slot.order}`)
    }
  }

  // ────────────────────────────────────────
  // 对话框
  // Issue #932：创建 / 改名 / 删除框已迁到窗口级 CompactDialogs
  //（openCompactTextDialog / openCompactConfirmDialog），不再有页面内自绘实现。
  // 下面只保留仍走页面内的「目录读取失败」提示框。
  // ────────────────────────────────────────

  // ── 目录读取失败对话框 ──
  // 宽屏点作品后 getProjectTree 失败：留在作品首页，给明确错误和重试；
  // 重试只重跑目录读取，成功后按原逻辑进 Routes.Writing。
  @Builder
  OpenProjectErrorDialog() {
    Column() {
      Column() {
        Text(this.viewModel.openProjectError)
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 8 })

        if (this.viewModel.openProjectName.length > 0) {
          Text(`作品「${this.viewModel.openProjectName}」，请重试。`)
            .fontSize(14)
            .margin({ bottom: 16 })
        }

        Row() {
          Button('取消')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.openNavigationGeneration++
              this.viewModel.dismissOpenProjectError()
            })

          Blank()

          Button('重试')
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.retryTreeLoad()
            })
        }
        .width('100%')
      }
      .width('100%')
      .constraintSize({ maxWidth: DIALOG_CONTENT_MAX_WIDTH })
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .padding({ left: 24, right: 24 })
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }
}
