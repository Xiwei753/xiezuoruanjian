import { Routes, StarMapRouteParams, WritingRouteParams } from '../../../app/navigation/Routes'
import { WideLayoutMode } from '../../../app/navigation/wide/WideLayoutMode'
import hilog from '@ohos.hilog'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { LocalSettings, ThemeMode } from '../../../corebridge/dto/SettingsDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { HarmonyLifecycle, AutoSaveLifecycleObserver, ShareLifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { getAppContext, getAppDataRoot, getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import {
  WritingWorkflowCoordinator,
  WritingWorkflowConfig,
  WritingWorkflowUpdate,
  WritingTextKey,
  WritingTextProvider
} from '../session/WritingWorkflowCoordinator'
import { NativeEditorSessionBridge } from '../interop/NativeEditorSessionBridge'
import { EditorStatsQueue } from '../stats/EditorStatsQueue'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorSessionSnapshot } from '../../../corebridge/dto/EditorDtos'
import { EditorInputAdapter } from '../input/EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
import { EditorSemanticDispatcher } from '../input/EditorSemanticDispatcher'
import { SujianEditor } from './SujianEditor'
import { WideWritingWorkbench } from './wide/WideWritingWorkbench'
// Issue #825 复核8第 3 点：订阅"刚落盘的 LocalSettings"广播，
// 让宽屏设置浮层的改动实时回流到本页（对齐 Linux 的 settingsBackend.onSettings_changed）。
import { LocalSettingsStore } from '../../../feature/settings/presentation/LocalSettingsStore'
import { ActiveWritingSessionRegistry } from '../session/ActiveWritingSessionRegistry'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'
import { HarmonyShareService } from '../../../platform/share/HarmonyShareService'
import { SharePayload } from '../../../platform/share/SharePayload'
import { EditorViewportState } from '../render/EditorViewportState'
import { EditorViewportController } from '../render/EditorViewportController'
import { LifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { VolumeTree } from '../../../corebridge/dto/ProjectDtos'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import {
  ScreenRole,
  ActionSlot,
  ActionRole,
  resolveScreenPolicy
} from '../../../app/navigation/ScreenSemantics'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsWritingDestinationApi23 } from '../../../app/navigation/impl/api23/HdsWritingDestinationApi23'
import { sortedHeaderTrailingActions } from '../../../app/navigation/NavigationHeaderActionModel'
import { NavigationHeaderActionDispatcher } from '../../../app/navigation/NavigationHeaderActionDispatcher'
import { EditorPresentationUpdate } from '../motion/EditorPresentationUpdate'
import { EditorMotionPolicy } from '../motion/EditorMotionPolicy'
import { ProjectWorkspaceViewModel } from '../../project/presentation/ProjectWorkspaceViewModel'
import { CorrectionCoordinator, CorrectionPresentationState } from '../correction/CorrectionCoordinator'
import { MappedCorrectionSuggestion } from '../correction/CorrectionDtos'
import { CorrectionIgnoreScope } from '../correction/ICorrectionBridge'
import { NativeCorrectionBridge } from '../correction/NativeCorrectionBridge'
import { CorrectionSuggestionsPanel } from '../correction/CorrectionSuggestionsPanel'
import { CorrectionResourceImporter } from '../correction/CorrectionResourceImporter'

// Issue #920 A：写作页不再向 Core 要七角色 bounds，布局由 ArkUI 自适应自行决定。
// Issue #932 / #972 复核：宽窄形态统一由 PrimaryTabShell 传入的 WideLayoutMode 判定，
// 不再维护写作页私有的宽屏断点（原 WORKBENCH_MIN_WIDTH_VP = 696 已删除）。
// 下面是平台自己的面板尺寸与正文最小宽度（vp），不再从 Core 的 WorkbenchLayoutPlan 取。
// 参考图：章节树占工作台约 18%，工具面板约 14%，最右工具轨约 3.8%。
const CHAPTER_NAV_WIDTH_RATIO: number = 0.18
const TOOL_PANE_WIDTH_RATIO: number = 0.14
const TOOL_RAIL_WIDTH_RATIO: number = 0.038
// 分隔条只占面板边缘，拖动手势不会覆盖正文或移动窗口。
// 参考图分栏线约 2vp；视觉把手溢出，不额外占正文宽度。
const PANE_HANDLE_WIDTH_VP: number = 2
const EDITOR_MIN_WIDTH_VP: number = 360

// 时钟刷新间隔（ms）。秒级跳动对手稿里的 HH:mm 没有意义，30s 对齐即可。
const STATUS_CLOCK_INTERVAL_MS: number = 30000
// Issue #829 评论3第2项：状态栏左段「字/分」的速度窗口（最近一分钟）。
const WRITING_SPEED_WINDOW_SECONDS: number = 60

// Issue #750 评论 5805761237：移除 EditableTitleBarV2 相关 import。
// 返回入口和标题改由 NavDestination 原生标题栏承载。

@Component
export struct WritingScreen {
  navPathStack: NavPathStack = new NavPathStack()
  // Issue #972 复核：宽窄形态由 PrimaryTabShell 唯一判定后传入，页面不再自行推断。
  @Prop layoutMode: WideLayoutMode = WideLayoutMode.Compact
  @State chapterId: string = ''
  @State chapterTitle: string = ''
  @State projectId: string = ''
  @State volumeId: string = ''
  @State content: string = ''
  @State correctionSuggestions: MappedCorrectionSuggestion[] = []
  @State correctionIsChecking: boolean = false
  @State correctionErrorMessage: string = ''
  @State correctionResourceDirectory: string = ''
  @State correctionDictionaryResourceStatus: string = ''
  @State correctionModelResourceStatus: string = ''
  @State correctionResourcesBusy: boolean = false
  @State wordCount: number = 0
  @State isLoading: boolean = true
  @State isSaving: boolean = false
  @State errorMessage: string = ''
  @State saveMessage: string = ''
  @State settings: LocalSettings = {
    editorFontSize: 16,
    editorLineSpacingMultiplier: 1.8,
    themeMode: 'system',
    appearanceMode: 'system',
    colorSource: 'built_in',
    dynamicColorEnabled: false,
    selectedBuiltinThemeId: '',
    selectedPaletteId: '',
    locale: null,
    autoSaveEnabled: true,
    autoSaveDelayMs: 30000,
    autoIndentEnabled: true,
    autoIndentWidth: 2,
    editorTypingAnimationEnabled: false,
    editorSmoothCursorEnabled: false,
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    statsDeviceId: null,
    editorCoordinatedTextCursorAnimationEnabled: false,
    diagnosticsEnabled: false,
    diagnosticsVerbose: false
  }
  @State lastSavedContent: string = ''
  // 跨设备稳定内容身份（Issue #629 评论 5 第 8 节），来自协调器回流。
  @State lastSavedContentHash: string | null = null
  @State hasUnsavedChanges: boolean = false
  @State isAutoSaving: boolean = false
  @State lastSaveFailed: boolean = false
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  // Issue #879：用单个 @State presentationUpdate 替代分离的 editorSnapshot + editorEditResult。
  @State presentationUpdate: EditorPresentationUpdate | null = null
  // Issue #879：运动策略，从 LocalSettings 构造，设置变化时同步更新。
  @State motionPolicy: EditorMotionPolicy = EditorMotionPolicy.fromSettings({
    editorFontSize: 16,
    editorLineSpacingMultiplier: 1.8,
    themeMode: 'system',
    appearanceMode: 'system',
    colorSource: 'built_in',
    dynamicColorEnabled: false,
    selectedBuiltinThemeId: '',
    selectedPaletteId: '',
    locale: null,
    autoSaveEnabled: true,
    autoSaveDelayMs: 30000,
    autoIndentEnabled: true,
    autoIndentWidth: 2,
    editorTypingAnimationEnabled: false,
    editorSmoothCursorEnabled: false,
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    statsDeviceId: null,
    editorCoordinatedTextCursorAnimationEnabled: false,
    diagnosticsEnabled: false,
    diagnosticsVerbose: false
  })
  // Issue #629 评论14 第1项：宽屏双栏模式下的目录数据。
  @State volumeTrees: VolumeTree[] = []
  // Issue #829 手稿：章节树的作品名分组头。
  @State projectTitle: string = ''
  // Issue #829：底部状态栏的时钟（HH:mm），只做展示，不参与任何业务判定。
  @State clockText: string = ''
  // Issue #829 评论3第2项：状态栏两段进度数据都来自 Core，端侧不自己算。
  @State latestCharsPerMinute: number = 0
  @State todayTypedChars: number = 0
  @State expandedVolumes: Set<string> = new Set()
  @State isTreeLoading: boolean = false
  // Issue #880 修复2：目录树加载失败标志，WideChapterTree 据此显示错误提示和重试按钮。
  @State projectTreeError: boolean = false
  // Issue #920 A：左右 pane 的展开状态只属于 ArkUI，纯平台 UI 状态，不进 Core。
  @State leftPaneCollapsed: boolean = false
  @State rightPaneCollapsed: boolean = true
  @State chapterNavWidthVp: number = 0
  @State toolPaneWidthVp: number = 0
  @State workbenchRowWidthVp: number = 0
  // Issue #829：手稿把最右 ToolRail 固定成“星图 / AI”。
  @State selectedToolKey: string = 'starmap'
  // 当前 UI 滚动位置（viewport），接续恢复时只设 UI，不写 Core。
  @State currentViewport: EditorViewportState | null = null
  // Issue #768 评论5835171266 第3项：API23+ 时用 HdsNavDestination 替代 NavDestination。
  @State useHdsNav: boolean = false
  // Issue #784 评论5857397503 第4点 / Issue #825 复核第2项：headerActions 由 WritingScreen
  // 唯一持有（窄屏页头胶囊与宽屏顶部工具条右侧动作读同一份数组）。
  @State headerActions: ActionSlot[] = []
  // Issue #825 复核第1项：窗口由窄变宽时，adaptive observer 第一次进入 Workbench
  // 也要触发一次目录树加载，不能只在 aboutToAppear 加载。
  private wideWorkbenchTreeLoaded: boolean = false
  // Issue #880 修复2：复用进行中的加载任务，避免并发请求覆盖目录状态。
  private projectTreeLoadTask: Promise<boolean> | null = null

