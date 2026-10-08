import { Routes, WritingRouteParams } from '../../../app/navigation/Routes'
import hilog from '@ohos.hilog'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { ChapterContent, ChapterSaveReceipt } from '../../../corebridge/dto/ProjectDtos'
import { LocalSettings, ThemeMode } from '../../../corebridge/dto/SettingsDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { HarmonyLifecycle, AutoSaveLifecycleObserver, ShareLifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { EditorSessionCoordinator, EditorStateUpdate } from '../session/EditorSessionCoordinator'
import { NativeEditorSessionBridge } from '../interop/NativeEditorSessionBridge'
import { EditorStatsQueue } from '../stats/EditorStatsQueue'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { EditorInputAdapter } from '../input/EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
import { EditorSemanticDispatcher } from '../input/EditorSemanticDispatcher'
import { SujianEditor } from './SujianEditor'
import { WritingWorkbenchToolbar } from './WritingWorkbenchToolbar'
// Issue #825 复核8第 3 点：订阅"刚落盘的 LocalSettings"广播，
// 让宽屏设置浮层的改动实时回流到本页（对齐 Linux 的 settingsBackend.onSettings_changed）。
import { LocalSettingsStore } from '../../../feature/settings/presentation/LocalSettingsStore'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'
import { HarmonyShareService } from '../../../platform/share/HarmonyShareService'
import { SharePayload } from '../../../platform/share/SharePayload'
import { EditorViewportState } from '../render/EditorViewportState'
import { EditorViewportController } from '../render/EditorViewportController'
import { LifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { ChapterTreePane } from '../../project/ui/ChapterTreePane'
import { PaneFoldButton } from './PaneFoldButton'
import { WritingStatusBar } from './WritingStatusBar'
import { VolumeTree } from '../../../corebridge/dto/ProjectDtos'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import {
  ScreenRole,
  ActionSlot,
  ActionRole,
  WorkspaceLayoutMode,
  WorkbenchLayoutPlan,
  WorkbenchRole,
  WorkbenchVisibility
} from '../../../corebridge/dto/PlatformDtos'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsWritingDestinationApi23 } from '../../../app/navigation/impl/api23/HdsWritingDestinationApi23'
import { sortedHeaderTrailingActions } from '../../../app/navigation/NavigationHeaderActionModel'
import { NavigationHeaderActionDispatcher } from '../../../app/navigation/NavigationHeaderActionDispatcher'
import { ScreenPolicyBridge } from '../../../ui/adaptive/ScreenPolicyBridge'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  getWorkbenchLayoutBridge,
  workbenchBounds,
  isWorkbenchMode
} from '../../../ui/adaptive/WorkbenchLayoutBridge'
import { WritingToolRail } from './WritingToolRail'
import { EditorPresentationUpdate } from '../motion/EditorPresentationUpdate'
import { EditorMotionPolicy } from '../motion/EditorMotionPolicy'

// Issue #750 评论 5805761237：移除 EditableTitleBarV2 相关 import。
// EditableTitleBarV2 自身无 systemMaterial 配置项，塞在内容区无法获得
// Navigation/NavDestination 标题栏的沉浸光感材质；返回入口和标题改由
// NavDestination 原生标题栏（hideTitleBar(false) + title）承载。

// Issue #825 复核第 2 项：目录栏 / 工具 pane / rail 的宽度不再在这里硬编码。
// 三个宽度都读 Core `resolve_workbench_layout` 返回的对应角色 bounds；
// 下面只是 plan 还没回来时的占位，plan 一到就被真实 bounds 覆盖。
const FALLBACK_CHAPTER_NAV_WIDTH_VP: number = 320
const FALLBACK_TOOL_PANE_WIDTH_VP: number = 240
const FALLBACK_TOOL_RAIL_WIDTH_VP: number = 56
// plan 还没回来时的工具条带高度占位；plan 一到就被 Core 的 ToolbarCenter bounds 覆盖。
const FALLBACK_TOOLBAR_HEIGHT_VP: number = 64
// 贴边折叠把手的尺寸（vp）。把手不是 Core 角色，只是 pane 边界上的 UI 装饰。
const PANE_HANDLE_WIDTH_VP: number = 18
const PANE_HANDLE_HEIGHT_VP: number = 64

// Issue #829 手稿：底部状态栏中段「434 / 2,000」的分母。
// Core 目前没有目标字数字段（settings 里只有 desktop_sidebar_width /
// desktop_editor_width 这类布局项），所以这一轮按手稿取 2000 作为占位，
// 不在端侧造一个假的 Core 字段。Core 加了目标字数设置后，这里改成读 Core。
const WRITING_PROGRESS_TARGET_PLACEHOLDER: number = 2000
// 时钟刷新间隔（ms）。秒级跳动对手稿里的 HH:mm 没有意义，30s 对齐即可。
const STATUS_CLOCK_INTERVAL_MS: number = 30000
// Issue #829 评论3第2项：状态栏左段「字/分」的速度窗口。
// 手稿的「86 字/分」是"最近一分钟的写入速度"，所以窗口取 60 秒，由 Core
// 以「现在」为终点重算（不是取历史速度曲线的最后一桶，那样停笔后不会归零）。
const WRITING_SPEED_WINDOW_SECONDS: number = 60

// Issue #629 评论 5308748920 问题3：保存事务记录（命名 interface，避免对象字面量类型）。
interface ActiveSaveTask {
  text: string
  task: Promise<boolean>
}