  // Issue #825 复核8第 3 点：顶栏字号 / 行距的"编辑中草稿值"（响应式 @State）。
  @State editorFormatDraftFontSize: number = 0
  @State editorFormatDraftLineSpacing: number = 0

  // Issue #825 复核8第 3 点：订阅"刚落盘的 LocalSettings"广播。
  private unsubscribeLocalSettings: (() => void) | null = null

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  // 编辑会话协调器（唯一编辑命令入口），由协调器持有并驱动。
  private editorCoordinator: EditorSessionCoordinator = new EditorSessionCoordinator(new NativeEditorSessionBridge())
  private correctionCoordinator: CorrectionCoordinator = new CorrectionCoordinator(
    new NativeCorrectionBridge(), this.editorCoordinator)
  private correctionModelCleanupQueue: Promise<void> = Promise.resolve()
  private correctionModelCleanupDeferred: boolean = false
  private inputAdapter: EditorInputAdapter = new EditorInputAdapter(this.editorCoordinator)
  private selectionController: SelectionController = new SelectionController(this.editorCoordinator)
  private harmonyImeConnection: HarmonyImeConnection = new HarmonyImeConnection()
  // Issue #629 评论6 Part A：统一语义输入调度器。
  private dispatcher: EditorSemanticDispatcher = new EditorSemanticDispatcher(this.inputAdapter, this.selectionController, this.editorCoordinator)
  // Share 生命周期服务（Issue #629 评论 5 第 7 节）：碰一碰/隔空传送由页面拥有生命周期。
  private shareService: HarmonyShareService = HarmonyShareService.getInstance()
  // Issue #829 评论 #5995146525：写作统计上报队列（串行、异步、与编辑事务同源）。
  private editorStatsQueue: EditorStatsQueue = new EditorStatsQueue(this.bridge, 'harmony')
  // Issue #920 A：写作流程协调器。页面只把事件交给它，并映射它产出的显示状态。
  private writingTextProvider: WritingTextProvider = (key: WritingTextKey): string => this.resolveWritingText(key)
  private writingWorkflow: WritingWorkflowCoordinator = new WritingWorkflowCoordinator(
    this.bridge,
    this.editorCoordinator,
    this.dispatcher,
    this.editorStatsQueue,
    NavigationTransactionCoordinator.getInstance(),
    this.harmonyImeConnection,
    this.writingTextProvider
  )
  // Issue #920 A：卷/章节创建/删除复用工作区 ViewModel 的共享业务入口，写作页不再维护第二套规则。
  private workspaceViewModel: ProjectWorkspaceViewModel = new ProjectWorkspaceViewModel()
  // Share 生命周期观察者：onForeground 重新注册，onBackground stop（成对）。
  private shareLifecycleObserver: LifecycleObserver | null = null
  // 接续进入时携带的编辑器状态（selection/cursor/viewport/contentHash），loadChapter 恢复时消费。
  private continuationParams: WritingRouteParams | null = null
  // Issue #629 评论6 Part C：父子 viewport 恢复通道 controller。
  private viewportController: EditorViewportController = new EditorViewportController()
  // Issue #778 任务8：唯一 owner scroller，传给 SujianEditor 和 HDS 壳层做沉浸联动。
  private editorScroller: Scroller = new Scroller()
  // Issue #629 评论14 第2项：离开守卫唯一 token。
  private leaveGuardToken: number = -1
  // Issue #923：活跃写作会话注册 token（aboutToDisappear 精确注销自身）。
  private activeSessionToken: (() => void) | null = null
  // Issue #829：状态栏时钟 interval 句柄，-1 表示未启动。
  private clockTimer: number = -1

  async aboutToAppear() {
    hilog.info(0x0000, 'WritingScreen', 'route name=Writing')
    // Issue #829：底部状态栏时钟。30s 对齐一次，aboutToDisappear 里成对 clearInterval。
    this.refreshClockText()
    this.clockTimer = setInterval(() => {
      this.refreshClockText()
      // Issue #829 评论2第2项：「字/分」是分钟级速度，必须跟着时间走。
      this.refreshWritingStatusData().catch(() => {})
    }, STATUS_CLOCK_INTERVAL_MS)
    // Issue #768 评论5835171266 第3项：检测 API Level，决定是否使用 HdsNavDestination。
    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    // Issue #784 评论5857397503 第4点 + Issue #825 复核第2点：HeaderTrailing 动作统一解析一次。
    this.resolveHeaderActions()
    const paramsArr = this.navPathStack.getParamByName(Routes.Writing) as WritingRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.projectId = params.projectId || ''
      this.volumeId = params.volumeId || ''
      this.chapterId = params.chapterId || ''
      this.chapterTitle = params.chapterTitle || ''
      this.continuationParams = params
    }

    this.correctionCoordinator.setStateListener((state: CorrectionPresentationState): void => {
      this.correctionSuggestions = state.suggestions
      this.correctionIsChecking = state.isChecking
      this.correctionErrorMessage = state.errorMessage
    })
    this.correctionCoordinator.setScanCompletionListener((): void => {
      if (this.correctionResourcesBusy) {
        this.correctionModelCleanupDeferred = true
      } else {
        void this.cleanupInactiveCorrectionModels().catch(() => {})
      }
    })
    // Issue #920 A：把路由上下文交给协调器，并注册显示状态回流 + 会话打开后的平台侧恢复。
    const workflowConfig: WritingWorkflowConfig = {
      chapterId: this.chapterId,
      projectId: this.projectId,
      volumeId: this.volumeId,
      navPathStack: this.navPathStack
    }
    this.writingWorkflow.configure(workflowConfig)
    this.writingWorkflow.setSessionOpenedHook((snap: EditorSessionSnapshot): Promise<void> => {
      // Issue #958：Core 就绪、会话打开后再尝试内置模型兜底；不阻塞会话恢复，
      // 失败保持可重试（由 CorrectionCoordinator 判定是否已处理）。
      void this.ensureBundledCorrectionModel().then((): void => {
        this.refreshCorrectionResourceStatus()
      })
      return this.restoreContinuationEditorState(snap)
    })
    this.writingWorkflow.setListener((update: WritingWorkflowUpdate): void => {
      this.applyWorkflowUpdate(update)
    })

    // Issue #923：把本页工作流登记为活跃写作会话（宽窄屏共用同一实例，
    // 不按布局创建第二份同步会话）；同步事务从这里取得保存屏障与同步后的会话回填入口。
    this.activeSessionToken = ActiveWritingSessionRegistry.getInstance().register(this.writingWorkflow)

    // 宽屏左右栏初始宽度从当前工作台宽度按参考图比例推导，不写死某一台平板的 vp。
    this.initializeWidePaneWidths()