@Component
export struct WritingScreen {
  navPathStack: NavPathStack = new NavPathStack()
  @State chapterId: string = ''
  @State chapterTitle: string = ''
  @State projectId: string = ''
  @State volumeId: string = ''
  @State content: string = ''
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
    windowWidth: 0,
    windowHeight: 0,
    editorTypingAnimationEnabled: false,
    editorSmoothCursorEnabled: false,
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    statsDeviceId: null,
    desktopSidebarWidth: 0,
    desktopEditorWidth: 0,
    editorCoordinatedTextCursorAnimationEnabled: false,
    diagnosticsEnabled: false,
    diagnosticsVerbose: false
  }
  @State lastSavedContent: string = ''
  // 跨设备稳定内容身份（Issue #629 评论 5 第 8 节）。
  // 加载章节时来自 ChapterContent.meta.hash；保存成功时来自 ChapterSaveReceipt.contentHash。
  // 不用 TextEditSession.revision（每次新建会话都是 initial()，无跨设备比较意义）。
  @State lastSavedContentHash: string | null = null
  @State hasUnsavedChanges: boolean = false
  @State isAutoSaving: boolean = false
  @State lastSaveFailed: boolean = false
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  // Issue #829 评论 #5995146525：原先的 sessionOldText / sessionId / sessionStartTime
  // 已删除。它们只服务于「保存时拿整章 old/new 文本比较」的旧统计上报；
  // 写作统计改由 EditorStatsQueue 按编辑事务上报后，会话 id 与时长口径由 Core 侧管理。
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  @State editorSnapshot: EditorLayoutSnapshot | null = null
  // Issue #879：保存 editResult 供运动层消费（构造 EditorPresentationUpdate 时需要）。
  @State editorEditResult: EditorEditResult | null = null
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
    windowWidth: 0,
    windowHeight: 0,
    editorTypingAnimationEnabled: false,
    editorSmoothCursorEnabled: false,
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    statsDeviceId: null,
    desktopSidebarWidth: 0,
    desktopEditorWidth: 0,
    editorCoordinatedTextCursorAnimationEnabled: false,
    diagnosticsEnabled: false,
    diagnosticsVerbose: false
  })
  // Issue #629 评论14 第1项：宽屏双栏模式下的目录数据。
  // WritingScreen 在宽屏模式下持有 ChapterTreePane，需要加载项目树。
  @State volumeTrees: VolumeTree[] = []
  // Issue #829 手稿：章节树的作品名分组头 + 底部章纲分组头。
  @State projectTitle: string = ''
  // Issue #829：底部状态栏的时钟（HH:mm），只做展示，不参与任何业务判定。
  @State clockText: string = ''
  // Issue #829 评论3第2项：状态栏两段进度数据都来自 Core，端侧不自己算。
  //   latestCharsPerMinute = Core「当前写作速度」的 charsPerMinute（手稿「86 字/分」）。
  //   todayTypedChars      = 今日纯输入字数（中段分子，和 Linux_Qt 同一份 Core 汇总语义）。
  @State latestCharsPerMinute: number = 0
  @State todayTypedChars: number = 0
  @State expandedVolumes: Set<string> = new Set()
  @State isTreeLoading: boolean = false
  // Issue #825 复核第 2 项：左右 pane 的展开状态只属于 ArkUI，不进入 Core 编辑状态，
  // 只作为 WorkbenchVisibility 输入让 Core 重算七角色 bounds。
  @State leftPaneCollapsed: boolean = false
  @State rightPaneCollapsed: boolean = true
  // Issue #829：手稿把最右 ToolRail 固定成“星图 / AI”。
  // 骨架默认选中星图；ToolPane 是否展开仍由 rightPaneCollapsed 单独控制。
  @State selectedToolKey: string = 'starmap'
  // Core 下发的七角色 bounds 计划；null 表示还没拿到（此时用 FALLBACK_* 占位）。
  @State workbenchPlan: WorkbenchLayoutPlan | null = null
  // 当前 UI 滚动位置（viewport），接续恢复时只设 UI，不写 Core。
  // 强类型 EditorViewportState，不用裸 object（Issue #629 评论 5 第 8 节）。
  @State currentViewport: EditorViewportState | null = null
  // Issue #768 评论5835171266 第3项：API23+ 时用 HdsNavDestination 替代 NavDestination，
  // 获得 HDS 沉浸光感材质效果。
  @State useHdsNav: boolean = false
  // Issue #784 评论5857397503 第4点 / Issue #825 复核第2项：headerActions 由 WritingScreen
  // 唯一持有（不再分 API 等级各解析一次），窄屏页头胶囊与宽屏右侧工具区读同一份数组。
  @State headerActions: ActionSlot[] = []
  // Issue #825 复核第1项：窗口由窄变宽时，adaptive observer 第一次进入 Workbench
  // 也要触发一次目录树加载，不能只在 aboutToAppear 加载。
  private wideWorkbenchTreeLoaded: boolean = false
  // Issue #825 复核第 4 项：resolve_workbench_layout 是异步的，用递增 generation
  // 保证只有最后一次请求能写 workbenchPlan，旧响应不能把布局改回旧尺寸。
  private workbenchPlanGeneration: number = 0

  // Issue #825 复核8第 3 点：顶栏字号 / 行距的"编辑中草稿值"。
  // 步进器的下一步必须从草稿算，而不是从"最后一次已落盘值"算——
  // 否则连点三次「+」时三次都从同一个旧值出发，pending 最后仍只 +1 步。
  // Issue #825 复核9 第1项：草稿必须是响应式状态。
  // WritingScreen 是 V1 @Component，只有 @State 变化才会触发 build，
  // 普通 private 字段改了不会重建 WritingWorkbenchToolbar（它是 @ComponentV2，
  // @Param 收不到新值），于是快速连点会一直按同一个旧值算下一步。
  @State editorFormatDraftFontSize: number = 0
  @State editorFormatDraftLineSpacing: number = 0

  // Issue #825 复核8第 3 点：订阅"刚落盘的 LocalSettings"广播。
  // 宽屏设置浮层盖在本页之上，用户在里面改完字号/主题后这里必须跟着重建排版，
  // 和 Linux 的 settingsBackend.onSettings_changed 同一语义。
  private unsubscribeLocalSettings: (() => void) | null = null

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private autoSaveObserver: AutoSaveLifecycleObserver | null = null
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private coordinator: EditorSessionCoordinator = new EditorSessionCoordinator(new NativeEditorSessionBridge())
  private inputAdapter: EditorInputAdapter = new EditorInputAdapter(this.coordinator)
  private selectionController: SelectionController = new SelectionController(this.coordinator)
  private harmonyImeConnection: HarmonyImeConnection = new HarmonyImeConnection()
  // Issue #629 评论6 Part A：统一语义输入调度器。
  // 物理键盘/IME/鼠标/触摸都经此 dispatcher 排队，出队时才读 Coordinator snapshot。
  private dispatcher: EditorSemanticDispatcher = new EditorSemanticDispatcher(this.inputAdapter, this.selectionController, this.coordinator)
  // Share 生命周期服务（Issue #629 评论 5 第 7 节）：碰一碰/隔空传送由页面拥有生命周期。
  private shareService: HarmonyShareService = HarmonyShareService.getInstance()
  // Issue #829 评论 #5995146525：写作统计上报队列（串行、异步、与编辑事务同源）。
  // state listener 里 enqueueChange 只入内存队列，由队列单协程按序跨 NAPI 写 Core，
  // 不在编辑主链上同步写盘。会话 id / 时长口径由队列自己持有。
  private editorStatsQueue: EditorStatsQueue = new EditorStatsQueue(this.bridge, 'harmony')
  // Share 生命周期观察者：onForeground 重新注册，onBackground stop（成对）。
  private shareLifecycleObserver: LifecycleObserver | null = null
  // 接续进入时携带的编辑器状态（selection/cursor/viewport/contentHash），loadChapter 恢复时消费。
  private continuationParams: WritingRouteParams | null = null
  // Issue #629 评论6 Part C：父子 viewport 恢复通道 controller。
  // WritingScreen 持有 controller 实例并注入 SujianEditor；接续加载完成、目标设备 layout 就绪后
  // 调 controller.restore(state)，controller 按目标设备当前 viewport/content 尺寸 clamp + scrollTo。
  private viewportController: EditorViewportController = new EditorViewportController()
  // Issue #778 任务8：唯一 owner scroller，传给 SujianEditor 和 HDS 壳层做沉浸联动。
  private editorScroller: Scroller = new Scroller()
  // Issue #629 评论7 第5项：幂等关闭事务 Promise。performGracefulClose 复用此 Promise，
  // 避免返回按钮 + aboutToDisappear 双重执行真实关闭链，防止重复统计。
  // Issue #629 评论8 第2项：改为 Promise<boolean>；关闭失败（保存失败/close 失败）时重置，
  // 允许下次返回重试（否则失败后返回按钮永远无法再 pop）。成功关闭后页面即销毁，保留缓存。
  private closeTask: Promise<boolean> | null = null
  // Issue #629 评论 5308748920 问题3：保存事务复用 + 精确目标文本。
  // activeSave 记录当前正在保存哪份正文（text）和对应任务（task）。
  // 后续 ensureSnapshotSaved 调用先 await 旧任务；若旧任务保存的正是目标 text 且成功，直接 true；
  // 若目标不同，旧任务结束后再启动一次精确保存 targetText 的新事务。
  // 不设固定重试次数，不靠 retry 次数保证落盘。
  private activeSave: ActiveSaveTask | null = null
  // Issue #629 评论 5308748920 问题2：离开事务（close + pop）整体幂等。
  // 已有 leaveTask 时直接返回同一 task，task 内只 pop 一次。
  // close 失败时清 leaveTask 允许再次返回重试；成功后不重置（页面已离开）。
  // Issue #783 评论5855754605：改为 Promise<boolean>；关闭事务返回 false 时，
  // 页面没有任何变化，需要给用户提示"保存或关闭失败，未退出"。
  private leaveTask: Promise<boolean> | null = null
  // Issue #629 评论14 第2项：离开守卫唯一 token。registerLeaveGuard 返回，
  // aboutToDisappear 用此 token 注销，避免误注销其他 WritingScreen 实例的 guard。
  private leaveGuardToken: number = -1
  // Issue #829：状态栏时钟 interval 句柄，-1 表示未启动。
  private clockTimer: number = -1

  async aboutToAppear() {
    hilog.info(0x0000, 'WritingScreen', 'route name=Writing')
    // Issue #829：底部状态栏时钟。秒级跳动对手稿里的 HH:mm 没有意义，
    // 30s 对齐一次即可；aboutToDisappear 里成对 clearInterval。
    this.refreshClockText()
    this.clockTimer = setInterval(() => {
      this.refreshClockText()
      // Issue #829 评论2第2项：「字/分」是分钟级速度，必须跟着时间走，
      // 否则用户停笔后左段还挂着停笔前的速度。挂在同一个 interval 上，
      // 顺带让「今日进度」和时钟保持同一次刷新，不做两套节奏。
      this.refreshWritingStatusData().catch(() => {})
    }, STATUS_CLOCK_INTERVAL_MS)
    // Issue #768 评论5835171266 第3项：检测 API Level，决定是否使用 HdsNavDestination。
    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    // Issue #784 评论5857397503 第4点 + Issue #825 复核第2点：ScreenPolicy 的 HeaderTrailing
    // 动作统一在 WritingScreen 解析一次，与 API 等级无关。API23+ 的壳层只透传，
    // 宽屏右侧工具区也读同一份数组（否则宽屏关掉页头胶囊后工具区会一个动作都没有）。
    this.resolveHeaderActions()
    const paramsArr = this.navPathStack.getParamByName(Routes.Writing) as WritingRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.chapterId = params.chapterId
      this.chapterTitle = params.chapterTitle
      this.projectId = params.projectId || ''
      this.volumeId = params.volumeId || ''
      // 暂存接续参数，loadChapter 在 coordinator.open 成功后消费以恢复编辑器状态。
      this.continuationParams = params
      this.loadWritingContent()
      // Issue #629 评论14 第1项：宽屏双栏模式下加载项目树（供 ChapterTreePane）。
      this.ensureWorkbenchProjectTree()
      // Issue #825 复核第 2 项：进页面先要一份 Core 七角色 bounds 计划。
      this.refreshWorkbenchPlan()
      // Issue #829 评论2第2项：状态栏两段进度先拉一次 Core 真值。
      this.refreshWritingStatusData()
    }

    const self = this
    // 注册编辑会话状态监听器：open/编辑命令成功后回流 editorSnapshot/content/hasUnsaved/wordCount。
    // 选区/正文变化后持续写入当前接续状态，保证 onContinue 读取到最新编辑器状态。
    // Issue #629 评论9 第4项：listener 改 typed EditorStateUpdate{snapshot, editResult}。
    // wordCount 实时来自 editResult.contentDelta（insertedNonWhitespaceChars/deletedNonWhitespaceChars，
    // Core 字数语义），不用 snap.text.length（JS UTF-16 code unit 数）。
    // editResult=null（open 初始加载 / stale-invalid 后的 snapshot 恢复）时不更新 wordCount（保持上次值）；
    // Core 已 Applied 只是平台 patch 失败时 recovery 后 editResult=本次 result.data，仍按 contentDelta 更新。
    this.coordinator.setStateListener((update: EditorStateUpdate) => {
      const snap = update.snapshot
      self.editorSnapshot = EditorLayoutSnapshot.fromEditorSnapshot(
        snap,
        self.settings.editorFontSize,
        self.settings.editorLineSpacingMultiplier,
        self.settings.autoIndentEnabled,
        self.settings.autoIndentWidth
      )
      // Issue #879：保存 editResult 供运动层消费。
      self.editorEditResult = update.editResult
      self.content = snap.text
      self.hasUnsavedChanges = snap.text !== self.lastSavedContent
      // 实时字数：消费 editResult.contentDelta（Core 非空白字符增量）
      if (update.editResult !== null) {
        const delta = update.editResult.contentDelta
        self.wordCount = Math.max(0, self.wordCount + delta.insertedNonWhitespaceChars - delta.deletedNonWhitespaceChars)
        // Issue #829 评论 #5995146525：正文每次真实变化就在这里产出统计事件，
        // 不再等保存后比较整章。enqueueChange 只把事件放进内存队列并立刻返回，
        // 不跨 NAPI、不 await，所以不会阻塞编辑主链。
        // 平台端只透传 cause 和 contentDelta，source 分类（HumanTyped / Pasted /
        // Deleted / Unknown）由 Core 的 record_editor_change_stats 决定。
        // 统计用 insertedChars/deletedChars（不含 nonWhitespace 对）：
        // Core 写作统计的字数口径是非空白字符，但事件计数与 per_* 维度用的是原始 delta。
        self.editorStatsQueue.enqueueChange(
          self.projectId, self.volumeId, self.chapterId,
          update.editResult.cause, delta.insertedChars, delta.deletedChars
        )
      }
      self.updateContinuationState()
      // 内容变化时更新碰一碰/隔空传送 payload（Issue #629 评论 5 第 7 节）。
      self.updateSharePayload()
    })
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
      // Issue #825 复核第1项：窗口从窄变宽时第一次进入 Workbench 也要补一次目录树，
      // 否则 aboutToAppear 当时是窄屏、目录树永远为空。
      self.ensureWorkbenchProjectTree()
      // Issue #825 复核第 2 项：窗口尺寸变了就重新向 Core 要七角色 bounds。
      if (self.isWideWorkbench) {
        self.refreshWorkbenchPlan()
      } else {
        // 复核第 4 项：离开宽屏时递增 generation 并清 plan，让在途的旧请求
        // 回来之后也匹配不上，不能复活已经作废的工作台布局。
        self.workbenchPlanGeneration++
        self.workbenchPlan = null
      }
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)

    // Issue #629 评论6 Part G：退后台用 performBackgroundSave（flush+idle+save），
    // 不只 saveChapter，避免最后几个字还在 dispatcher 队列里就 save 丢输入。
    // Issue #825 复核10：observer 只创建一次，之后靠 reconfigure() 改开关/延迟。
    // 早先每次设置变化都 remove + new，而 removeObserver 不会停旧定时器，
    // 旧 interval 会按旧延迟继续跑，新 observer 又不会补发 onForeground。
    this.autoSaveObserver = new AutoSaveLifecycleObserver(
      (): Promise<void> => this.performBackgroundSave(),
      this.settings.autoSaveDelayMs,
      this.settings.autoSaveEnabled
    )
    this.lifecycle.addObserver(this.autoSaveObserver)
    // 页面出现时通常已经在前台，addObserver 不会补发 onForeground，
    // 所以按当前真实前后台状态显式起一次（或确认该关着）。
    this.autoSaveObserver.reconfigure(
      this.settings.autoSaveEnabled,
      this.settings.autoSaveDelayMs,
      this.lifecycle.isForeground()
    )

    // Issue #825 复核8第 3 点：订阅 LocalSettings 落盘广播，
    // 让宽屏设置浮层里的改动能实时回流到本页（重建排版 + 重配自动保存）。
    this.unsubscribeLocalSettings = LocalSettingsStore.getInstance().subscribe(
      (latest: LocalSettings): void => { this.applyPublishedSettings(latest) }
    )

    // Issue #629 评论 5 第 7 节：碰一碰/隔空传送由页面拥有生命周期。
    // 进入前台（aboutToAppear）注册 startTapSharing/startAirTransferSharing；
    // onForeground 重新注册，onBackground stop（成对，不全局乱开）。
    this.startShareLifecycle()
    const shareObserver: ShareLifecycleObserver = new ShareLifecycleObserver(
      (): void => { this.startShareLifecycle() },
      (): void => { this.stopShareLifecycle() }
    )
    this.shareLifecycleObserver = shareObserver
    this.lifecycle.addObserver(this.shareLifecycleObserver!)

    // Issue #629 评论14 第2项：注册当前 WritingScreen 的离开守卫，获取唯一 token。
    // NavigationTransactionCoordinator.safePop/safeClearAndRebuild/safeReplacePath 会 await 此 guard（performGracefulClose），
    // 只有保存成功且 Core close 成功才允许改栈，未保存正文不被程序化 clear/pop 绕过。
    this.leaveGuardToken = NavigationTransactionCoordinator.getInstance().registerLeaveGuard((): Promise<boolean> => this.performGracefulClose())
  }

  // Issue #629 评论6 Part G + 评论7 第5项 + 评论8 第2/3项：统一幂等关闭事务。
  // seal（拒新输入）→ finishActiveComposition（最后中文 preedit 提交）→ drain dispatcher
  // → coordinator idle → 保存稳定 snapshot → 保存成功后 detach IME → close Core session。
  // 返回 boolean：保存失败/close 失败都返回 false（不 detach/close/pop，未落盘正文留在活跃会话）。
  // 幂等：closeTask 复用同一 Promise；失败后重置允许重试。
  private async performGracefulClose(): Promise<boolean> {
    // Issue #629 评论7 第5项：已有 closeTask 时直接返回同一 Promise（幂等）。
    if (this.closeTask !== null) {
      return this.closeTask
    }
    const task: Promise<boolean> = this.doGracefulClose()
    this.closeTask = task
    task.then(
      (closed: boolean) => {
        // 成功关闭：页面即将销毁，保留缓存（不重跑）。失败：重置，允许下次返回重试。
        if (!closed && this.closeTask === task) {
          this.closeTask = null
        }
      },
      () => {
        // 异常路径也重置（失败后页面仍在，允许重试）。
        if (this.closeTask === task) {
          this.closeTask = null
        }
      }
    )
    return task
  }

  // Issue #629 评论7 第5项：真实关闭逻辑（拆出，由 performGracefulClose 幂等包装）。
  // Issue #629 评论8 第3项 + 评论9 第2项：顺序改为
  //   seal → finishActiveComposition → drain → whenIdle → save → closeAsync → detach。
  // 关键：先 closeAsync（Core 关闭会话），成功后才 detach IME（不可逆清理）。
  // 失败路径（保存失败 / closeAsync 失败 / 异常）统一 unseal 后返回 false，且失败前不 detach IME
  // ——这样页面可立即继续编辑，不需要“重新 attach IME”补救。
  // 返回 boolean：保存失败或 closeAsync 失败返回 false，不 pop；正文仍在活跃编辑会话。
  // 等写作统计队列写完 barrier 及之前入队的那些事件（Issue #829 评论 #5996577737 第 2 项）。
  // 传 barrier 而不是「等到队列全空」：保存期间用户还能继续输入，新事件不断入队，
  // 全空在持续输入时理论上永远等不到，activeSave 会一直挂着把后面的保存全堵住。
  private async flushEditorStatsQueue(barrier: number): Promise<boolean> {
    return this.editorStatsQueue.flushThrough(barrier)
  }

  private async doGracefulClose(): Promise<boolean> {
    // 1. dispatcher 进入 sealed：拒绝新的普通输入命令（已排队命令仍可 drain）
    this.dispatcher.seal()
    try {
      // 2. 同一 dispatcher 队列里 finish 当前 composition：最后的中文 preedit 提交进
      //    committed 正文（否则 snapshot.text 不含 preedit，直接保存会漏掉屏幕上看到的预输入）。
      // Issue #629 评论 5308748920 问题1：必须看 finishResult.success。
      // FFI 的 ResultEnvelope.success=true 只代表调用成功返回了 DTO，不等于 composition 已结束。
      const finishResult = await this.dispatcher.finishActiveComposition()
      // 3. drain dispatcher（所有已排队语义输入应用完）
      await this.dispatcher.flush()
      // 4. 等 coordinator 命令队列空闲（所有 Core 编辑命令完成，snapshot 已包含最后输入）
      await this.coordinator.whenIdle()
      // 5. Issue #629 评论 5308748920 问题1：composition 唯一真源是 Core snapshot。
      //    重读 afterFinish snapshot，只要 finishResult.success===false、afterFinish===null、
      //    或 afterFinish.composition 仍然存在，立即 unseal 并 return false，不进入保存。
      //    发生 stale/恢复时 Core 可能仍保留 composition，close 必须以最终 Core snapshot 为准。
      // EditorSessionSnapshot.composition 是可选键（Core 侧 skip_serializing_if，不活跃时整个
      // 键省略），所以"没有 composition"落在 undefined 上。只判 null 时 undefined !== null 恒成立，
      // doGracefulClose 永远 return false → requestLeave 全部被拒 → 写作页再也退不出去。
      const afterFinish = this.coordinator.getSnapshot()
      if (finishResult.success === false || afterFinish === null ||
        (afterFinish.composition !== null && afterFinish.composition !== undefined)) {
        this.dispatcher.unseal()
        return false
      }
      // 6. Issue #829 评论 #5995146525 / #5996028848 / #5996577737：等写作统计真正写完再离开。
      //    dispatcher 已 seal、coordinator 已 whenIdle，所以此刻之后不会再有编辑事务入队，
      //    取当前最后一个 seq 当 barrier 即可覆盖「本次会话的全部编辑」。
      //    这里等的是真正的写完（drainPromise + 完成序号），不是轮询 pendingCount。
      //    统计没写进去不该拦住用户离开，所以失败只记日志不 return false。
      const statsBarrier: number = this.editorStatsQueue.currentSeq()
      const statsFlushed: boolean = await this.flushEditorStatsQueue(statsBarrier)
      if (!statsFlushed) {
        console.warn(`[WritingScreen] 写作统计未全部落盘（丢弃 ${this.editorStatsQueue.dropped()} 条）`)
      }
      // 7. 保存稳定 snapshot（此时不再读可能已变化的 this.content）
      const savedText: string = afterFinish.text
      if (savedText !== this.lastSavedContent) {
        // Issue #629 评论 5308748920 问题3：用 ensureSnapshotSaved 精确保存这份 savedText，
        // 不靠 retry 次数，不靠 lastSavedContent 校验循环。失败立即 unseal + return false。
        const settled: boolean = await this.ensureSnapshotSaved(savedText, false)
        if (!settled) {
          // 保存失败：不 detach、不 close、不 pop；解锁输入让用户继续编辑（下次返回重试）。
          this.dispatcher.unseal()
          return false
        }
      }
      // Issue #829 评论 #5995146525：这里不再有「没有未保存变更就兜底上报统计」的分支。
      // 写作统计已改由编辑事务同源上报，不需要在退出时按整章文本差异补一笔——
      // 那种补报既会和事务级上报重复计数，也会把连续输入误判成粘贴。
      // 7. 关闭编辑会话。Issue #629 评论9 第2项：先 closeAsync，成功后才 detach IME。
      // Issue #629 评论11 第3项：close 必须 success===true && data===true 才算成功。
      // 旧实现只判断 closeResult.success（FFI 返回 { success:true, data:false } 时会误当成功）。
      const closeResult = await this.coordinator.closeAsync()
      if (!closeResult.success || closeResult.data !== true) {
        // close 失败：不 detach、不 pop；unseal 让用户继续编辑（不需要重新 attach IME）。
        this.dispatcher.unseal()
        return false
      }
      // 8. Core close 成功后才 detach IME（不可逆清理放到最后，失败路径不触不可逆动作）
      await this.harmonyImeConnection.detach()
      return true
    } catch (err) {
      // 异常路径按失败处理：不 pop（正文仍在活跃会话），解锁输入允许重试，不 detach IME。
      this.dispatcher.unseal()
      return false
    }
  }

  // Issue #629 评论6 Part G：退后台保存事务。
  // flush input → await coordinator idle → save latest snapshot
  // 不 detach IME、不 close session（session 在 aboutToDisappear 时才 close）。
  // 退后台只保证：所有排队输入应用完 + 保存最新正文，回前台可继续编辑。
  private async performBackgroundSave(): Promise<void> {
    if (this.dispatcher) {
      await this.dispatcher.flush()
    }
    await this.coordinator.whenIdle()
    if (this.hasUnsavedChanges) {
      await this.saveChapter(true)
    }
  }

  // Issue #629 评论9 第3项：统一的离开请求入口。
  // NavDestination.onBackPressed 和左上角返回按钮都调此方法：执行 performGracefulClose，
  // 只有关闭事务成功（保存成功且 Core close 成功）才 navPathStack.pop()；失败留在本页不 pop。
  // 不在 onBackPressed 里直接 await（onBackPressed 同步返回 boolean 拦截默认退栈），
  // 异步关闭链在 requestLeave 里跑，onBackPressed 立即 return true 拦截默认回退。
  // Issue #629 评论 5308748920 问题2：离开事务（close + pop）整体幂等。
  // 已有 leaveTask 时直接返回同一 task，task 内只 pop 一次。
  // close 失败时清 leaveTask 允许再次返回重试；成功后不重置（页面已离开）。
  // 左上角按钮和 onBackPressed 都只调用此入口，禁止多套关闭/离开流程。
  // Issue #629 评论11 第3项：requestLeave 改用 NavigationTransactionCoordinator.safePop()。
  // safePop 内部 runGuardedNavigation 先 await leaveGuard（performGracefulClose），
  // 成功才 navPathStack.pop()，且两个并发 safePop 合并成一次事务（guard 一次、pop 一次）。
  // leaveTask 仍保留作为 requestLeave 自身重入防护（safePop 内部 navigationTask 也幂等，双保险）。
  // Issue #783 评论5855754605：requestLeave 改为返回 Promise<boolean>；
  // safePopWithStack 返回 false 时（关闭事务被拒绝），给用户提示"保存或关闭失败，未退出"。
  private requestLeave(): Promise<boolean> {
    if (this.leaveTask !== null) {
      return this.leaveTask
    }

    const task: Promise<boolean> = (async () => {
      const ok = await NavigationTransactionCoordinator
        .getInstance()
        .safePopWithStack(this.navPathStack)

      if (!ok) {
        hilog.error(0x0000, 'WritingScreen', 'requestLeave: graceful close rejected')
        this.saveMessage = '保存或关闭失败，未退出'
      }
      return ok
    })()

    this.leaveTask = task
    task.finally(() => {
      if (this.leaveTask === task) {
        this.leaveTask = null
      }
    })
    return task
  }

  // Issue #783 评论5855754605：把"标题栏返回"和"系统返回手势"分成两个意图。
  // 标题栏返回按钮始终走 requestLeave（退出页面）。
  // 系统返回手势在 IME 打开时先收键盘（dismissForBack），再次手势才退出页面。
  private handleTitleBack(): void {
    this.requestLeave()
  }

  private handleSystemBack(): void {
    if (this.harmonyImeConnection.canDismissForBack()) {
      this.harmonyImeConnection.dismissForBack()
      return
    }
    this.requestLeave()
  }

  // Issue #629 评论9 第3项：aboutToDisappear 不再承担“阻止丢正文”职责——返回拦截由
  // NavDestination.onBackPressed + requestLeave() 统一处理。aboutToDisappear 只做页面已确定
  // 离开后的 observer/share/view 清理（removeObserver、stopShareLifecycle、setStateListener(null)）。
  // 不能把它当返回拦截器：此时路由已退栈，无法阻止返回。
  async aboutToDisappear() {
    // Issue #629 评论14 第2项：用唯一 token 注销自己的离开守卫，
    // 不影响其他 WritingScreen 实例（如宽屏切章节时新旧实例生命周期重叠）。
    if (this.leaveGuardToken >= 0) {
      NavigationTransactionCoordinator.getInstance().unregisterLeaveGuard(this.leaveGuardToken)
      this.leaveGuardToken = -1
    }
    // 清除监听器（避免 close 后回调）
    this.coordinator.setStateListener(null)

    // Issue #629 评论 5 第 7 节：页面离开时 stop 碰一碰/隔空传送（成对，不留监听）。
    this.stopShareLifecycle()

    if (this.autoSaveObserver) {
      // Issue #825 复核10：removeObserver 只从 Set 里删引用，不会停定时器，
      // 所以移除前先显式 stopAutoSave()，保证页面离开后不留 interval。
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

  // Issue #829：HH:mm。两位补零，手稿里就是「14:26」这种宽度。
  // 只做展示，不参与保存 / 统计 / 同步任何判定。
  private refreshClockText(): void {
    const now = new Date()
    const hour = String(now.getHours()).padStart(2, '0')
    const minute = String(now.getMinutes()).padStart(2, '0')
    this.clockText = `${hour}:${minute}`
  }

  // 两段状态都失败时保持旧值：状态栏是只读展示，查询失败不该把已有数字清零。
  private async refreshWritingStatusData(): Promise<void> {
    // 左段：Core「当前写作速度」，窗口 60 秒 = 手稿「86 字/分」的「最近一分钟」。
    // 不用写作速度曲线的最后一个桶——曲线桶只生成到最后一个输入事件，
    // 停笔后会一直挂着停笔前的非零值。也不在这里判断桶是否过期或强制 flush
    // 统计事件，Core 已把内存缓冲和已落盘事件当成同一份事实源。
    const speed = await this.bridge.getCurrentWritingSpeed(WRITING_SPEED_WINDOW_SECONDS)
    if (speed.success && speed.data) {
      this.latestCharsPerMinute = Math.round(speed.data.charsPerMinute)
    }

    // 中段：今日纯输入字数。
    // 这里不自己拼"今天是哪一天"（原 todayDateString 已删）：Core 的每日统计按
    // 事件发生地的本地午夜分桶，端侧再拼一份本地日期，一旦时区口径和 Core 错开，
    // 凌晨就会出现"今日进度提前清零"。日历日语义只留在 Core 一处，Linux_Qt 走
    // 同一个 getTodayWritingStatsSummary，不给第三个端留复制的机会。
    const summary = await this.bridge.getTodayWritingStatsSummary()
    if (summary.success && summary.data) {
      this.todayTypedChars = summary.data.totalHumanTypedChars
    }
  }

  // Issue #768 评论5838296806：先加载排版设置，再打开章节会话。
  // 避免并发加载时 coordinator.open() 先完成导致 editorSnapshot 用默认设置生成。
  private async loadWritingContent(): Promise<void> {
    await this.loadSettings()
    await this.loadChapter()
  }

  async loadChapter() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.loadChapter(this.chapterId)
      if (result.success && result.data) {
        this.content = result.data.content
        this.lastSavedContent = result.data.content
        // Issue #629 评论 5 第 8 节：跨设备内容身份来自 ChapterContent.meta.hash。
        this.lastSavedContentHash = result.data.meta.hash
        this.wordCount = result.data.meta.wordCount
        this.hasUnsavedChanges = false
        // Issue #829 评论 #5995146525：不再记录 sessionOldText。打开章节时正文还没被编辑过，
        // 没有任何编辑事务可上报，统计基线由队列从第一条真实编辑事件开始算。
        // 打开编辑会话，用 Core 管理正文。open 成功后 listener 自动回流 editorSnapshot/content/hasUnsaved/wordCount。
        // Issue #653 评论 5608056955：Harmony 没有 persistent/temporary 两套宿主生命周期，
        // close 在 requestLeave 统一做，open 不再接收 isPersistent 参数。
        const snapResult = await this.coordinator.open(this.chapterId, result.data.content)
        if (!snapResult.success) {
          this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_chapter').id)
        } else if (snapResult.data) {
          // ── 恢复接续编辑器状态（selection/cursor/viewport/contentHash）──
          await this.restoreContinuationEditorState(snapResult.data)
        }
      } else {
        this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_chapter').id)
      }
    } catch (err) {
      this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_chapter_detail').id)
    } finally {
      this.isLoading = false
    }
  }

  // 恢复接续编辑器状态。在 coordinator.open 成功后调用。
  // offset 转换方向：payload(UTF-16) → Core(UTF-8)，用 TextOffsetMapper.utf16ToUtf8。
  // contentHash 不匹配时不强套旧 offset，只恢复章节导航，以当前 Core 正文为准。
  // Issue #629 评论 5 第 8 节：跨设备内容身份用 contentHash（稳定内容哈希），
  // 不用 TextEditSession.revision（每次新建会话都是 initial()，无跨设备比较意义）。
  private async restoreContinuationEditorState(snap: EditorSessionSnapshot): Promise<void> {
    const params = this.continuationParams
    if (params === null) {
      // 非接续进入；写入当前状态供后续 onContinue 读取。
      this.updateContinuationState()
      return
    }

    // contentHash 校验：continuation contentHash 与当前加载章节的 meta.hash 不匹配时，
    // 不强套旧 offset，只恢复章节导航，以当前 Core 正文为准。
    // contentHash undefined 时视为匹配（兼容旧 payload）。
    const contentHashMatches: boolean =
      params.contentHash === undefined || params.contentHash === this.lastSavedContentHash

    if (contentHashMatches) {
      // selection 优先；无 selection 时用 cursor。
      // selection/cursor 是 UTF-16 code unit offset（UI 坐标），转成 Core UTF-8 byte offset。
      if (params.selection !== undefined && params.selection.length >= 2) {
        const anchorByte: number = TextOffsetMapper.utf16ToUtf8(snap.text, params.selection[0])
        const headByte: number = TextOffsetMapper.utf16ToUtf8(snap.text, params.selection[1])
        await this.coordinator.setSelection(anchorByte, headByte)
      } else if (params.cursor !== undefined) {
        const cursorByte: number = TextOffsetMapper.utf16ToUtf8(snap.text, params.cursor)
        await this.coordinator.setSelection(cursorByte, cursorByte)
      }
    }

    // viewport 只恢复到 UI 滚动位置，不写 Core。
    // Issue #629 评论6 Part C：通过父子 controller 通道恢复，不直接赋值后跳过子组件滚动。
    // controller 会等 SujianEditor 注册 scrollTo（layout 就绪）后按目标设备当前尺寸 clamp + scrollTo。
    // 旧实现只 this.currentViewport = params.viewport，子组件不会真的滚动到接续位置。
    if (params.viewport !== undefined) {
      this.currentViewport = params.viewport
      this.viewportController.restore(params.viewport)
    }

    // 恢复后写入当前状态（UTF-8 → UTF-16 转换在 updateContinuationState 内完成）。
    this.updateContinuationState()
  }

  // 持续写入当前接续状态。每次章节打开、选区变化、正文编辑、viewport 变化后调用。
  // offset 转换方向：Core(UTF-8) → payload(UTF-16)，用 TextOffsetMapper.utf8ToUtf16。
  // Issue #629 评论 5 第 8 节：
  //   - 跨设备内容身份用 contentHash（来自 lastSavedContentHash），不用 revision。
  //   - 源设备有未保存修改时，不把基于未持久化正文的 selection/cursor 标成"可精确恢复"；
  //     只带导航信息 + contentHash（上次保存的），等保存后的 contentHash 更新再带完整
  //     selection/cursor。目标设备若磁盘内容已变（contentHash 不匹配）会安全恢复。
  private updateContinuationState(): void {
    if (!this.projectId || !this.chapterId) {
      return
    }
    const snap = this.coordinator.getSnapshot()
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
    // 有未保存修改时 selection/cursor 基于未持久化正文，不标"可精确恢复"。
    if (!this.hasUnsavedChanges) {
      // Core snapshot 的 cursor/selectionAnchor 是 UTF-8 byte offset；
      // ContinuationPayload 存 UTF-16 code unit offset（UI 坐标，跨设备平台无关）。
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
  // 由实际可分享页面拥有生命周期，不 AppContext 全局乱开。
  // 当前章节内容进入前台时注册（startTapSharing/startAirTransferSharing），
  // 内容变化时 update payload（updateTapPayload/updateAirTransferPayload），
  // 页面离开/退后台时 stop（stopTapSharing/stopAirTransferSharing）。
  // 普通"系统分享"按钮只调用 HarmonyShareService.share(payload, 'system')。
  // 碰一碰和隔空传送走其生命周期接口，不和普通分享混用。

  // 构造当前章节的 SharePayload。内容来自 coordinator snapshot（不另算一份）。
  private buildSharePayload(): SharePayload | null {
    if (!this.chapterId) {
      return null
    }
    const snap = this.coordinator.getSnapshot()
    const text: string = snap ? snap.text : this.content
    const payload: SharePayload = {
      kind: 'text',
      title: this.chapterTitle,
      text: text
    }
    return payload
  }

  // 内容变化时更新碰一碰/隔空传送 payload（state listener 调用）。
  private updateSharePayload(): void {
    const payload = this.buildSharePayload()
    this.shareService.updateTapPayload(payload)
    this.shareService.updateAirTransferPayload(payload)
  }

  // 进入前台/页面显示时注册碰一碰/隔空传送监听。
  // startTapSharing 返回真实注册结果；startAirTransferSharing 返回真实注册结果。
  // 注册失败不抛错（用户仍可正常编辑），下次 onForeground 重试。
  private startShareLifecycle(): void {
    const payload = this.buildSharePayload()
    if (payload === null) {
      return
    }
    // 碰一碰：先 updatePayload 再 start，返回真实注册结果。
    this.shareService.startTapSharing(payload)
    // 隔空传送：先 updatePayload 再 start(windowId)，返回真实注册结果。
    // windowId 由 HarmonyShareService 内部从 window.getLastWindow 统一获取。
    this.shareService.startAirTransferSharing(payload).then(() => {})
  }

  // 页面离开/退后台时 stop 碰一碰/隔空传送监听（成对，不留监听）。
  private stopShareLifecycle(): void {
    this.shareService.stopTapSharing()
    this.shareService.stopAirTransferSharing().then(() => {})
  }

  // Issue #767 评论 5828146609 第 2 节：shareToSystem() 已删除。
  // 普通"系统分享"按钮不再存在；碰一碰/隔空传送仍走 startShareLifecycle/stopShareLifecycle。

  // Issue #825 复核第1项：宽屏工作台只看 Core 下发的 WorkspaceLayoutMode.Workbench。
  // AdaptiveState.isTwoPane 只在 ShellMode.TwoPane 时为 true，而 1200vp 以上的真实大屏
  // Core 给的是 ShellMode.ThreePane，用 isTwoPane 当宽屏标志会让大屏退回单栏。
  private get isWideWorkbench(): boolean {
    return this.adaptiveState.layoutPlan.workspaceLayoutMode === WorkspaceLayoutMode.Workbench
  }

  // Issue #825 复核第2项：ScreenPolicy 的 HeaderTrailing 动作只在这里解析一次。
  private async resolveHeaderActions(): Promise<void> {
    const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(ScreenRole.Writing)
    if (policy !== null) {
      this.headerActions = policy.actionSlots
    }
  }

  // Issue #825 复核第 2 项：向 Core 要一份七角色 bounds 计划。
  // 目录栏 / 工具 pane 的展开状态只作为 WorkbenchVisibility 输入，不落 Core 持久化，
  // 也不影响编辑会话——所以每次重算只改布局，EditorSessionCoordinator 始终是同一个。
  private async refreshWorkbenchPlan(): Promise<void> {
    // Issue #825 复核8第 1 点：只有"窗口尺寸 / 普通折叠变化"这条状态路径
    // 才写正式 plan。展开请求走 resolveWorkbenchCandidate（纯候选），不碰这里。
    const plan = await this.resolveWorkbenchCandidate(!this.leftPaneCollapsed, !this.rightPaneCollapsed)
    if (plan !== null) {
      this.workbenchPlan = plan
    }
  }

  // 纯候选：只问一次 Core 并返回这一次的 plan，不写 this.workbenchPlan。
  //
  // Issue #825 复核7第 1 点：resolve 必须真正被 await。之前那个函数只是启动
  // resolveWorkbenchLayout(...).then(...) 就返回，于是调用方的 await 立刻结束，
  // 读到的还是上一份 plan——pane 请求在"旧 plan 是 Workbench"时直接 return，
  // 锁死路径原样复现。
  //
  // Issue #825 复核8第 1 点：它此前还会顺手把 plan 写进 this.workbenchPlan，
  // 于是"算候选一（SinglePane）→ 算候选二（Workbench）"的过程中页面会先真的
  // 退成单栏再跳回来；两个候选都失败时左右 @State 没改，但正式 plan 已经停在
  // 失败的 SinglePane，页面还是变了。候选就是候选，正式 plan 由 refreshWorkbenchPlan
  // 或展开请求最终确认时一次性写入。
  private async resolveWorkbenchCandidate(chapterVisible: boolean, toolVisible: boolean):
    Promise<WorkbenchLayoutPlan | null> {
    const visibility: WorkbenchVisibility = {
      chapterNavigationVisible: chapterVisible,
      toolPaneVisible: toolVisible
    }
    const widthVp: number = this.adaptiveState.widthVp
    const heightVp: number = this.adaptiveState.heightVp
    // Issue #825 复核第 4 项：resolve 是异步的，快速拖窗口 / 连点折叠会让多次请求
    // 在途，旧请求可能晚回来把最新 plan 覆盖成旧尺寸。这里用递增的 generation
    // 标记"只有最后一次请求的结果算数"，并把本次的尺寸与可见性一起快照进闭包，
    // 避免回调里再读可能已经变了的 adaptiveState。
    const generation = ++this.workbenchPlanGeneration
    let plan: WorkbenchLayoutPlan | null = null
    try {
      plan = await getWorkbenchLayoutBridge().resolveWorkbenchLayout(widthVp, heightVp, visibility)
    } catch (err) {
      return null
    }
    if (generation !== this.workbenchPlanGeneration) {
      // 已经有更新的请求在途（或已经离开宽屏），这次的旧结果作废。
      return null
    }
    return plan
  }

  // Issue #825 复核6第1点：展开侧 pane 的请求流程。
  // 窄窗口下 Core 按「当前可见性」重判最终 mode：600～695vp 里左章节栏开着时
  // 再展开右工具 pane，最小需求 200+240+200+56 = 696vp 放不下，Core 会退回
  // SinglePane。如果端侧还留着「右 pane 已展开」，下一轮重算又继续喂
  // toolPaneVisible=true，于是永远 SinglePane，而 rail / 折叠把手已被隐藏，
  // 用户没有入口再把它关回去——只能拉大窗口解锁。
  // 所以这里把展开当成一次「请求」：先按当前组合问一次 Core，放不下就换一种
  // 组合再问一次；第二次能 Workbench 就让另一侧让位，仍放不下就回滚本次展开。
  // 真正因为 free region 连 Editor + ToolRail 都放不下而返回 SinglePane 时，
  // 才会自然落到纯单栏（那条路径下两端侧都没有展开状态，不需要额外回滚）。
  //
  // Issue #825 复核7第 2 点：改成「先算候选，再一次性提交」。
  // 之前是「先改 UI 状态 → 看 Core 结果 → 再回滚」，失败分支只回滚了自己那侧，
  // 为了腾位置收掉的另一侧没有恢复——用户只想开右栏，失败后左栏反而被关掉了。
  // 现在一个 @State 都不改就先把候选组合问一遍，哪一组是 Workbench 就整体提交；
  // 两组都不行就什么都不改，不留"已展开但看不见"的端侧状态，也没有中间态闪烁。
  private async requestToolPaneOpen(toolKey: string, allowCollapseLeft: boolean): Promise<void> {
    // 目标状态初始就是"当前左栏状态"，不是 false：
    // 候选一是「保持当前左栏状态」，命中时要提交的必须是它本身，
    // 只有走候选二（左栏让位）才强制改成 true。
    // 用"有没有走第二候选"反推全部状态，会在用户本来就收着左栏时
    // 反而把左栏重新打开，提交出来的 UI 状态和刚拿到的 Core plan 也对不上。
    let targetLeftCollapsed: boolean = this.leftPaneCollapsed
    let plan: WorkbenchLayoutPlan | null =
      await this.resolveWorkbenchCandidate(!this.leftPaneCollapsed, true)
    if (!isWorkbenchMode(plan) && allowCollapseLeft) {
      plan = await this.resolveWorkbenchCandidate(false, true)
      targetLeftCollapsed = true
    }
    if (!isWorkbenchMode(plan)) {
      return
    }
    this.leftPaneCollapsed = targetLeftCollapsed
    this.rightPaneCollapsed = false
    this.selectedToolKey = toolKey
    if (plan !== null) {
      this.workbenchPlan = plan
    }
  }

  // 左目录栏的展开请求，逻辑与右侧对称：必要时让右 pane 让位，仍放不下就不改任何状态。
  private async requestChapterNavigationOpen(): Promise<void> {
    let targetRightCollapsed: boolean = this.rightPaneCollapsed
    let plan: WorkbenchLayoutPlan | null =
      await this.resolveWorkbenchCandidate(true, !this.rightPaneCollapsed)
    if (!isWorkbenchMode(plan)) {
      plan = await this.resolveWorkbenchCandidate(true, false)
      targetRightCollapsed = true
    }
    if (!isWorkbenchMode(plan)) {
      return
    }
    this.rightPaneCollapsed = targetRightCollapsed
    this.leftPaneCollapsed = false
    if (plan !== null) {
      this.workbenchPlan = plan
    }
  }

  // 关闭方向不会把窗口撑小，永远放得下：改完状态后重算一次正式 plan，
  // 让 Editor / ToolPane / handle 的 bounds 继续跟着 Core 走，
  // 而不是让 layoutWeight(1) 帮平台自己分剩下的宽度。
  private closeToolPane(): void {
    // 只收起 pane，不清空最后选中的工具；再次展开回到原来的星图 / AI。
    this.rightPaneCollapsed = true
    this.refreshWorkbenchPlan().catch(() => {})
  }

  private closeChapterNavigation(): void {
    this.leftPaneCollapsed = true
    this.refreshWorkbenchPlan().catch(() => {})
  }

  private chapterNavigationWidthVp(): number {
    const bounds = workbenchBounds(this.workbenchPlan, WorkbenchRole.ChapterNavigation)
    if (bounds === null) {
      return FALLBACK_CHAPTER_NAV_WIDTH_VP
    }
    return Math.max(0, bounds.rightDp - bounds.leftDp)
  }

  private toolPaneWidthVp(): number {
    const bounds = workbenchBounds(this.workbenchPlan, WorkbenchRole.ToolPane)
    if (bounds === null) {
      return FALLBACK_TOOL_PANE_WIDTH_VP
    }
    return Math.max(0, bounds.rightDp - bounds.leftDp)
  }

  private toolRailWidthVp(): number {
    const bounds = workbenchBounds(this.workbenchPlan, WorkbenchRole.ToolRail)
    if (bounds === null) {
      return FALLBACK_TOOL_RAIL_WIDTH_VP
    }
    return Math.max(0, bounds.rightDp - bounds.leftDp)
  }

  // 顶部工具条带三个角色的 bounds。Core 在 SinglePane 下不给工具条带，
  // 这里返回 -1，由 WritingWorkbenchToolbar 按 Core metrics 的默认段宽兜底。
  private toolbarBoundsVp(role: WorkbenchRole): number {
    const bounds = workbenchBounds(this.workbenchPlan, role)
    if (bounds === null) {
      return -1
    }
    return Math.max(0, bounds.rightDp - bounds.leftDp)
  }

  private toolbarHeightVp(): number {
    const bounds = workbenchBounds(this.workbenchPlan, WorkbenchRole.ToolbarCenter)
    if (bounds === null) {
      return FALLBACK_TOOLBAR_HEIGHT_VP
    }
    return Math.max(0, bounds.bottomDp - bounds.topDp)
  }

  // Core 决定的最终产品模式；SinglePane 时页面回单栏，不摆七角色。
  private get hasWorkbenchPlan(): boolean {
    return isWorkbenchMode(this.workbenchPlan)
  }

  // Issue #825 复核第1项：进入宽屏工作台后只加载一次目录树。
  // aboutToAppear（当时可能还是窄屏）与 adaptive observer（窗口变宽）都走这里。
  private ensureWorkbenchProjectTree(): void {
    if (!this.isWideWorkbench || this.wideWorkbenchTreeLoaded) {
      return
    }
    this.wideWorkbenchTreeLoaded = true
    this.loadProjectTree()
  }

  // Issue #629 评论14 第1项：宽屏双栏模式加载项目树。
  private async loadProjectTree(): Promise<void> {
    if (!this.projectId || !this.isWideWorkbench) {
      return
    }
    this.isTreeLoading = true
    try {
      const result = await this.bridge.getProjectTree(this.projectId)
      if (result.success && result.data) {
        this.volumeTrees = result.data.volumes
        // Issue #829 手稿：章节树最上面一组是作品名，不是笼统的"章节"。
        this.projectTitle = result.data.project.title
        // 展开包含当前章节的卷
        for (const vt of this.volumeTrees) {
          for (const ch of vt.chapters) {
            if (ch.id === this.chapterId) {
              this.expandedVolumes.add(vt.volume.id)
              break
            }
          }
        }
      }
    } catch (err) {
      console.error('Failed to load project tree:', err)
    } finally {
      this.isTreeLoading = false
    }
  }

  // Issue #629 评论14 第1项：宽屏左侧切章节。
  // 先完成当前 Writing 的 performGracefulClose()，成功后把当前 Writing route
  // 替换成目标章节的 Writing route；失败保持原章节。
  private async switchChapter(chapterId: string, chapterTitle: string, volumeId: string): Promise<void> {
    if (chapterId === this.chapterId) {
      return // 已是当前章节
    }
    const host = NavigationTransactionCoordinator.getInstance()
    const targetKey = `writing:${this.projectId}:${volumeId}:${chapterId}`
    await host.safeReplacePathWithStack(this.navPathStack, targetKey, () => {
      const replaceParam: WritingRouteParams = {
        chapterId: chapterId,
        chapterTitle: chapterTitle,
        projectId: this.projectId,
        volumeId: volumeId
      }
      this.navPathStack.replacePath({
        name: Routes.Writing,
        param: replaceParam
      })
    })
  }

  // Issue #629 评论14 第1项：展开/折叠卷。
  private toggleVolume(volumeId: string): void {
    if (this.expandedVolumes.has(volumeId)) {
      this.expandedVolumes.delete(volumeId)
    } else {
      this.expandedVolumes.add(volumeId)
    }
  }

  // Issue #629 评论14 第1项：创建卷（宽屏左侧目录面板回调）。
  private async createVolume(): Promise<void> {
    if (!this.projectId) {
      return
    }
    try {
      const defaultName = getContext(this).resourceManager.getStringSync($r('app.string.default_new_volume_title').id)
      const result = await this.bridge.createVolume(this.projectId, defaultName)
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to create volume:', err)
    }
  }

  // Issue #629 评论14 第1项：创建章节（宽屏左侧目录面板回调）。
  private async createChapter(volumeId: string): Promise<void> {
    try {
      const defaultName = getContext(this).resourceManager.getStringSync($r('app.string.default_new_chapter_title').id)
      const result = await this.bridge.createChapter(volumeId, defaultName)
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to create chapter:', err)
    }
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
    try {
      const result = await this.bridge.deleteVolume(this.projectId, volumeId)
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to delete volume:', err)
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
    try {
      const result = await this.bridge.deleteChapter(chapterId)
      if (result.success) {
        this.loadProjectTree()
      }
    } catch (err) {
      console.error('Failed to delete chapter:', err)
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
  //
  // Issue #825 复核8第 3 点：进页面（loadSettings）和设置浮层保存成功后的
  // LocalSettingsStore 广播共用这一份实现，两边不再各写一套，
  // 否则浮层改了字号、底下写作页还按旧排版渲染。
  private applyPublishedSettings(settings: LocalSettings): void {
    this.settings = settings
    // Issue #879：设置变化时同步更新运动策略。
    this.motionPolicy = EditorMotionPolicy.fromSettings(settings)
    // Issue #825 复核8第 3 点：外部设置更新时，顶栏草稿回到真实值，
    // 步进器下一次点击从真实值继续算。
    //
    // 但如果队列里还有用户点出来的目标（保存途中又连点了步进器），
    // 这里不能把草稿拽回刚落盘的旧值——否则顶栏会闪回旧字号，
    // 下一次点击又会从旧值继续算，把用户点的目标改掉。
    // pending 目标由 applyEditorFormat 的队列自己收尾，这里不动。
    if (!this.hasPendingEditorFormat) {
      this.editorFormatDraftFontSize = settings.editorFontSize
      this.editorFormatDraftLineSpacing = settings.editorLineSpacingMultiplier
    }
    // Issue #768 评论5838296806：设置真相变化后，如果 coordinator 已有 snapshot，
    // 同步重建 editorSnapshot，保证 StyledString 与设置真相一致。
    const existingSnap = this.coordinator.getSnapshot()
    if (existingSnap !== null) {
      this.editorSnapshot = EditorLayoutSnapshot.fromEditorSnapshot(
        existingSnap,
        this.settings.editorFontSize,
        this.settings.editorLineSpacingMultiplier,
        this.settings.autoIndentEnabled,
        this.settings.autoIndentWidth
      )
    }
    // Issue #705：Core 的 themeMode 已废弃，运行时恒为 null；
    // 主题真相是 appearanceMode（与 SettingsScreen 同一读法）。
    const appearanceMode = this.settings.appearanceMode || this.settings.themeMode || 'system'
    this.themeAdapter.updateFromSettings({
      theme: appearanceMode as ThemeMode,
      fontSize: this.settings.editorFontSize,
      fontFamily: 'HarmonyOS Sans',
      lineHeight: this.settings.editorLineSpacingMultiplier
    } as ThemeUpdateParams)

    // Issue #825 复核10：设置真相变了就地重配同一个 observer，
    // 不再 remove + new（remove 不会停旧定时器，会叠 interval）。
    this.autoSaveObserver?.reconfigure(
      this.settings.autoSaveEnabled,
      this.settings.autoSaveDelayMs,
      this.lifecycle.isForeground()
    )
  }

  // Issue #629 评论 5308748920 问题3：手动/自动保存入口。
  // 先取得稳定 snapshot（flush + whenIdle），再调 ensureSnapshotSaved 精确保存这份文本。
  // 不直接 doSaveChapter（ensureSnapshotSaved 负责复用/启动保存事务）。
  async saveChapter(isAutoSave: boolean = false): Promise<boolean> {
    if (this.dispatcher) {
      await this.dispatcher.flush()
    }
    await this.coordinator.whenIdle()
    const snap = this.coordinator.getSnapshot()
    const targetText: string = snap ? snap.text : this.content
    return this.ensureSnapshotSaved(targetText, isAutoSave)
  }

  // Issue #629 评论 5308748920 问题3：统一保存接口，保证指定 targetText 落盘。
  // 若 activeSave 正好保存 targetText 且成功，直接 true；
  // 若目标不同，先 await 旧任务，再启动一次精确保存 targetText 的新事务。
  // 不设固定重试次数，不靠 retry 次数保证落盘。
  private async ensureSnapshotSaved(targetText: string, isAutoSave: boolean): Promise<boolean> {
    // 若有进行中保存任务，先等它结束
    if (this.activeSave !== null) {
      const prev = this.activeSave
      let prevOk: boolean = false
      try {
        prevOk = await prev.task
      } catch (err) {
        // 旧任务异常，继续启动新事务
      }
      // 旧任务保存的正是 targetText 且成功 → 直接复用结果
      if (prev.text === targetText && prevOk) {
        return true
      }
      // 目标不同或旧任务失败：清 activeSave，启动新事务
      if (this.activeSave === prev) {
        this.activeSave = null
      }
    }
    // 启动一次精确保存 targetText 的新事务
    const task: Promise<boolean> = this.doSaveChapter(targetText, isAutoSave)
    const saveEntry: ActiveSaveTask = { text: targetText, task: task }
    this.activeSave = saveEntry
    task.then(
      () => {
        if (this.activeSave !== null && this.activeSave.task === task) {
          this.activeSave = null
        }
      },
      () => {
        if (this.activeSave !== null && this.activeSave.task === task) {
          this.activeSave = null
        }
      }
    )
    return task
  }

  // Issue #629 评论 5308748920 问题3：真实持久化函数，只接受显式 targetText。
  // 不在里面重新抓"当前正文"（避免保存期间新输入被误存）。
  // 调用方（saveChapter/ensureSnapshotSaved）负责取得稳定 snapshot 后再调用。
  // 结算：lastSavedContent 按 targetText；
  // 再取 latestSnapshot：this.content = latestSnapshot.text；
  // hasUnsavedChanges = latestSnapshot.text !== targetText（保存期间新输入保持 unsaved）。
  // contentHash 是这次 targetText 的回执。wordCount 是当前编辑器正文的实时字数（listener 实时更新），
  // 只有保存期间无新输入时才用 receipt.wordCount 校正；有新输入时保留实时 wordCount。
  private async doSaveChapter(targetText: string, isAutoSave: boolean): Promise<boolean> {
    this.isSaving = true
    this.isAutoSaving = isAutoSave
    this.saveMessage = ''
    // Issue #829 评论 #5996577737 第 2 项：在这份正文定型之前记下统计 barrier。
    // targetText 是上面拿到的稳定快照，它包含的编辑事务都已经入队；
    // 保存期间的新输入 seq 更大，与这次保存无关，不该拖住这次保存。
    const statsBarrier: number = this.editorStatsQueue.currentSeq()

    try {
      // 只保存这份 targetText（不重新抓当前正文）
      const result = await this.bridge.saveChapter(this.chapterId, targetText)
      if (result.success && result.data) {
        // Issue #829 评论 #5995146525：这里不再上报写作统计。
        // 写作统计改由编辑事务同源上报（EditorStatsQueue，见 state listener），
        // 保存时再拿整章 old/new 文本比较会重复计数，而且 Core 那条路径有
        // 「净增 > 20 就当 paste」的启发式，正常连续敲 30 字会被误判成粘贴。
        this.lastSavedContent = targetText
        // Issue #629 评论 5 第 8 节：保存成功后跨设备内容身份来自 ChapterSaveReceipt.contentHash。
        this.lastSavedContentHash = result.data.contentHash
        this.hasUnsavedChanges = false
        this.lastSaveFailed = false
        // 重新取 latestSnapshot 结算：保存期间若又产生新输入，hasUnsavedChanges 保持 true，
        // 不把新版正文标成已保存。
        // Issue #629 评论9 第4项：wordCount 表示当前编辑器正文的实时字数（listener 按
        // editResult.contentDelta 实时更新）。只有保存期间无新输入（latest.text === targetText）
        // 时才用 receipt.wordCount 校正当前实时字数；有新输入时保留实时 wordCount，
        // 不拿旧 targetText 的 receipt.wordCount 覆盖当前 A+B 的实时字数。
        const latest = this.coordinator.getSnapshot()
        if (latest) {
          this.content = latest.text
          this.hasUnsavedChanges = latest.text !== targetText
          if (latest.text === targetText) {
            this.wordCount = result.data.wordCount
          }
        } else {
          this.wordCount = result.data.wordCount
        }
        this.saveMessage = isAutoSave ? getContext(this).resourceManager.getStringSync($r('app.string.auto_saved').id) : getContext(this).resourceManager.getStringSync($r('app.string.saved').id)
        setTimeout(() => { this.saveMessage = '' }, 2000)
        // Issue #829 评论 #5996028848：正文已经落盘，等这份正文对应的统计事件也写完，
        // 这样「磁盘上有正文」和「磁盘上有对应统计」是一致的 —— 崩溃时不会留下
        // 有正文却缺统计的窗口。
        // 评论 #5996577737 第 2 项：只等到 statsBarrier，不等之后的新输入。
        const statsFlushed: boolean = await this.flushEditorStatsQueue(statsBarrier)
        if (!statsFlushed) {
          console.warn(`[WritingScreen] 本次保存对应的写作统计未全部落盘（丢弃 ${this.editorStatsQueue.dropped()} 条）`)
        }
        // 保存成功后更新接续状态（contentHash 前进，此时 hasUnsavedChanges=false 带完整 selection/cursor）。
        this.updateContinuationState()
        return true
      } else {
        this.lastSaveFailed = true
        this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.save_failed').id)
        return false
      }
    } catch (err) {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_save').id)
      return false
    } finally {
      this.isSaving = false
      this.isAutoSaving = false
    }
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

  // Issue #784 评论5857856673 问题2：API<23 自定义标题 Row 右侧动作按钮的图标映射。
  // 与 HdsHeaderActionMenu 中 mapNavMenuItem 的图标保持一致。
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
    // 非预期 role 不应出现（sortedHeaderTrailingActions 只返回 HeaderTrailing slot），
    // 给一个 fallback 图标避免编译报错。
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

  // Issue #629 评论 5 第 4 节 + 评论9 第4项 + 评论11 第1项：
  // 清空正文必须走 Dispatcher 的 clearDocument 语义命令，不绕过队列直接 coordinator.replace。
  // 这样之前的输入一定先执行，之后的新输入只能排在 clear 后面，不再有跨队列抢跑。
  // replace 成功后 state listener 自动回流空正文（content=''、hasUnsavedChanges=true）并按
  // editResult.contentDelta 实时更新 wordCount——doClearContent 不再单独算字数，走同一 listener 路径。
  // 等 listener 回流后走同一套 ensureSnapshotSaved('') 持久化。
  private async doClearContent() {
    if (!this.dispatcher) {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.clear_failed').id)
      return
    }
    // dispatch clearDocument：命令进 Dispatcher 队列，保证之前的输入先执行。
    // 出队后 Dispatcher 内部读取最新 Core snapshot 计算全文范围，调 coordinator.replace 清空。
    this.isSaving = true
    const result = await this.dispatcher.dispatch({ kind: 'clearDocument' })
    this.isSaving = false
    if (!result.success) {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.clear_failed').id)
      return
    }
    // Issue #629 评论11 第1项：executeClearDocument 已自验空正文（cancel composition → replace
    // → 重读 snapshot 确认 text===''），这里不再二次 flush/whenIdle/afterClear 检查。
    // clear 之后用户立刻打的新字允许继续进入 Dispatcher；ensureSnapshotSaved('') 只保存空基线，
    // 保存完成后最新 snapshot 若已有新字，hasUnsavedChanges = latest.text !== '' 正好保持未保存。
    const saved: boolean = await this.ensureSnapshotSaved('', false)
    if (saved) {
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.cleared').id)
      setTimeout(() => { this.saveMessage = '' }, 2000)
    } else {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.clear_failed').id)
    }
  }

  // Issue #768 评论5835171266 第3项：NavDestination 内容部分抽取为 @Builder，
  // 供 HdsNavDestination 和 NavDestination 两个条件渲染分支共用。
  @Builder
  NavContent() {
    if (this.isWideWorkbench && this.hasWorkbenchPlan) {
      // Issue #825 复核第 1/2/3 项：宽屏按 Core 七角色摆，不再自己决定几栏。
      // 顶部一整条工具带（ToolbarLeading | ToolbarCenter | ToolbarTrailing），
      // 下面才是 ChapterNavigation | Editor | ToolPane | ToolRail。
      // 高度与各段宽度全部来自 resolve_workbench_layout 的 bounds；plan 说 SinglePane 时回单栏。
      // 屏幕宽度变化只改布局，不换 TextEditSession owner（同一个 coordinator/dispatcher/IME/save）。
      Column() {
        // 复核第 3 项：宽屏不再用 HDS 标题栏当工具条带的替身，按 Core 的
        // Toolbar* bounds 自己画一条；窄屏继续走壳层标题栏（见 build 的分支）。
        WritingWorkbenchToolbar({
          toolbarHeightVp: this.toolbarHeightVp(),
          leadingWidthVp: this.toolbarBoundsVp(WorkbenchRole.ToolbarLeading),
          trailingWidthVp: this.toolbarBoundsVp(WorkbenchRole.ToolbarTrailing),
          headerActions: this.headerActions,
          // Issue #825 复核8第 3 点：步进器显示并基于"编辑中草稿"，
          // 不是最后一次已落盘值——否则连点三次「+」都从同一个旧值出发，只 +1 步。
          fontSizePt: this.editorFormatDraftFontSize,
          lineSpacing: this.editorFormatDraftLineSpacing,
          onBack: () => { this.handleTitleBack() },
          onUndo: () => { this.applyUndo() },
          onRedo: () => { this.applyRedo() },
          onFontSizeChange: (value: number) => { this.changeEditorFontSize(value) },
          onLineSpacingChange: (value: number) => { this.changeEditorLineSpacing(value) }
        })
          .width('100%')

      Stack() {
        Row() {
          // ChapterNavigation：可收起目录栏。收起后整块不占宽度，只留贴边把手。
          if (!this.leftPaneCollapsed) {
            ChapterTreePane({
              volumeTrees: this.volumeTrees,
              projectTitle: this.projectTitle,
              expandedVolumes: this.expandedVolumes,
              selectedChapterId: this.chapterId,
              onToggleVolume: (volumeId: string): void => { this.toggleVolume(volumeId) },
              onSelectChapter: (chapterId: string, chapterTitle: string, volumeId: string): void => {
                this.switchChapter(chapterId, chapterTitle, volumeId)
              },
              onCreateVolume: (): void => { this.createVolume() },
              onCreateChapter: (volumeId: string): void => { this.createChapter(volumeId) },
              onDeleteVolume: (volumeId: string): void => { this.deleteVolume(volumeId) },
              onDeleteChapter: (chapterId: string): void => { this.deleteChapter(chapterId) }
            })
              .width(this.chapterNavigationWidthVp())
              .height('100%')
          }

          // Editor：占剩余空间，纸面限宽居中；左右 pane 展开/收起只改变可用空间，
          // 不改变编辑会话。
          Column() {
            // 正文区顶部的折叠钮：手稿里正文区自己带一个三角（展开态向下收起、
            // 收起态向上拉开左章节树）。用 Stack 浮在正文上方，不占正文高度，
            // 也不改 Core 给的 Editor bounds。贴边把手保留——它是收起后唯一的入口。
            Stack({ alignContent: Alignment.Top }) {
              // Issue #829 评论2第1项：这里只放正文主体，不再调完整 EditorPane()。
              // EditorPane() 自带窄屏旧底栏，宽屏再叠一条 WritingStatusBar 就是两条
              // 状态栏、且状态有两份来源。宽屏的唯一状态栏是下面那条 WritingStatusBar。
              this.EditorBody()

              PaneFoldButton({
                glyph: this.leftPaneCollapsed ? '⌃' : '⌄',
                onTriggered: (): void => {
                  if (this.leftPaneCollapsed) {
                    this.requestChapterNavigationOpen().catch(() => {})
                  } else {
                    this.closeChapterNavigation()
                  }
                }
              })
                .margin({ top: 4 })
            }
            .width('100%')
            .layoutWeight(1)

            // Issue #829 手稿：底部状态栏只属于 Editor 这一列，
            // 横贯「章节树右缘 ~ 工具面板左缘」，不放章节树 / 工具面板 / rail 上。
            // 硬约束 6：这里只放速度、进度、时间、保存状态，不做导航。
            WritingStatusBar({
              // 左段 = Core 速度曲线最后一个桶的 charsPerMinute（「86 字/分」）。
              charsPerMinute: this.latestCharsPerMinute,
              // 中段 = Core 今日纯输入 / 目标字数，分子分母都不是 UI 自算。
              progressCurrent: this.todayTypedChars,
              progressTarget: WRITING_PROGRESS_TARGET_PLACEHOLDER,
              clockText: this.clockText,
              saveStatus: this.getSaveStatusText(),
              saveStatusIsError: this.lastSaveFailed
            })
              .width('100%')
          }
          .layoutWeight(1)
          .height('100%')

          // ToolPane：可收起，承载 rail 选中的工具内容，不再重复顶部 ToolbarTrailing 动作。
          if (!this.rightPaneCollapsed) {
            this.RightToolPane()
          }

          // ToolRail：最右竖向 rail，宽度由 Core 的 ToolRail bounds 决定（恒 56dp）。
          WritingToolRail({
            railWidthVp: this.toolRailWidthVp(),
            selectedToolKey: this.selectedToolKey,
            toolPaneVisible: !this.rightPaneCollapsed,
            onSelectTool: (key: string): void => { this.onSelectTool(key) },
            onToggleToolPane: () => { this.onToggleToolPane() }
          })
            .height('100%')
        }
        .width('100%')
        .height('100%')

        // Issue #825 复核第5项：整屏覆盖层排把手，不占正文宽度。
        this.PaneHandleLayer()
      }
        .width('100%')
        .layoutWeight(1)
      }
      .width('100%')
      .height('100%')
    } else {
      // Issue #629 评论9 第3项：窄屏单栏布局（或 Core 判定 SinglePane）。
      this.EditorPane()
    }
  }

  // Issue #825 复核第 3 项：撤销/重做走唯一的 EditorSessionCoordinator，
  // 与输入法快捷键、dispatcher 是同一条编辑事务路径，栈空时 coordinator 自己 no-op。
  // Issue #825 复核6第2点：撤销 / 重做不再直接调 coordinator。
  // 它们作为语义命令进 EditorSemanticDispatcher 的串行队列，和物理键盘 /
  // IME / 鼠标 / 触摸输入同一条顺序边界，避免 undo 与 pending IME 命令并发，
  // 也不在 composition 还活着时先撤 committed text。
  private applyUndo(): void {
    this.dispatcher.dispatchFireAndForget({ kind: 'undo' })
  }

  private applyRedo(): void {
    this.dispatcher.dispatchFireAndForget({ kind: 'redo' })
  }

  // Issue #825 复核5第4点：ToolbarCenter 的字号 / 行距是 Core 设置里的真实字段，
  // 所以做成可操作控件——写回 saveLocalSettings 落盘，成功后用当前 coordinator
  // snapshot 重建 EditorLayoutSnapshot（和 loadSettings 走同一条重建路径）。
  // 存失败就保持当前排版，不做"看起来改了"的假更新。
  //
  // Issue #825 复核7第 3 点：两条并发/覆盖修复。
  // 1) this.settings 是进写作页时 loadSettings() 拉的那一份。宽屏设置是悬浮层，
  //    用户在里面改过主题 / 自动保存 / AI / 诊断后，底下的 WritingScreen 不会
  //    重新 load；此时直接 saveLocalSettings(this.settings) 会把用户刚改的
  //    其它字段一起覆盖回旧值。所以真正落盘前先 getLocalSettings() 拿最新真相，
  //    只覆盖本次要改的 editorFontSize / editorLineSpacingMultiplier 再存。
  // 2) 步进器快速连点会让多次 save 并发：先发的那次若晚失败，后发的那次已成功，
  //    先发分支的 catch 再把 this.settings 回滚到更旧的字号，UI 与磁盘分叉。
  //    这里用「单一 pending task」串行化：同一时刻只跑一个保存任务，
  //    连点只把 pending 目标合并成最新值，保存成功才回写 this.settings。
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
      // 已有一个任务在跑，它结束时会把合并后的最新目标一起处理掉。
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
  // 失败时不用"这次请求的 previous 值"回滚——那会把上一次已经成功的新值盖掉，
  // 改为按最新磁盘值决定要不要把顶栏草稿回正（见 realignDraftToPersisted）。
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
      // Issue #825 复核9 第3项：成功后再从 Core 重新读一次，
      // 只把这次重新 decode 出来的 persisted DTO 当成"最近一次落盘"，
      // 不把上面那个自己改过的对象当成真相——它还和 getLocalSettings()
      // 之前拿到的其他持有者共享同一份引用。
      const persisted = await this.bridge.getLocalSettings()
      if (persisted.success && persisted.data) {
        // Issue #825 复核10：本页已经订阅 LocalSettingsStore，publish 会同步回调
        // applyPublishedSettings 一次，所以这里只广播、不再直接调用，
        // 否则 snapshot/主题/自动保存会重建两遍。
        LocalSettingsStore.getInstance().publish(persisted.data)
      } else {
        // 重新读取失败：没有可广播的 persisted 快照，直接用刚保存的那份兜底，
        // 但不 publish（不把可能还被别人持有的对象当成「最近一次落盘」）。
        this.applyPublishedSettings(latest.data)
      }
      // 草稿不动：它本来就是这次保存的来源值，若期间又点过，队列里还有下一次。
    } catch (err) {
      this.realignDraftToPersisted()
    }
  }

  // 保存失败时把顶栏草稿按最新已知磁盘真相回正。
  // 但如果用户在这期间又点过（队列里还有 pending 目标），草稿必须保持那个目标，
  // 否则旧请求的失败会覆盖后续点击想去的值。
  private realignDraftToPersisted(): void {
    if (this.hasPendingEditorFormat) {
      return
    }
    this.editorFormatDraftFontSize = this.settings.editorFontSize
    this.editorFormatDraftLineSpacing = this.settings.editorLineSpacingMultiplier
  }

  // 顶栏步进器的落点：先同步草稿，再把草稿目标塞进 pending 队列。
  // 连点时草稿是累加的（16 → 17 → 18 → 19），不会像原来那样
  // 三次都从同一个"最后一次已落盘值"算出同一个 17。
  private changeEditorFontSize(fontSizePt: number): void {
    this.editorFormatDraftFontSize = fontSizePt
    this.applyEditorFormat(this.editorFormatDraftFontSize, this.editorFormatDraftLineSpacing)
  }

  private changeEditorLineSpacing(lineSpacingMultiplier: number): void {
    this.editorFormatDraftLineSpacing = lineSpacingMultiplier
    this.applyEditorFormat(this.editorFormatDraftFontSize, this.editorFormatDraftLineSpacing)
  }

  // Issue #825 复核第 2 项：选中工具时同时请求展开 ToolPane
  // （rail 负责“选工具 + 展开/收起”）。展开要走 requestToolPaneOpen，
  // 让 Core 在放不下时决定让另一侧让位或回滚，不留死锁状态。
  private onSelectTool(key: string): void {
    this.requestToolPaneOpen(key, true).catch(() => {
      // 请求失败（Core 不可用）时保持当前布局，不额外弹提示。
    })
  }

  private onToggleToolPane(): void {
    if (this.rightPaneCollapsed) {
      this.requestToolPaneOpen(this.selectedToolKey || 'starmap', true).catch(() => {})
    } else {
      this.closeToolPane()
    }
  }

  // Issue #825 复核第5项：把手走整屏覆盖层，按“窗口左缘 / 分隔边缘 / 窗口右缘”显式排位。
  // 之前是把窄 Stack 塞进外层 Stack 再用 offset 猜坐标，而 Stack 的 offset 是相对自身对齐点算的，
  // 收起态会落到屏幕中间。这里用一段带占位宽度的整屏 Row 从左往右排：
  // 左把手在目录栏右缘（前置占位 (目录栏宽 - 半把手)，按钮中心压在目录栏边界上），
  // 右把手在 ToolPane 左缘（占位放在按钮之后，按钮中心压在分隔线上；收起时贴窗口右缘）。
  // 中间 layoutWeight 的 Blank 吃掉剩余空间，位置由布局算出来而不是猜出来。
  // 覆盖层自身不参与命中测试（HitTestMode.Transparent），只有两个 Button 可点，
  // 正文、目录树和工具区的触摸照常落到下层。
  @Builder
  PaneHandleLayer() {
    Row() {
      if (!this.leftPaneCollapsed) {
        Blank()
          .width(this.chapterNavigationWidthVp() - PANE_HANDLE_WIDTH_VP / 2)
          .hitTestBehavior(HitTestMode.None)
      }

      Button(this.leftPaneCollapsed ? '›' : '‹', { type: ButtonType.Normal })
        .width(PANE_HANDLE_WIDTH_VP)
        .height(PANE_HANDLE_HEIGHT_VP)
        .fontSize(14)
        .fontColor($r('sys.color.font_primary'))
        .backgroundColor($r('sys.color.comp_background_gray'))
        .borderRadius(DesignTokens.radiusPill)
        .onClick(() => {
          // 展开要走 Core 判定（必要时让右侧让位 / 回滚），收起永远放得下。
          if (this.leftPaneCollapsed) {
            this.requestChapterNavigationOpen().catch(() => {})
          } else {
            this.closeChapterNavigation()
          }
        })

      Blank()
        .layoutWeight(1)
        .hitTestBehavior(HitTestMode.None)

      // 右把手占位必须在按钮“之后”：收起时没有占位，按钮自然贴窗口右缘；
      // 展开时按钮后面补 (ToolPane 宽 - 半把手)，按钮中心正好压在“正文 | ToolPane”分隔线上。
      // rail 恒在最右，占位不包含 rail 宽度。
      Button(this.rightPaneCollapsed ? '‹' : '›', { type: ButtonType.Normal })
        .width(PANE_HANDLE_WIDTH_VP)
        .height(PANE_HANDLE_HEIGHT_VP)
        .fontSize(14)
        .fontColor($r('sys.color.font_primary'))
        .backgroundColor($r('sys.color.comp_background_gray'))
        .borderRadius(DesignTokens.radiusPill)
        .onClick(() => { this.onToggleToolPane() })

      if (!this.rightPaneCollapsed) {
        Blank()
          .width(this.toolPaneWidthVp() - PANE_HANDLE_WIDTH_VP / 2)
          .hitTestBehavior(HitTestMode.None)
      }
    }
    .width('100%')
    .height('100%')
    .alignItems(VerticalAlign.Center)
    .hitTestBehavior(HitTestMode.Transparent)
  }

  // Issue #829：ToolPane 骨架固定下来。rail 只切“星图 / AI”，这里负责承载内容。
  // 本轮只定空间结构和标题槽，不在页面层伪造星图 / AI 业务状态；后续真实内容直接填入
  // 下面的内容 Stack，不再改工作台列数、宽度或展开/收起逻辑。
  @Builder
  RightToolPane() {
    Column() {
      Row() {
        // Issue #829 手稿：工具面板自己带一个顶部折叠钮（收起 ToolPane）。
        // 和正文区顶部那个一样是"还开着时主动收起"的入口；
        // 收起后靠 PaneHandleLayer 的贴边把手重新拉开。
        PaneFoldButton({
          glyph: '⌄',
          onTriggered: (): void => { this.closeToolPane() }
        })

        Text(this.selectedToolKey === 'ai' ? 'AI' : '星图')
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .margin({ left: 8 })

        Blank()
      }
      .width('100%')
      .height(48)
      .padding({ left: 16, right: 12 })
      .alignItems(VerticalAlign.Center)

      Column()
        .width('100%')
        .height(1)
        .backgroundColor($r('sys.color.comp_background_gray'))

      Stack() {
        // 星图 / AI 的真实内容后续直接填到这里。
      }
      .width('100%')
      .layoutWeight(1)
    }
    .width(this.toolPaneWidthVp())
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }

  build() {
    // Issue #629 评论9 第3项：NavDestination 作为根容器，onBackPressed 拦截系统返回键/侧滑返回。
    // 拦截默认退栈（return true），异步调 requestLeave()（performGracefulClose 成功才 pop）；
    // 失败留在本页不 pop。aboutToDisappear 不再当返回拦截器，只做页面已离开后的清理。
    // Issue #768 评论5835171266 第3项：API23+ 用 HdsNavDestination 替代 NavDestination，
    // 获得 HDS 沉浸光感材质效果；API < 23 保持普通 NavDestination。
    if (this.useHdsNav) {
      HdsWritingDestinationApi23({
        title: this.chapterTitle,
        onTitleBack: () => { this.handleTitleBack() },
        onSystemBack: () => { this.handleSystemBack() },
        onShown: () => {
          NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.Writing, 0)
        },
        scroller: this.editorScroller,
        // Issue #825 复核第2项：动作数组由本页面统一解析后透传，壳层不再自己解析一遍。
        // Issue #825 复核第1项：同步 / 搜索 / 设置属于 Core 的 ToolbarTrailing 角色，
        // 宽屏也继续显示在顶部，不再搬进右侧 ToolPane（那里只放选中工具的内容）。
        headerActions: this.headerActions,
        // Issue #825 复核第 3 项：宽屏由 WritingWorkbenchToolbar 按 Core 的
        // Toolbar* bounds 画工具条带，HDS 标题栏（只有章节标题 + HeaderTrailing、
        // 高度也不等于 Core 的 toolbar bounds）必须让位，否则工具条带会重复一份。
        // Issue #825 复核5第1项：让位与否看 Core 的最终模式 resolve_workbench_layout().mode，
        // 不能只看第一层 WorkspaceLayoutMode.Workbench —— 窗口仍是 Workbench 但用户展开
        // 右 pane 导致最终 plan=SinglePane 时，内容区已经退回单栏，标题栏必须回来，
        // 否则返回 / 同步 / 搜索 / 设置会一起消失。
        showTitleBar: !this.hasWorkbenchPlan
      }) {
        this.NavContent()
      }
    } else {
      NavDestination() {
        Column() {
          // Issue #825 复核第 3 项：宽屏这条自绘标题行让位给 WritingWorkbenchToolbar
          // （返回归 ToolbarLeading，HeaderTrailing 归 ToolbarTrailing），
          // 否则同一条带会出现两套返回与两套尾部动作。
          // Issue #825 复核5第1项：同 showTitleBar，这里也必须看 Core 的最终模式。
          if (!this.hasWorkbenchPlan) {
          // Issue #783 评论5856245403：自定义标题行 — 圆形返回按钮 + 标题。
          // 标题栏返回走 handleTitleBack（始终退出页面），
          // 系统返回手势走 onBackPressed → handleSystemBack（IME 打开时先收键盘）。
          // 隐藏 NavDestination 内置标题栏，在内容顶部放自己的标题 Row，
          // 这样 API<23 fallback 和 API23+ HDS 分支都有两个明确返回意图。
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

            // Issue #784 评论5857856673 问题2：右侧渲染 HeaderTrailing 动作按钮。
            // 动作来源是 Core ScreenPolicy（this.headerActions），通过 sortedHeaderTrailingActions
            // 排序后 ForEach 渲染，统一调用 NavigationHeaderActionDispatcher.dispatchAction。
            // Issue #825 复核第1项：这组动作属于 ToolbarTrailing，窄屏宽屏都在顶部，
            // 右侧 ToolPane 不再重复它们。
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

  // Issue #767 评论 5828146609 第 2 节：删除正文页右上"分享/保存"按钮。
  // shareToSystem() 已删除，但保留 HarmonyShareService / SharePayload / buildSharePayload() /
  // startShareLifecycle() / updateSharePayload() / stopShareLifecycle()（碰一碰/隔空传送仍在用）。
  // Issue #629 评论14 第1项：编辑区域 Builder（窄屏和宽屏复用同一套编辑会话）。
  // Issue #829 评论2第1项：正文主体和底部状态栏必须分开。
  // 宽屏之前直接复用整个 EditorPane()，而 EditorPane() 末尾还画着窄屏那条
  // 「字数 / 自动保存 / 保存状态」底栏，外层又追加了新的 WritingStatusBar，
  // 于是宽屏出现两条底部状态栏。这里把正文主体抽成不含任何状态栏的 EditorBody()：
  //   - 窄屏 EditorPane() = EditorBody() + 窄屏旧底栏（保留窄屏既有语义）；
  //   - 宽屏              = EditorBody() + WritingStatusBar（唯一一条状态栏）。
  @Builder
  EditorBody() {
    Column() {
      // Issue #783 评论5856245403：返回入口和标题由各分支的标题栏承载
      //（API23+ HDS backIcon / API<23 自定义标题 Row），正文主体只负责正文本身。
      // Issue #767 评论 5828146609 第 2 节：删除顶部操作栏（分享/保存按钮已删）。

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
            .onClick(() => this.loadChapter())
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        SujianEditor({
          scroller: this.editorScroller,
          presentationUpdate: this.editorSnapshot !== null
            ? new EditorPresentationUpdate(
                this.editorSnapshot,
                this.editorEditResult,
                this.coordinator.getSessionId() ?? 0
              )
            : null,
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
          .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      }
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Center)
  }

  // 窄屏专用底栏（Issue #767 评论 5828146609 第 2 节：字数显示合并到底部保存状态栏）。
  // Issue #829 评论2第1项：宽屏不再走这个 Builder，改用 WritingStatusBar。
  @Builder
  NarrowStatusBar() {
    Row() {
        // 字数显示（从顶部操作栏合并到此）
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
  // Issue #829 评论2第1项：宽屏不调这个 Builder，只调 EditorBody() + WritingStatusBar，
  // 保证任何时候窗口里只有一条底部状态栏、也只有一份状态。
  @Builder
  EditorPane() {
    Column() {
      this.EditorBody()
      this.NarrowStatusBar()
    }
    .width('100%')
    .height('100%')
  }
}