    if (params) {
      // 只要有 projectId 就加载目录树；有有效 chapterId 才加载正文和打开编辑会话。
      this.ensureWorkbenchProjectTree()
      this.refreshWritingStatusData()
      if (this.chapterId.length > 0) {
        this.loadWritingContent()
      } else {
        this.isLoading = false
      }
    }

    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
      // Issue #825 复核第1项：窗口从窄变宽时第一次进入 Workbench 也要补一次目录树。
      self.ensureWorkbenchProjectTree()
      self.reconcilePaneSpace()
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)

    // Issue #629 评论6 Part G：退后台用 performBackgroundSave（flush+idle+save）。
    // Issue #927：回调改为返回真实成功/失败；保存事务由 AutoSaveLifecycleObserver →
    // HarmonyBackgroundSaveTask 申请平台短时任务托管，页面只接收结果、不在生命周期里 await。
    this.autoSaveObserver = new AutoSaveLifecycleObserver(
      (): Promise<boolean> => this.writingWorkflow.performBackgroundSave(),
      this.settings.autoSaveDelayMs,
      this.settings.autoSaveEnabled,
      (success: boolean): void => { this.onBackgroundSaveResult(success) }
    )
    this.lifecycle.addObserver(this.autoSaveObserver)
    // 页面出现时通常已经在前台，addObserver 不会补发 onForeground，显式起一次。
    this.autoSaveObserver.reconfigure(
      this.settings.autoSaveEnabled,
      this.settings.autoSaveDelayMs,
      this.lifecycle.isForeground()
    )

    // Issue #825 复核8第 3 点：订阅 LocalSettings 落盘广播，让宽屏设置浮层的改动实时回流到本页。
    this.unsubscribeLocalSettings = LocalSettingsStore.getInstance().subscribe(
      (latest: LocalSettings): void => { this.applyPublishedSettings(latest) }
    )

    // Issue #629 评论 5 第 7 节：碰一碰/隔空传送由页面拥有生命周期。
    this.startShareLifecycle()
    const shareObserver: ShareLifecycleObserver = new ShareLifecycleObserver(
      (): void => { this.startShareLifecycle() },
      (): void => { this.stopShareLifecycle() }
    )
    this.shareLifecycleObserver = shareObserver
    this.lifecycle.addObserver(this.shareLifecycleObserver!)

    // Issue #629 评论14 第2项：注册当前 WritingScreen 的离开守卫（协调器的 performGracefulClose）。
    this.leaveGuardToken = NavigationTransactionCoordinator.getInstance().registerLeaveGuard(
      (): Promise<boolean> => this.writingWorkflow.performGracefulClose()
    )
  }

  // Issue #920 A：协调器产出的显示状态映射到页面 @State；编辑会话驱动的更新顺带重建排版快照、
  // 接续 / 分享 payload（平台侧呈现）。
  private applyWorkflowUpdate(update: WritingWorkflowUpdate): void {
    const state = update.state
    const hashChanged: boolean = state.lastSavedContentHash !== this.lastSavedContentHash
    this.content = state.content
    this.wordCount = state.wordCount
    this.hasUnsavedChanges = state.hasUnsavedChanges
    this.lastSavedContent = state.lastSavedContent
    this.lastSavedContentHash = state.lastSavedContentHash
    this.isLoading = state.isLoading
    this.errorMessage = state.errorMessage
    this.isSaving = state.isSaving
    this.isAutoSaving = state.isAutoSaving
    this.lastSaveFailed = state.lastSaveFailed
    this.saveMessage = state.saveMessage

    const snap = update.snapshot
    if (snap !== null) {
      this.correctionCoordinator.acceptSnapshot(this.projectId, this.chapterId, snap, update.editResult)
      // Issue #879：把 snapshot + editResult + sessionId 保存在一个 @State presentationUpdate 中。
      const layoutSnapshot = EditorLayoutSnapshot.fromEditorSnapshot(
        snap,
        this.settings.editorFontSize,
        this.settings.editorLineSpacingMultiplier,
        this.settings.autoIndentEnabled,
        this.settings.autoIndentWidth
      )
      this.presentationUpdate = new EditorPresentationUpdate(
        layoutSnapshot,
        update.editResult,
        this.editorCoordinator.getSessionId() ?? 0
      )
      this.updateContinuationState()
      // 内容变化时更新碰一碰/隔空传送 payload（Issue #629 评论 5 第 7 节）。
      this.updateSharePayload()
    } else {
      if (state.isLoading || state.errorMessage.length > 0) {
        this.correctionCoordinator.clear()
      }
      if (hashChanged) {
        // 保存成功后 contentHash 前进：更新接续状态（此时 hasUnsavedChanges=false 带完整 selection/cursor）。
        this.updateContinuationState()
      }
    }
  }

  // Issue #927：退后台保存结果回调。保存失败/平台不支持时，dirty 与 lastSaveFailed 由
  // WritingWorkflowCoordinator 内部状态保证（此处不清除、不另造第二套状态），页面只补日志。
  private onBackgroundSaveResult(success: boolean): void {
    if (!success) {
      hilog.warn(0x0000, 'WritingScreen', 'background save did not complete; unsaved content stays dirty')
    }
  }

  // Issue #629 评论9 第3项：aboutToDisappear 只做页面已确定离开后的清理。
  async aboutToDisappear() {
    // Issue #629 评论14 第2项：用唯一 token 注销自己的离开守卫，不影响其他 WritingScreen 实例。
    if (this.leaveGuardToken >= 0) {
      NavigationTransactionCoordinator.getInstance().unregisterLeaveGuard(this.leaveGuardToken)
      this.leaveGuardToken = -1
    }
    // Issue #923：精确注销自身的活跃会话 token（不影响切章时新页面已完成的注册）。
    if (this.activeSessionToken !== null) {
      this.activeSessionToken()
      this.activeSessionToken = null
    }
    // 清除协调器监听器（避免 close 后回调）。
    this.writingWorkflow.dispose()
    this.correctionCoordinator.dispose()

    // Issue #629 评论 5 第 7 节：页面离开时 stop 碰一碰/隔空传送（成对，不留监听）。
    this.stopShareLifecycle()

    if (this.autoSaveObserver) {
      // Issue #825 复核10：removeObserver 只从 Set 里删引用，不会停定时器，先显式 stopAutoSave()。
      this.autoSaveObserver.stopAutoSave()
      this.lifecycle.removeObserver(this.autoSaveObserver)
      this.autoSaveObserver = null
    }
    if (this.shareLifecycleObserver) {
      this.lifecycle.removeObserver(this.shareLifecycleObserver)
      this.shareLifecycleObserver = null
    }
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    // Issue #825 复核8第 3 点：退订 LocalSettings 广播（成对，不留悬挂回调）。
    if (this.unsubscribeLocalSettings) {
      this.unsubscribeLocalSettings()
      this.unsubscribeLocalSettings = null
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    // Issue #829：状态栏时钟定时器成对清理，页面离开后不留 interval。
    if (this.clockTimer >= 0) {
      clearInterval(this.clockTimer)
      this.clockTimer = -1
    }
  }

  // Issue #783 评论5855754605：把"标题栏返回"和"系统返回手势"分成两个意图。
  // 标题栏返回按钮始终走 requestLeave（退出页面）。
  // 系统返回手势在 IME 打开时先收键盘（dismissForBack），再次手势才退出页面。
  private handleTitleBack(): void {
    this.writingWorkflow.requestLeave()
  }

  private handleSystemBack(): void {
    if (this.harmonyImeConnection.canDismissForBack()) {
      this.harmonyImeConnection.dismissForBack()
      return
    }
    this.writingWorkflow.requestLeave()
  }

  // 宽屏工具面板里的星图条目复用现有星图详情路由和 Core 数据，不另造详情页。
  private openStarMapFromWorkbench(starmapId: string, starmapTitle: string): void {
    const param: StarMapRouteParams = { starmapId, starmapTitle }
    this.navPathStack.pushPath({ name: Routes.StarMap, param })
  }

  // Issue #829：HH:mm。两位补零，手稿里就是「14:26」这种宽度。只做展示。
  private refreshClockText(): void {
    const now = new Date()
    const hour = String(now.getHours()).padStart(2, '0')
    const minute = String(now.getMinutes()).padStart(2, '0')
    this.clockText = `${hour}:${minute}`
  }

  // 两段状态都失败时保持旧值：状态栏是只读展示，查询失败不该把已有数字清零。
  private async refreshWritingStatusData(): Promise<void> {
    const speed = await this.bridge.getCurrentWritingSpeed(WRITING_SPEED_WINDOW_SECONDS)
    if (speed.success && speed.data) {
      this.latestCharsPerMinute = Math.round(speed.data.charsPerMinute)
    }

    // 中段：今日纯输入字数。日历日语义只留在 Core 一处，端侧不自己拼日期。
    const summary = await this.bridge.getTodayWritingStatsSummary()
    if (summary.success && summary.data) {
      this.todayTypedChars = summary.data.totalHumanTypedChars
    }
  }

  // Issue #768 评论5838296806：先加载排版设置，再打开章节会话（避免并发时用默认设置生成 presentationUpdate）。
  private async loadWritingContent(): Promise<void> {
    await this.loadSettings()
    await this.writingWorkflow.loadChapter()
  }

  // 恢复接续编辑器状态。在 coordinator.open 成功后由协调器回调进入。
  // offset 转换方向：payload(UTF-16) → Core(UTF-8)，用 TextOffsetMapper.utf16ToUtf8。
  private async restoreContinuationEditorState(snap: EditorSessionSnapshot): Promise<void> {
    const params = this.continuationParams
    if (params === null) {
      // 非接续进入；写入当前状态供后续 onContinue 读取。
      this.updateContinuationState()
      return
    }

    // contentHash 校验：不匹配时不强套旧 offset，只恢复章节导航，以当前 Core 正文为准。
    const contentHashMatches: boolean =
      params.contentHash === undefined || params.contentHash === this.lastSavedContentHash

    if (contentHashMatches) {
      // selection 优先；无 selection 时用 cursor。offset 是 UTF-16，转成 Core UTF-8 byte offset。
      if (params.selection !== undefined && params.selection.length >= 2) {
        const anchorByte: number = TextOffsetMapper.utf16ToUtf8(snap.text, params.selection[0])
        const headByte: number = TextOffsetMapper.utf16ToUtf8(snap.text, params.selection[1])
        await this.editorCoordinator.setSelection(anchorByte, headByte)
      } else if (params.cursor !== undefined) {
        const cursorByte: number = TextOffsetMapper.utf16ToUtf8(snap.text, params.cursor)
        await this.editorCoordinator.setSelection(cursorByte, cursorByte)
      }
    }

    // viewport 只恢复到 UI 滚动位置，不写 Core。通过父子 controller 通道恢复。
    if (params.viewport !== undefined) {
      this.currentViewport = params.viewport
      this.viewportController.restore(params.viewport)
    }

    // 恢复后写入当前状态（UTF-8 → UTF-16 转换在 updateContinuationState 内完成）。
    this.updateContinuationState()
  }

  // 持续写入当前接续状态。每次章节打开、选区变化、正文编辑、viewport 变化后调用。
  private updateContinuationState(): void {
    if (!this.projectId || !this.chapterId) {
      return
    }
    const snap = this.editorCoordinator.getSnapshot()
    if (!snap) {
      return
    }
    const payload: ContinuationPayload = {
      projectId: this.projectId,
      volumeId: this.volumeId,
      chapterId: this.chapterId,
      contentHash: this.lastSavedContentHash !== null ? this.lastSavedContentHash : undefined
    }
    // 只有已保存（hasUnsavedChanges=false）时才带 selection/cursor（可精确恢复）。
    if (!this.hasUnsavedChanges) {
      const cursorUtf16: number = TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursor)
      const anchorUtf16: number = TextOffsetMapper.utf8ToUtf16(snap.text, snap.selectionAnchor)
      payload.selection = [anchorUtf16, cursorUtf16]
      payload.cursor = cursorUtf16
    }
    if (this.currentViewport !== null) {
      payload.viewport = this.currentViewport
    }
    AppContinuationService.getInstance().updateCurrentState(payload)
  }

  // ── Issue #629 评论 5 第 7 节：碰一碰/隔空传送 Share 生命周期 ──

  // 构造当前章节的 SharePayload。内容来自 coordinator snapshot（不另算一份）。
  private buildSharePayload(): SharePayload | null {
    if (!this.chapterId) {
      return null
    }
    const snap = this.editorCoordinator.getSnapshot()
    const text: string = snap ? snap.text : this.content
    const payload: SharePayload = {
      kind: 'text',
      title: this.chapterTitle,
      text: text
    }
    return payload
  }

  // 内容变化时更新碰一碰/隔空传送 payload。
  private updateSharePayload(): void {
    const payload = this.buildSharePayload()
    this.shareService.updateTapPayload(payload)
    this.shareService.updateAirTransferPayload(payload)
  }

  // 进入前台/页面显示时注册碰一碰/隔空传送监听。注册失败不抛错，下次 onForeground 重试。
  private startShareLifecycle(): void {
    const payload = this.buildSharePayload()
    if (payload === null) {
      return
    }
    this.shareService.startTapSharing(payload)
    this.shareService.startAirTransferSharing(payload).then(() => {})
  }

  // 页面离开/退后台时 stop 碰一碰/隔空传送监听（成对，不留监听）。
  private stopShareLifecycle(): void {
    this.shareService.stopTapSharing()
    this.shareService.stopAirTransferSharing().then(() => {})
  }

  // Issue #972 复核：宽屏形态只认外壳传入的 layoutMode，页面不再读取
  // 外层导航壳传入的 layoutMode；窄屏 / 中屏仍走 HDS 标题栏 + 正文 + 底部状态栏。
  private get isWideWorkbench(): boolean {
    return this.layoutMode === WideLayoutMode.Wide
  }

  // Issue #825 复核第2项 / #920 D：HeaderTrailing 动作只在这里解析一次（平台动作表）。
  private async resolveHeaderActions(): Promise<void> {
    this.headerActions = resolveScreenPolicy(ScreenRole.Writing).actionSlots
  }

  private getWorkbenchWidthVp(): number {
    return this.workbenchRowWidthVp > 0 ? this.workbenchRowWidthVp : this.adaptiveState.widthVp
  }

  // 宽屏正文由中央工作台列决定宽度，不再套 840vp 阅读栏；窄屏保持原阅读宽度。
  private editorMaxWidthVp(): number {
    return this.isWideWorkbench ? this.getWorkbenchWidthVp() : this.adaptiveState.contentMaxWidth
  }

  private toolRailWidthVp(): number {
    return this.getWorkbenchWidthVp() * TOOL_RAIL_WIDTH_RATIO
  }

  private initializeWidePaneWidths(): void {
    const width: number = this.getWorkbenchWidthVp()
    if (!this.isWideWorkbench || width <= 0) {
      return
    }
    if (this.chapterNavWidthVp <= 0) {
      this.chapterNavWidthVp = width * CHAPTER_NAV_WIDTH_RATIO
    }
    if (this.toolPaneWidthVp <= 0) {
      this.toolPaneWidthVp = width * TOOL_PANE_WIDTH_RATIO
    }
  }

  private canKeepBothPanesOpen(): boolean {
    const requiredWidth: number = this.chapterNavWidthVp + this.toolPaneWidthVp +
      PANE_HANDLE_WIDTH_VP * 2 + EDITOR_MIN_WIDTH_VP + this.toolRailWidthVp()
    return this.getWorkbenchWidthVp() >= requiredWidth
  }

  private maximumChapterNavWidthVp(): number {
    const rightWidth: number = this.rightPaneCollapsed
      ? PANE_HANDLE_WIDTH_VP
      : this.toolPaneWidthVp + PANE_HANDLE_WIDTH_VP
    const available: number = this.getWorkbenchWidthVp() - EDITOR_MIN_WIDTH_VP - this.toolRailWidthVp() -
      rightWidth - PANE_HANDLE_WIDTH_VP
    const minimum: number = this.getWorkbenchWidthVp() * 0.14
    const maximum: number = this.getWorkbenchWidthVp() * 0.34
    return Math.max(minimum, Math.min(maximum, available))
  }

  private maximumToolPaneWidthVp(): number {
    const leftWidth: number = this.leftPaneCollapsed
      ? PANE_HANDLE_WIDTH_VP
      : this.chapterNavWidthVp + PANE_HANDLE_WIDTH_VP
    const available: number = this.getWorkbenchWidthVp() - EDITOR_MIN_WIDTH_VP - this.toolRailWidthVp() -
      leftWidth - PANE_HANDLE_WIDTH_VP
    const minimum: number = this.getWorkbenchWidthVp() * 0.1
    const maximum: number = this.getWorkbenchWidthVp() * 0.28
    return Math.max(minimum, Math.min(maximum, available))
  }

  private reconcilePaneSpace(): void {
    const available: number = this.workbenchRowWidthVp
    if (available <= 0 || !this.isWideWorkbench) {
      return
    }

    if (!this.leftPaneCollapsed && !this.rightPaneCollapsed && !this.canKeepBothPanesOpen()) {
      // Keep chapter navigation available when a resized window can no longer fit both panes.
      this.rightPaneCollapsed = true
    }
    if (!this.leftPaneCollapsed && available < available * 0.14 + PANE_HANDLE_WIDTH_VP * 2 +
      EDITOR_MIN_WIDTH_VP + this.toolRailWidthVp()) {
      this.leftPaneCollapsed = true
    }
    if (!this.rightPaneCollapsed && available < available * 0.1 + PANE_HANDLE_WIDTH_VP * 2 +
      EDITOR_MIN_WIDTH_VP + this.toolRailWidthVp()) {
      this.rightPaneCollapsed = true
    }

    if (!this.leftPaneCollapsed) {
      this.chapterNavWidthVp = Math.min(this.chapterNavWidthVp, this.maximumChapterNavWidthVp())
    }
    if (!this.rightPaneCollapsed) {
      this.toolPaneWidthVp = Math.min(this.toolPaneWidthVp, this.maximumToolPaneWidthVp())
    }
  }

  // 展开/收起和栏宽均为 ArkUI 状态，不进入 Core；展开另一侧仅在当前栏宽会挤占正文时让位。
  private requestToolPaneOpen(toolKey: string): void {
    if (!this.isWideWorkbench) {
      return
    }
    this.selectedToolKey = toolKey
    if (this.rightPaneCollapsed && !this.leftPaneCollapsed && !this.canKeepBothPanesOpen()) {
      this.leftPaneCollapsed = true
    }
    this.rightPaneCollapsed = false
  }

  private requestChapterNavigationOpen(): void {
    if (!this.isWideWorkbench) {
      return
    }
    if (this.leftPaneCollapsed && !this.rightPaneCollapsed && !this.canKeepBothPanesOpen()) {
      this.rightPaneCollapsed = true
    }
    this.leftPaneCollapsed = false
  }

  // 收起方向永远放得下：只改状态。
  private closeToolPane(): void {
    this.rightPaneCollapsed = true
  }

  private closeChapterNavigation(): void {
    this.leftPaneCollapsed = true
  }

  // 拖拽分隔条：宽度仍由页面持有，组件只上报目标宽度；这里再套一次现有上下限，
  // 然后跑原有 reconcile，避免拖到挤掉正文或两侧互相重叠。
  private resizeChapterNavigation(widthVp: number): void {
    const maximum: number = this.maximumChapterNavWidthVp()
    const minimum: number = this.getWorkbenchWidthVp() * 0.14
    this.chapterNavWidthVp = Math.max(minimum, Math.min(maximum, widthVp))
    this.reconcilePaneSpace()
  }

  private resizeToolPane(widthVp: number): void {
    const maximum: number = this.maximumToolPaneWidthVp()
    const minimum: number = this.getWorkbenchWidthVp() * 0.1
    this.toolPaneWidthVp = Math.max(minimum, Math.min(maximum, widthVp))
    this.reconcilePaneSpace()
  }

  // 宽屏工作台内容行宽度变化：只更新平台侧布局状态，不改编辑会话。
  private handleWorkbenchRowWidth(widthVp: number): void {
    this.workbenchRowWidthVp = widthVp
    this.reconcilePaneSpace()
  }

  // Issue #880 复核第 2 项：宽屏工作台目录树只有这一个加载入口，在途请求复用同一个 Promise。
  private ensureWorkbenchProjectTree(): Promise<boolean> {
    if (!this.isWideWorkbench || this.wideWorkbenchTreeLoaded) {
      return Promise.resolve(this.wideWorkbenchTreeLoaded)
    }
    if (this.projectTreeLoadTask !== null) {
      return this.projectTreeLoadTask
    }
    const task: Promise<boolean> = this.loadProjectTree()
    this.projectTreeLoadTask = task
    task.finally((): void => {
      if (this.projectTreeLoadTask === task) {
        this.projectTreeLoadTask = null
      }
    })
    return task
  }

  // Issue #629 评论14 第1项：宽屏双栏模式加载项目树。
  // Issue #880 修复4b：返回 boolean，成功时才标记 wideWorkbenchTreeLoaded，失败时可重试。
  private async loadProjectTree(): Promise<boolean> {
    if (!this.projectId || !this.isWideWorkbench) {
      return false
    }
    this.isTreeLoading = true
    this.projectTreeError = false
    try {
      const result = await this.bridge.getProjectTree(this.projectId)
      if (result.success && result.data) {
        this.volumeTrees = result.data.volumes
        // Issue #829 手稿：章节树最上面一组是作品名，不是笼统的"章节"。
        this.projectTitle = result.data.project.title
        // 展开包含当前章节的卷
        const expanded: Set<string> = new Set<string>()
        this.expandedVolumes.forEach((id: string): void => {
          expanded.add(id)
        })
        for (const vt of this.volumeTrees) {
          for (const ch of vt.chapters) {
            if (ch.id === this.chapterId) {
              expanded.add(vt.volume.id)
              break
            }
          }
        }
        this.expandedVolumes = expanded
        this.wideWorkbenchTreeLoaded = true
        return true
      }
    } catch (err) {
      console.error('Failed to load project tree:', err)
    } finally {
      this.isTreeLoading = false
    }
    // 失败：保留明确错误状态，并撤销「已加载」标记（否则「重试」会被已加载判断挡掉）。
    this.projectTreeError = true
    this.wideWorkbenchTreeLoaded = false
    return false
  }

  // Issue #629 评论14 第1项：宽屏左侧切章节，流程在协调器里（先关闭当前会话再安全 replace）。
  private switchChapter(chapterId: string, chapterTitle: string, volumeId: string): void {
    this.writingWorkflow.switchChapter(chapterId, chapterTitle, volumeId).catch(() => {})
  }

  // Issue #629 评论14 第1项：展开/折叠卷。
  private toggleVolume(volumeId: string): void {
    const next: Set<string> = new Set<string>()
    this.expandedVolumes.forEach((id: string): void => {
      next.add(id)
    })
    if (next.has(volumeId)) {
      next.delete(volumeId)
    } else {
      next.add(volumeId)
    }
    // 整体赋值触发子组件 @Param 更新；Set 原地增删不保证宽屏树重绘。
    this.expandedVolumes = next
  }

  // Issue #920 A：创建/删除卷章复用 ProjectWorkspaceViewModel 的共享业务入口，
  // 写作页只负责本地文案与目录刷新，不再维护第二套创建/删除规则。
  private async createVolume(): Promise<void> {
    if (!this.projectId) {
      return
    }
    const defaultName = getContext(this).resourceManager.getStringSync($r('app.string.default_new_volume_title').id)
    const created = await this.workspaceViewModel.createVolumeIn(this.projectId, defaultName)
    if (created === null) {
      return
    }
    this.loadProjectTree()
  }

  // 创建成功后直接 switchChapter 打开新章节，不再只刷新目录树。
  private async createChapter(volumeId: string): Promise<void> {
    const defaultName = getContext(this).resourceManager.getStringSync($r('app.string.default_new_chapter_title').id)
    const created = await this.workspaceViewModel.createChapterIn(volumeId, defaultName)
    this.loadProjectTree()
    if (created === null) {
      return
    }
    this.switchChapter(created.id, created.title, volumeId)
  }

  // Issue #629 评论14 第1项：删除卷（宽屏左侧目录面板回调）— 确认后删除。
  private deleteVolume(volumeId: string): void {
    AlertDialog.show({
      title: $r('app.string.confirm_delete'),
      message: $r('app.string.confirm_delete_volume'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: { value: $r('app.string.delete'), action: () => this.doDeleteVolume(volumeId) }
    })
  }

  private async doDeleteVolume(volumeId: string): Promise<void> {
    if (!this.projectId) {
      return
    }
    const ok: boolean = await this.workspaceViewModel.deleteVolumeIn(this.projectId, volumeId)
    if (ok) {
      this.loadProjectTree()
    }
  }

  // Issue #629 评论14 第1项：删除章节（宽屏左侧目录面板回调）— 确认后删除。
  private deleteChapter(chapterId: string): void {
    AlertDialog.show({
      title: $r('app.string.confirm_delete'),
      message: $r('app.string.confirm_delete_chapter'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: { value: $r('app.string.delete'), action: () => this.doDeleteChapter(chapterId) }
    })
  }

  private async doDeleteChapter(chapterId: string): Promise<void> {
    const ok: boolean = await this.workspaceViewModel.deleteChapterIn(chapterId)
    if (ok) {
      this.loadProjectTree()
    }
  }

  async loadSettings() {
    try {
      const result = await this.bridge.getLocalSettings()
      if (result.success && result.data) {
        this.applyPublishedSettings(result.data)
      }
    } catch (err) {
      // Ignore settings load error
    }
  }

  // 把一份"已经是真相"的 LocalSettings 应用到本页：重建正文排版、同步主题、
  // 按新的自动保存间隔重配 observer，并让顶栏草稿对齐。
  private applyPublishedSettings(settings: LocalSettings): void {
    this.settings = settings
    // Issue #879：设置变化时同步更新运动策略。
    this.motionPolicy = EditorMotionPolicy.fromSettings(settings)
    // 队列里还有用户点出来的目标时不把草稿拽回旧值（pending 目标由 applyEditorFormat 队列自己收尾）。
    if (!this.hasPendingEditorFormat) {
      this.editorFormatDraftFontSize = settings.editorFontSize
      this.editorFormatDraftLineSpacing = settings.editorLineSpacingMultiplier
    }
    // Issue #768 评论5838296806：设置真相变化后，如果 coordinator 已有 snapshot，
    // 同步重建 presentationUpdate（editResult=null 表示布局重建，不是编辑事务）。
    const existingSnap = this.editorCoordinator.getSnapshot()
    if (existingSnap !== null) {
      const newLayoutSnapshot = EditorLayoutSnapshot.fromEditorSnapshot(
        existingSnap,
        this.settings.editorFontSize,
        this.settings.editorLineSpacingMultiplier,
        this.settings.autoIndentEnabled,
        this.settings.autoIndentWidth
      )
      this.presentationUpdate = new EditorPresentationUpdate(
        newLayoutSnapshot,
        null,
        this.editorCoordinator.getSessionId() ?? 0
      )
    }
    // Issue #705：主题真相是 appearanceMode（与 SettingsScreen 同一读法）。
    const appearanceMode = this.settings.appearanceMode || this.settings.themeMode || 'system'
    this.themeAdapter.updateFromSettings({
      theme: appearanceMode as ThemeMode,
      fontSize: this.settings.editorFontSize,
      fontFamily: 'HarmonyOS Sans',
      lineHeight: this.settings.editorLineSpacingMultiplier
    } as ThemeUpdateParams)

    // Issue #825 复核10：设置真相变了就地重配同一个 observer，不再 remove + new。
    this.autoSaveObserver?.reconfigure(
      this.settings.autoSaveEnabled,
      this.settings.autoSaveDelayMs,
      this.lifecycle.isForeground()
    )
  }

  private getSaveStatusColor(): ResourceColor {
    if (this.lastSaveFailed) return "#E84026"
    if (this.isSaving) return "#99000000"
    if (this.hasUnsavedChanges) return "#007DFF"
    return "#0A59F7"
  }

  private getSaveStatusText(): string {
    if (this.lastSaveFailed) return getContext(this).resourceManager.getStringSync($r('app.string.save_failed').id)
    if (this.isSaving) return getContext(this).resourceManager.getStringSync($r('app.string.saving').id)
    if (this.hasUnsavedChanges) return getContext(this).resourceManager.getStringSync($r('app.string.unsaved').id)
    return getContext(this).resourceManager.getStringSync($r('app.string.saved').id)
  }

  // 协调器文案键 → 资源字符串（资源本地化留在 UI 层）。
  private resolveWritingText(key: WritingTextKey): string {
    const rm = getContext(this).resourceManager
    if (key === WritingTextKey.AutoSaved) return rm.getStringSync($r('app.string.auto_saved').id)
    if (key === WritingTextKey.Saved) return rm.getStringSync($r('app.string.saved').id)
    if (key === WritingTextKey.SaveFailed) return rm.getStringSync($r('app.string.save_failed').id)
    if (key === WritingTextKey.SaveError) return rm.getStringSync($r('app.string.error_save').id)
    if (key === WritingTextKey.LoadChapterFailed) return rm.getStringSync($r('app.string.error_load_chapter').id)
    if (key === WritingTextKey.LoadChapterDetailFailed) return rm.getStringSync($r('app.string.error_load_chapter_detail').id)
    if (key === WritingTextKey.ClearFailed) return rm.getStringSync($r('app.string.clear_failed').id)
    if (key === WritingTextKey.Cleared) return rm.getStringSync($r('app.string.cleared').id)
    return '保存或关闭失败，未退出'
  }

  // Issue #784 评论5857856673 问题2：API<23 自定义标题 Row 右侧动作按钮的图标映射。
  private getHeaderActionIcon(role: ActionRole): Resource {
    if (role === ActionRole.Sync) {
      return $r('app.media.ic_refresh')
    }
    if (role === ActionRole.Search) {
      return $r('app.media.ic_search')
    }
    if (role === ActionRole.Settings) {
      return $r('app.media.ic_settings')
    }
    // 非预期 role 不应出现，给一个 fallback 图标避免编译报错。
    return $r('app.media.ic_settings')
  }

  async clearContent() {
    AlertDialog.show({
      title: $r('app.string.confirm_clear'),
      message: $r('app.string.confirm_clear_message'),
      primaryButton: { value: $r('app.string.cancel'), action: () => {} },
      secondaryButton: { value: $r('app.string.confirm_clear_action'), action: () => { this.doClearContent() } }
    })
  }

  // Issue #629 评论 5 第 4 节 + 评论9 第4项 + 评论11 第1项：清空正文的编排在协调器里。
  private async doClearContent(): Promise<void> {
    await this.writingWorkflow.clearDocument()
  }

  // Issue #768 评论5835171266 第3项：NavDestination 内容部分抽取为 @Builder，供两个分支共用。
  @Builder
  NavContent() {
    if (this.isWideWorkbench) {
      // Issue #972：宽屏只挂一个整页工作台。上下左右都是同一个布局流，
      // 折叠/拖动只改变列宽；编辑会话仍是本页唯一的 coordinator/dispatcher/IME。
      WideWritingWorkbench({
        headerActions: this.headerActions,
        // Issue #825 复核8第 3 点：步进器显示并基于"编辑中草稿"，不是最后一次已落盘值。
        fontSizePt: this.editorFormatDraftFontSize,
        lineSpacing: this.editorFormatDraftLineSpacing,
        onBack: (): void => { this.handleTitleBack() },
        onUndo: (): void => { this.applyUndo() },
        onRedo: (): void => { this.applyRedo() },
        onFontSizeChange: (value: number): void => { this.changeEditorFontSize(value) },
        onLineSpacingChange: (value: number): void => { this.changeEditorLineSpacing(value) },
        onHeaderAction: (role: ActionRole): void => {
          NavigationHeaderActionDispatcher.getInstance().dispatchAction(role)
        },

        projectTitle: this.projectTitle,
        volumeTrees: this.volumeTrees,
        expandedVolumes: this.expandedVolumes,
        selectedChapterId: this.chapterId,
        isTreeLoading: this.isTreeLoading,
        projectTreeError: this.projectTreeError,
        onToggleVolume: (volumeId: string): void => { this.toggleVolume(volumeId) },
        onSelectChapter: (chapterId: string, chapterTitle: string, volumeId: string): void => {
          this.switchChapter(chapterId, chapterTitle, volumeId)
        },
        onCreateVolume: (): void => { this.createVolume() },
        onCreateChapter: (volumeId: string): void => { this.createChapter(volumeId) },
        onDeleteVolume: (volumeId: string): void => { this.deleteVolume(volumeId) },
        onDeleteChapter: (chapterId: string): void => { this.deleteChapter(chapterId) },
        onRetryTree: (): void => {
          void this.ensureWorkbenchProjectTree()
        },

        hasChapter: this.chapterId.length > 0,
        leftPaneCollapsed: this.leftPaneCollapsed,
        rightPaneCollapsed: this.rightPaneCollapsed,
        chapterNavWidthVp: this.chapterNavWidthVp,
        chapterNavMinWidthVp: this.getWorkbenchWidthVp() * 0.14,
        chapterNavMaxWidthVp: this.maximumChapterNavWidthVp(),
        toolPaneWidthVp: this.toolPaneWidthVp,
        toolPaneMinWidthVp: this.getWorkbenchWidthVp() * 0.1,
        toolPaneMaxWidthVp: this.maximumToolPaneWidthVp(),
        selectedToolKey: this.selectedToolKey,
        projectId: this.projectId,
        onOpenStarMap: (starmapId: string, starmapTitle: string): void => {
          this.openStarMapFromWorkbench(starmapId, starmapTitle)
        },
        onChapterNavResize: (widthVp: number): void => { this.resizeChapterNavigation(widthVp) },
        onToolPaneResize: (widthVp: number): void => { this.resizeToolPane(widthVp) },
        onCollapseChapterTree: (): void => { this.closeChapterNavigation() },
        onExpandChapterTree: (): void => { this.requestChapterNavigationOpen() },
        onCollapseToolPane: (): void => { this.closeToolPane() },
        onExpandToolPane: (): void => { this.requestToolPaneOpen(this.selectedToolKey || 'starmap') },
        onSelectTool: (key: string): void => { this.onSelectTool(key) },
        onToggleToolPane: (): void => { this.onToggleToolPane() },
        onRowWidthChange: (widthVp: number): void => { this.handleWorkbenchRowWidth(widthVp) },

        // 硬约束 6：正文列底部只放状态信息，不承担页面导航。
        charsPerMinute: this.latestCharsPerMinute,
        progressCurrent: this.todayTypedChars,
        // Core 没有目标字数字段，不伪造分母；宽屏状态条会显示「目标未设置」。
        progressTarget: 0,
        clockText: this.clockText,
        saveStatus: this.getSaveStatusText(),
        saveStatusColor: this.getSaveStatusColor()
      }) {
        this.EditorBody()
      }
    } else {
      // Issue #629 评论9 第3项：窄屏单栏布局（或窄窗口）。
      this.EditorPane()
    }
  }

  // Issue #825 复核第 3 项：撤销/重做走唯一的 EditorSemanticDispatcher 串行队列（与输入同一条顺序边界）。
  private applyUndo(): void {
    this.dispatcher.dispatchFireAndForget({ kind: 'undo' })
  }

  private applyRedo(): void {
    this.dispatcher.dispatchFireAndForget({ kind: 'redo' })
  }

  // Issue #825 复核5第4点：ToolbarCenter 的字号 / 行距是 Core 设置里的真实字段，写回 saveLocalSettings 落盘。
  // Issue #825 复核7第 3 点：串行化保存，连点只把 pending 目标合并成最新值。
  private editorFormatSaveRunning: boolean = false
  private pendingEditorFontSize: number = 0
  private pendingEditorLineSpacing: number = 0
  private hasPendingEditorFormat: boolean = false

  private async applyEditorFormat(fontSizePt: number, lineSpacingMultiplier: number): Promise<void> {
    // 只登记目标；真正落盘由下面的串行队列执行。
    this.pendingEditorFontSize = fontSizePt
    this.pendingEditorLineSpacing = lineSpacingMultiplier
    this.hasPendingEditorFormat = true
    if (this.editorFormatSaveRunning) {
      return
    }
    this.editorFormatSaveRunning = true
    try {
      while (this.hasPendingEditorFormat) {
        const targetFontSize = this.pendingEditorFontSize
        const targetLineSpacing = this.pendingEditorLineSpacing
        this.hasPendingEditorFormat = false
        await this.saveEditorFormatOnce(targetFontSize, targetLineSpacing)
      }
    } finally {
      this.editorFormatSaveRunning = false
    }
  }

  // 单次落盘：先取最新设置真相，只覆盖字号 / 行距，存成功才回写内存状态。
  private async saveEditorFormatOnce(fontSizePt: number, lineSpacingMultiplier: number): Promise<void> {
    try {
      const latest = await this.bridge.getLocalSettings()
      if (!latest.success || !latest.data) {
        this.realignDraftToPersisted()
        return
      }
      // ArkTS 不允许对象展开（arkts-no-spread），所以就地覆盖这两个字段。
      latest.data.editorFontSize = fontSizePt
      latest.data.editorLineSpacingMultiplier = lineSpacingMultiplier
      const saved = await this.bridge.saveLocalSettings(latest.data)
      if (!saved.success) {
        this.realignDraftToPersisted()
        return
      }
      // Issue #825 复核9 第3项：成功后再从 Core 重新读一次，把重新 decode 出来的 DTO 当成"最近一次落盘"。
      const persisted = await this.bridge.getLocalSettings()
      if (persisted.success && persisted.data) {
        LocalSettingsStore.getInstance().publish(persisted.data)
      } else {
        this.applyPublishedSettings(latest.data)
      }
    } catch (err) {
      this.realignDraftToPersisted()
    }
  }

  // 保存失败时把顶栏草稿按最新已知磁盘真相回正；但有 pending 目标时保持那个目标。
  private realignDraftToPersisted(): void {
    if (this.hasPendingEditorFormat) {
      return
    }
    this.editorFormatDraftFontSize = this.settings.editorFontSize
    this.editorFormatDraftLineSpacing = this.settings.editorLineSpacingMultiplier
  }

  // 顶栏步进器的落点：先同步草稿，再把草稿目标塞进 pending 队列。
  private changeEditorFontSize(fontSizePt: number): void {
    this.editorFormatDraftFontSize = fontSizePt
    this.applyEditorFormat(this.editorFormatDraftFontSize, this.editorFormatDraftLineSpacing)
  }

  private changeEditorLineSpacing(lineSpacingMultiplier: number): void {
    this.editorFormatDraftLineSpacing = lineSpacingMultiplier
    this.applyEditorFormat(this.editorFormatDraftFontSize, this.editorFormatDraftLineSpacing)
  }

  // Issue #825 复核第 2 项：选中工具时同时请求展开 ToolPane。
  private onSelectTool(key: string): void {
    this.requestToolPaneOpen(key)
  }

  private onToggleToolPane(): void {
    if (this.rightPaneCollapsed) {
      this.requestToolPaneOpen(this.selectedToolKey || 'starmap')
    } else {
      this.closeToolPane()
    }
  }

  // Issue #972：旧宽屏分隔条 / 收起把手 / 右侧工具面板已迁到
  // ui/wide/WidePaneDivider 与 ui/wide/WideToolPanel；本页只保留布局状态和业务回调。

  build() {
    // Issue #768 评论5835171266 第3项：API23+ 用 HdsNavDestination，API < 23 保持普通 NavDestination。
    if (this.useHdsNav) {
      HdsWritingDestinationApi23({
        title: this.chapterTitle,
        onTitleBack: () => { this.handleTitleBack() },
        onSystemBack: () => { this.handleSystemBack() },
        onShown: () => {
          NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Writing, 0)
        },
        scroller: this.editorScroller,
        headerActions: this.headerActions,
        // Issue #920 A / #972：宽屏由 WideWritingWorkbench 整页排版，HDS 标题栏必须让位。
        showTitleBar: !this.isWideWorkbench
      }) {
        this.NavContent()
      }
    } else {
      NavDestination() {
        Column() {
          // 宽屏这条自绘标题行让位给 WideWritingWorkbench 的顶部工具条。
          if (!this.isWideWorkbench) {
            // Issue #783 评论5856245403：自定义标题行 — 圆形返回按钮 + 标题。
            Row() {
              Button({ type: ButtonType.Circle, stateEffect: true }) {
                SymbolGlyph($r('sys.symbol.chevron_left'))
                  .fontSize(24)
              }
              .width(40)
              .height(40)
              .onClick(() => this.handleTitleBack())

              Text(this.chapterTitle)
                .fontSize(20)
                .margin({ left: 12 })

              Blank()

              ForEach(sortedHeaderTrailingActions(this.headerActions), (slot: ActionSlot) => {
                Button({ type: ButtonType.Circle, stateEffect: true }) {
                  Image(this.getHeaderActionIcon(slot.role))
                    .width(24)
                    .height(24)
                    .fillColor($r('sys.color.icon_primary'))
                }
                .width(40)
                .height(40)
                .onClick(() => {
                  NavigationHeaderActionDispatcher.getInstance().dispatchAction(slot.role)
                })
              }, (slot: ActionSlot) => slot.role)
            }
            .width('100%')
            .padding({ left: 12, right: 12, top: 8, bottom: 8 })
          }

          Stack() {
            this.NavContent()
          }
          .layoutWeight(1)
        }
      }
      // Issue #783 评论5856245403：隐藏内置标题栏，用自定义标题 Row 替代。
      .hideTitleBar(true)
      .onBackPressed(() => {
        // 系统返回手势（包括全面屏侧滑返回）：IME 打开时先收键盘，否则退出页面。
        this.handleSystemBack()
        return true
      })
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Writing, 0)
      })
    }
  }

  // Issue #629 评论14 第1项：编辑区域 Builder（窄屏和宽屏复用同一套编辑会话）。
  // Issue #829 评论2第1项：正文主体抽成不含任何状态栏的 EditorBody()。
  @Builder
  EditorBody() {
    Column() {
      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
          Text($r('app.string.loading'))
            .fontSize(14)
            .margin({ top: 12 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .margin({ bottom: 16 })

          Button($r('app.string.retry'))
            .onClick(() => { this.writingWorkflow.loadChapter() })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        Column() {
          CorrectionSuggestionsPanel({
            suggestions: this.correctionSuggestions,
            isChecking: this.correctionIsChecking,
            errorMessage: this.correctionErrorMessage,
            resourceDirectory: this.correctionResourceDirectory,
            dictionaryResourceStatus: this.correctionDictionaryResourceStatus,
            modelResourceStatus: this.correctionModelResourceStatus,
            isPreparingResources: this.correctionResourcesBusy,
            onCheckChapter: (): void => { void this.correctionCoordinator.checkChapter() },
            onImportModel: (): void => { void this.importCorrectionModel() },
            onImportDictionary: (): void => { void this.importCorrectionDictionary() },
            onApply: (suggestion: MappedCorrectionSuggestion): void => { void this.applyCorrection(suggestion) },
            onIgnore: (suggestion: MappedCorrectionSuggestion, scope: CorrectionIgnoreScope): void => {
              void this.ignoreCorrection(suggestion, scope)
            },
            onAddDictionary: (suggestion: MappedCorrectionSuggestion): void => {
              void this.addCorrectionDictionaryEntry(suggestion)
            }
          })
            .width('100%')
            .constraintSize({ maxWidth: this.editorMaxWidthVp() })

          SujianEditor({
            scroller: this.editorScroller,
            presentationUpdate: this.presentationUpdate,
            correctionSuggestions: this.correctionSuggestions,
            motionPolicy: this.motionPolicy,
            harmonyImeConnection: this.harmonyImeConnection,
            inputAdapter: this.inputAdapter,
            selectionController: this.selectionController,
            dispatcher: this.dispatcher,
            viewportController: this.viewportController,
            onViewportChange: (state: EditorViewportState): void => {
              this.currentViewport = state
              this.updateContinuationState()
            }
          })
            .width('100%')
            .layoutWeight(1)
        }
        .width('100%')
        .layoutWeight(1)
        .constraintSize({ maxWidth: this.editorMaxWidthVp() })
      }
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Center)
  }

  private async applyCorrection(suggestion: MappedCorrectionSuggestion): Promise<void> {
    const result = await this.correctionCoordinator.apply(suggestion)
    if (!result.success) {
      this.correctionErrorMessage = result.userMessage ?? result.errorCode ?? '校对建议已过期，请重新检查'
    }
  }

  /**
   * Issue #958：内置 KenLM 模型兜底。用户导入的模型始终优先；只有 Core 尚未
   * 配置模型时才把 HAP 内置模型流式复制到沙箱并交给 Core（协调器每会话只做一次）。
   * 资源管理器沿用现有 UIAbilityContext 获取方式；失败只记 warning，不阻断编辑。
   */
  private async ensureBundledCorrectionModel(): Promise<void> {
    const context = getAppContext().getAbilityContext()
    const appDataRoot = getAppDataRoot()
    if (context === null || appDataRoot.length === 0) {
      return
    }
    const result = await this.correctionCoordinator.ensureBundledModel(
      (): Promise<string> => CorrectionResourceImporter.ensureBundledModel(context.resourceManager, appDataRoot))
    if (!result.success) {
      hilog.warn(0x0000, 'WritingScreen', 'bundled correction model fallback skipped: %{public}s',
        result.userMessage ?? result.errorCode ?? 'unknown')
    }
  }

  private async refreshCorrectionResourceStatus(): Promise<void> {
    const snapshot = CorrectionResourceImporter.inspect(getAppDataRoot())
    this.correctionResourceDirectory = snapshot.directory
    const result = await this.correctionCoordinator.getResourceStatus()
    if (!result.success || result.data === undefined) {
      this.correctionDictionaryResourceStatus =
        `无法读取 Core 词库状态：${result.userMessage ?? result.rawError ?? '未知错误'}`
      this.correctionModelResourceStatus =
        `无法读取 Core 模型状态：${result.userMessage ?? result.rawError ?? '未知错误'}`
      return
    }
    const status = result.data
    const missingDictionaries = status.missingDictionaryFiles
    const dictionaryReadiness = missingDictionaries.length > 0
      ? `可选词典 ${status.installedDictionaryFiles.length}/8 已安装；未安装：${missingDictionaries.join('、')}`
      : '八种可选词典均已安装'
    this.correctionDictionaryResourceStatus = status.bundledLexiconAvailable
      ? `Core 内置词库已就绪；${dictionaryReadiness}`
      : 'Core 内置词库不可用'
    const modelName = status.modelPath === null
      ? ''
      : status.modelPath.substring(status.modelPath.lastIndexOf('/') + 1)
    this.correctionModelResourceStatus = status.modelFileAvailable
      ? `Core 已配置可读取的模型${modelName.length > 0 ? `：${modelName}` : '文件'}`
      : status.modelConfigured
        ? 'Core 已配置模型路径，但模型文件不可用'
        : '尚未配置语言模型；内置固定规则仍可用'
  }

  private cleanupInactiveCorrectionModels(allowDuringImport: boolean = false): Promise<string> {
    let lifecycleMessage = ''
    const task = this.correctionModelCleanupQueue.then(async (): Promise<void> => {
      lifecycleMessage = await this.performInactiveCorrectionModelCleanup(allowDuringImport)
    })
    this.correctionModelCleanupQueue = task.then((): void => {}, (): void => {})
    return task.then((): string => lifecycleMessage,
      (): string => '；清理状态无法确认，旧模型均已保留')
  }

  private async performInactiveCorrectionModelCleanup(allowDuringImport: boolean): Promise<string> {
    const appDataRoot = getAppDataRoot()
    if (appDataRoot.length === 0) {
      return '；无法获取应用数据目录，旧模型均已保留'
    }
    const currentStatus = await this.correctionCoordinator.getResourceStatus()
    if (!currentStatus.success || currentStatus.data === undefined) {
      return '；无法读取 Core 活动模型路径，旧模型均已保留'
    }
    const currentModelPath = currentStatus.data.modelPath
    const currentModelName = currentModelPath === null
      ? ''
      : currentModelPath.substring(currentModelPath.lastIndexOf('/') + 1)
    const snapshot = CorrectionResourceImporter.inspect(appDataRoot)
    let managedModelCount = 0
    let removedModels = 0
    let retainedModels = 0
    for (let i = 0; i < snapshot.modelFiles.length; i++) {
      const name = snapshot.modelFiles[i]
      if (!CorrectionResourceImporter.isManagedModelFile(name)) {
        continue
      }
      const path = `${snapshot.directory}/${name}`
      if (currentModelPath !== null && (path === currentModelPath || name === currentModelName)) {
        continue
      }
      managedModelCount++
      if (this.correctionResourcesBusy && !allowDuringImport) {
        retainedModels++
        continue
      }
      const pathStatus = await this.correctionCoordinator.getModelPathStatus(path)
      if (this.correctionResourcesBusy && !allowDuringImport) {
        retainedModels++
        continue
      }
      if (pathStatus.success && pathStatus.data !== undefined &&
          CorrectionResourceImporter.removeModelIfEligible(appDataRoot, path, pathStatus.data)) {
        removedModels++
      } else {
        retainedModels++
      }
    }
    if (managedModelCount === 0) {
      return ''
    }
    let message = removedModels > 0
      ? `；Core 确认并清理了 ${removedModels} 个无活动引用的旧模型`
      : ''
    if (retainedModels > 0) {
      message += `；${retainedModels} 个旧模型仍由 Core 保留`
    }
    return message
  }

  private runDeferredCorrectionModelCleanup(): void {
    if (!this.correctionModelCleanupDeferred) {
      return
    }
    this.correctionModelCleanupDeferred = false
    void this.cleanupInactiveCorrectionModels().catch(() => {})
  }

  private async importCorrectionModel(): Promise<void> {
    if (this.correctionResourcesBusy) {
      return
    }
    const context = getAppContext().getAbilityContext()
    const appDataRoot = getAppDataRoot()
    if (context === null || appDataRoot.length === 0) {
      this.correctionModelResourceStatus = '无法获取应用目录或 UIAbilityContext，模型未导入'
      return
    }
    this.correctionResourcesBusy = true
    try {
      const result = await CorrectionResourceImporter.importModel(
        context,
        appDataRoot,
        (path: string) => this.correctionCoordinator.setModelPath(path),
        (message: string): void => { this.correctionModelResourceStatus = message }
      )
      let lifecycleMessage = ''
      if (result.status === 'imported') {
        lifecycleMessage = await this.cleanupInactiveCorrectionModels(true)
      }
      await this.refreshCorrectionResourceStatus()
      if (result.status === 'imported') {
        this.correctionModelResourceStatus = `${result.message}${lifecycleMessage}`
      } else if (result.status === 'cancelled') {
        this.correctionModelResourceStatus = `模型配置未改变；${result.message}`
      } else {
        this.correctionModelResourceStatus = `${result.message}；${this.correctionModelResourceStatus}`
      }
    } finally {
      this.correctionResourcesBusy = false
      this.runDeferredCorrectionModelCleanup()
    }
  }

  private async importCorrectionDictionary(): Promise<void> {
    if (this.correctionResourcesBusy) {
      return
    }
    const context = getAppContext().getAbilityContext()
    const appDataRoot = getAppDataRoot()
    if (context === null || appDataRoot.length === 0) {
      this.correctionDictionaryResourceStatus = '无法获取应用目录或 UIAbilityContext，词典未导入'
      return
    }
    this.correctionResourcesBusy = true
    try {
      const result = await CorrectionResourceImporter.importDictionary(
        context,
        appDataRoot,
        (message: string): void => { this.correctionDictionaryResourceStatus = message }
      )
      await this.refreshCorrectionResourceStatus()
      this.correctionDictionaryResourceStatus = `${result.message}；${this.correctionDictionaryResourceStatus}`
    } finally {
      this.correctionResourcesBusy = false
      this.runDeferredCorrectionModelCleanup()
    }
  }

  private async ignoreCorrection(suggestion: MappedCorrectionSuggestion, scope: CorrectionIgnoreScope): Promise<void> {
    const result = await this.correctionCoordinator.ignore(suggestion, scope)
    if (!result.success) {
      this.correctionErrorMessage = result.userMessage ?? result.errorCode ?? '忽略操作未完成'
    }
  }

  private async addCorrectionDictionaryEntry(suggestion: MappedCorrectionSuggestion): Promise<void> {
    const result = await this.correctionCoordinator.addToDictionary(suggestion)
    if (!result.success) {
      this.correctionErrorMessage = result.userMessage ?? result.errorCode ?? '加入词典未完成'
    }
  }

  // 窄屏专用底栏（Issue #767 评论 5828146609 第 2 节：字数显示合并到底部保存状态栏）。
  @Builder
  NarrowStatusBar() {
    Row() {
        Text(getContext(this).resourceManager
          .getStringSync($r('app.string.word_count_format').id)
          .replace('%s', String(this.wordCount)))
          .fontSize(12)
        if (this.settings.autoSaveEnabled) {
          Text($r('app.string.auto_save'))
            .fontSize(12)
            .margin({ left: 8 })
        }

        Blank()

        Text(this.getSaveStatusText())
          .fontSize(12)
          .fontColor(this.getSaveStatusColor())
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 10, bottom: 10 })
  }

  // 窄屏编辑区 = 正文主体 + 窄屏旧底栏。
  @Builder
  EditorPane() {
    Column() {
      if (this.chapterId.length === 0) {
        Column() {
          Text('请先选择或创建章节')
            .fontSize(16)
            .fontColor($r('sys.color.font_secondary'))
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        this.EditorBody()
      }
      this.NarrowStatusBar()
    }
    .width('100%')
    .height('100%')
    // Issue #932 复核：窄屏编辑区根容器补页面底色，避免 HDS NavDestination 的内部 surface 透出白底。
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
