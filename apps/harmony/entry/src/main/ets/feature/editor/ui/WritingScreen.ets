import { Routes, WritingRouteParams } from '../../../app/navigation/Routes'
import { AppNavigationHost } from '../../../app/navigation/AppNavigation'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { ChapterContent, ChapterSaveReceipt } from '../../../corebridge/dto/ProjectDtos'
import { LocalSettings } from '../../../corebridge/dto/SettingsDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { HarmonyLifecycle, AutoSaveLifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { EditorSessionCoordinator, EditorStateUpdate } from '../session/EditorSessionCoordinator'
import { NativeEditorSessionBridge } from '../interop/NativeEditorSessionBridge'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { EditorInputAdapter } from '../input/EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
import { EditorSemanticDispatcher } from '../input/EditorSemanticDispatcher'
import { SujianEditor } from './SujianEditor'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'
import { HarmonyShareService } from '../../../platform/share/HarmonyShareService'
import { SharePayload } from '../../../platform/share/SharePayload'
import { EditorViewportState } from '../render/EditorViewportState'
import { EditorViewportController } from '../render/EditorViewportController'
import { LifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { ChapterTreePane } from '../../project/ui/ChapterTreePane'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { VolumeTree } from '../../../corebridge/dto/ProjectDtos'

@Component
export struct WritingScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
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
    linuxQtSidebarWidth: 0,
    linuxQtEditorWidth: 0
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
  @State sessionOldText: string = ''
  @State sessionId: string = ''
  @State sessionStartTime: number = 0
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  @State editorSnapshot: EditorLayoutSnapshot | null = null
  // Issue #629 评论14 第3项：当前内容宽度，由 SujianEditor 回调更新。
  // 用于构造 EditorLayoutSnapshot 时传递 contentWidth（行导航 resolver 身份匹配）。
  @State contentWidthForSnapshot: number = 0
  // Issue #629 评论14 第1项：宽屏双栏模式下的目录数据。
  // WritingScreen 在宽屏模式下持有 ChapterTreePane，需要加载项目树。
  @State volumeTrees: VolumeTree[] = []
  @State expandedVolumes: Set<string> = new Set()
  @State isTreeLoading: boolean = false
  // 当前 UI 滚动位置（viewport），接续恢复时只设 UI，不写 Core。
  // 强类型 EditorViewportState，不用裸 object（Issue #629 评论 5 第 8 节）。
  @State currentViewport: EditorViewportState | null = null

  private bridge: IWriterCoreBridge = getBridge()
  private projectBridge: IWriterCoreBridge = getBridge()
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
  // Share 生命周期观察者：onForeground 重新注册，onBackground stop（成对）。
  private shareLifecycleObserver: LifecycleObserver | null = null
  // 接续进入时携带的编辑器状态（selection/cursor/viewport/contentHash），loadChapter 恢复时消费。
  private continuationParams: WritingRouteParams | null = null
  // Issue #629 评论6 Part C：父子 viewport 恢复通道 controller。
  // WritingScreen 持有 controller 实例并注入 SujianEditor；接续加载完成、目标设备 layout 就绪后
  // 调 controller.restore(state)，controller 按目标设备当前 viewport/content 尺寸 clamp + scrollTo。
  private viewportController: EditorViewportController = new EditorViewportController()
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
  private activeSave: { text: string; task: Promise<boolean> } | null = null
  // Issue #629 评论 5308748920 问题2：离开事务（close + pop）整体幂等。
  // 已有 leaveTask 时直接返回同一 task，task 内只 pop 一次。
  // close 失败时清 leaveTask 允许再次返回重试；成功后不重置（页面已离开）。
  private leaveTask: Promise<void> | null = null
  // Issue #629 评论14 第2项：离开守卫唯一 token。registerLeaveGuard 返回，
  // aboutToDisappear 用此 token 注销，避免误注销其他 WritingScreen 实例的 guard。
  private leaveGuardToken: number = -1

  aboutToAppear() {
    const paramsArr = this.navPathStack.getParamByName(Routes.Writing) as WritingRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.chapterId = params.chapterId
      this.chapterTitle = params.chapterTitle
      this.projectId = params.projectId || ''
      this.volumeId = params.volumeId || ''
      // 暂存接续参数，loadChapter 在 coordinator.open 成功后消费以恢复编辑器状态。
      this.continuationParams = params
      this.loadChapter()
      this.loadSettings()
      // Issue #629 评论14 第1项：宽屏双栏模式下加载项目树（供 ChapterTreePane）。
      if (this.adaptiveState.isTwoPane) {
        this.loadProjectTree()
      }
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
      self.editorSnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap, self.settings.editorFontSize, self.contentWidthForSnapshot)
      self.content = snap.text
      self.hasUnsavedChanges = snap.text !== self.lastSavedContent
      // 实时字数：消费 editResult.contentDelta（Core 非空白字符增量）
      if (update.editResult !== null) {
        const delta = update.editResult.contentDelta
        self.wordCount = Math.max(0, self.wordCount + delta.insertedNonWhitespaceChars - delta.deletedNonWhitespaceChars)
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
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver!)

    // Issue #629 评论6 Part G：退后台用 performBackgroundSave（flush+idle+save），
    // 不只 saveChapter，避免最后几个字还在 dispatcher 队列里就 save 丢输入。
    this.autoSaveObserver = new AutoSaveLifecycleObserver(
      (): Promise<void> => this.performBackgroundSave(),
      this.settings.autoSaveDelayMs
    )
    this.lifecycle.addObserver(this.autoSaveObserver)

    // Issue #629 评论 5 第 7 节：碰一碰/隔空传送由页面拥有生命周期。
    // 进入前台（aboutToAppear）注册 startTapSharing/startAirTransferSharing；
    // onForeground 重新注册，onBackground stop（成对，不全局乱开）。
    this.startShareLifecycle()
    this.shareLifecycleObserver = {
      onForeground: (): void => { this.startShareLifecycle() },
      onBackground: (): void => { this.stopShareLifecycle() }
    }
    this.lifecycle.addObserver(this.shareLifecycleObserver!)

    // Issue #629 评论14 第2项：注册当前 WritingScreen 的离开守卫，获取唯一 token。
    // AppNavigationHost.safePop/safeClearAndRebuild/safeReplacePath 会 await 此 guard（performGracefulClose），
    // 只有保存成功且 Core close 成功才允许改栈，未保存正文不被程序化 clear/pop 绕过。
    this.leaveGuardToken = AppNavigationHost.getInstance().registerLeaveGuard((): Promise<boolean> => this.performGracefulClose())
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
      //    或 afterFinish.composition!==null，立即 unseal 并 return false，不进入保存。
      //    发生 stale/恢复时 Core 可能仍保留 composition，close 必须以最终 Core snapshot 为准。
      const afterFinish = this.coordinator.getSnapshot()
      if (finishResult.success === false || afterFinish === null || afterFinish.composition !== null) {
        this.dispatcher.unseal()
        return false
      }
      // 6. 保存稳定 snapshot（此时不再读可能已变化的 this.content）
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
      } else if (this.sessionOldText !== savedText && this.chapterId) {
        // 没有未保存变更但有统计差异时，兜底上报一次写作统计
        const deviceId = this.settings.statsDeviceId || 'unknown'
        const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
        this.bridge.processWritingEvent(
          deviceId, 'harmony', this.projectId, this.volumeId,
          this.chapterId, this.sessionOldText, savedText, durationSeconds, this.sessionId
        )
        // Issue #629 评论7 第5项：统计成功推进 sessionOldText，避免第二遍重复统计。
        this.sessionOldText = savedText
      }
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
  // Issue #629 评论11 第3项：requestLeave 改用 AppNavigationHost.safePop()。
  // safePop 内部 runGuardedNavigation 先 await leaveGuard（performGracefulClose），
  // 成功才 navPathStack.pop()，且两个并发 safePop 合并成一次事务（guard 一次、pop 一次）。
  // leaveTask 仍保留作为 requestLeave 自身重入防护（safePop 内部 navigationTask 也幂等，双保险）。
  private requestLeave(): Promise<void> {
    if (this.leaveTask !== null) {
      return this.leaveTask
    }
    const task: Promise<void> = (async () => {
      await AppNavigationHost.getInstance().safePop()
    })()
    this.leaveTask = task
    task.then(
      () => {
        // safePop 完成（无论成功失败）：清 leaveTask 允许再次调用。
        if (this.leaveTask === task) {
          this.leaveTask = null
        }
      },
      () => {
        if (this.leaveTask === task) {
          this.leaveTask = null
        }
      }
    )
    return task
  }

  // Issue #629 评论9 第3项：aboutToDisappear 不再承担“阻止丢正文”职责——返回拦截由
  // NavDestination.onBackPressed + requestLeave() 统一处理。aboutToDisappear 只做页面已确定
  // 离开后的 observer/share/view 清理（removeObserver、stopShareLifecycle、setStateListener(null)）。
  // 不能把它当返回拦截器：此时路由已退栈，无法阻止返回。
  async aboutToDisappear() {
    // Issue #629 评论14 第2项：用唯一 token 注销自己的离开守卫，
    // 不影响其他 WritingScreen 实例（如宽屏切章节时新旧实例生命周期重叠）。
    if (this.leaveGuardToken >= 0) {
      AppNavigationHost.getInstance().unregisterLeaveGuard(this.leaveGuardToken)
      this.leaveGuardToken = -1
    }
    // 清除监听器（避免 close 后回调）
    this.coordinator.setStateListener(null)

    // Issue #629 评论 5 第 7 节：页面离开时 stop 碰一碰/隔空传送（成对，不留监听）。
    this.stopShareLifecycle()

    if (this.autoSaveObserver) {
      this.lifecycle.removeObserver(this.autoSaveObserver)
    }
    if (this.shareLifecycleObserver) {
      this.lifecycle.removeObserver(this.shareLifecycleObserver)
      this.shareLifecycleObserver = null
    }
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
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
        this.sessionOldText = result.data.content
        this.sessionId = `hm-${this.chapterId}-${Date.now()}`
        this.sessionStartTime = Date.now()
        // 打开编辑会话，用 Core 管理正文。open 成功后 listener 自动回流 editorSnapshot/content/hasUnsaved/wordCount。
        // Issue #629 评论8 第5项：章节正文是持续编辑内容，传 isPersistent=true（persistent session），
        // 不再把章节伪装成 temporary session。
        const snapResult = await this.coordinator.open(this.chapterId, result.data.content, true)
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

  // 普通系统分享按钮：只调用 HarmonyShareService.share(payload, 'system')。
  // 碰一碰/隔空传送不走此入口，走其生命周期接口。
  private async shareToSystem(): Promise<void> {
    const payload = this.buildSharePayload()
    if (payload === null) {
      return
    }
    await this.shareService.share(payload, 'system')
  }

  // Issue #629 评论14 第1项：宽屏双栏模式加载项目树。
  private async loadProjectTree(): Promise<void> {
    if (!this.projectId || this.adaptiveState.isTwoPane === false) {
      return
    }
    this.isTreeLoading = true
    try {
      const result = await this.bridge.getProjectTree(this.projectId)
      if (result.success && result.data) {
        this.volumeTrees = result.data.volumes
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
    const host = AppNavigationHost.getInstance()
    const targetKey = `writing:${this.projectId}:${volumeId}:${chapterId}`
    await host.safeReplacePath(targetKey, () => {
      this.navPathStack.replace({
        name: Routes.Writing,
        param: {
          chapterId: chapterId,
          chapterTitle: chapterTitle,
          projectId: this.projectId,
          volumeId: volumeId
        }
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
        this.settings = result.data
        this.themeAdapter.updateFromSettings({
          theme: result.data.themeMode,
          fontSize: result.data.editorFontSize,
          fontFamily: 'HarmonyOS Sans',
          lineHeight: result.data.editorLineSpacingMultiplier
        } as ThemeUpdateParams)

        if (this.autoSaveObserver) {
          this.lifecycle.removeObserver(this.autoSaveObserver)
          this.autoSaveObserver = new AutoSaveLifecycleObserver(
            (): Promise<void> => this.performBackgroundSave(),
            this.settings.autoSaveDelayMs
          )
          this.lifecycle.addObserver(this.autoSaveObserver)
        }
      }
    } catch (err) {
      // Ignore settings load error
    }
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
    this.activeSave = { text: targetText, task: task }
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
  // 结算：processWritingEvent/sessionOldText/lastSavedContent 按 targetText；
  // 再取 latestSnapshot：this.content = latestSnapshot.text；
  // hasUnsavedChanges = latestSnapshot.text !== targetText（保存期间新输入保持 unsaved）。
  // contentHash 是这次 targetText 的回执。wordCount 是当前编辑器正文的实时字数（listener 实时更新），
  // 只有保存期间无新输入时才用 receipt.wordCount 校正；有新输入时保留实时 wordCount。
  private async doSaveChapter(targetText: string, isAutoSave: boolean): Promise<boolean> {
    this.isSaving = true
    this.isAutoSaving = isAutoSave
    this.saveMessage = ''

    try {
      // 只保存这份 targetText（不重新抓当前正文）
      const result = await this.bridge.saveChapter(this.chapterId, targetText)
      if (result.success && result.data) {
        // 按 targetText 结算统计（不读可能已变化的 this.content）
        if (this.sessionOldText !== targetText) {
          const deviceId = this.settings.statsDeviceId || 'unknown'
          const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
          this.bridge.processWritingEvent(
            deviceId, 'harmony', this.projectId, this.volumeId,
            this.chapterId, this.sessionOldText, targetText, durationSeconds, this.sessionId
          )
          this.sessionOldText = targetText
          this.sessionId = `hm-${this.chapterId}-${Date.now()}`
          this.sessionStartTime = Date.now()
        }
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
    if (this.lastSaveFailed) return this.themeConfig.colors.error
    if (this.isSaving) return this.themeConfig.colors.textSecondary
    if (this.hasUnsavedChanges) return this.themeConfig.colors.secondary
    return this.themeConfig.colors.success
  }

  private getSaveStatusText(): string {
    if (this.lastSaveFailed) return getContext(this).resourceManager.getStringSync($r('app.string.save_failed').id)
    if (this.isSaving) return getContext(this).resourceManager.getStringSync($r('app.string.saving').id)
    if (this.hasUnsavedChanges) return getContext(this).resourceManager.getStringSync($r('app.string.unsaved').id)
    return getContext(this).resourceManager.getStringSync($r('app.string.saved').id)
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

  build() {
    // Issue #629 评论9 第3项：NavDestination 作为根容器，onBackPressed 拦截系统返回键/侧滑返回。
    // 拦截默认退栈（return true），异步调 requestLeave()（performGracefulClose 成功才 pop）；
    // 失败留在本页不 pop。aboutToDisappear 不再当返回拦截器，只做页面已离开后的清理。
    NavDestination() {
      if (this.adaptiveState.isTwoPane) {
        // Issue #629 评论14 第1项：宽屏双栏布局。
        // 左侧：ChapterTreePane（纯目录 UI，不持有编辑会话）。
        // 右侧：当前 WritingScreen 的编辑会话（同一个 coordinator/inputAdapter/dispatcher/IME/save/close）。
        // 屏幕宽度变化只改布局，不换 TextEditSession owner。
        Row() {
          // 左侧目录面板
          ChapterTreePane({
            volumeTrees: this.volumeTrees,
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
            .width(320)
            .height('100%')

          // 分割线
          Column() {}
            .width(0.5)
            .height('100%')
            .backgroundColor(this.themeConfig.colors.divider)

          // 右侧编辑区域（同一个 WritingScreen 编辑会话）
          this.EditorPane()
        }
        .width('100%')
        .height('100%')
      } else {
        // Issue #629 评论9 第3项：窄屏单栏布局。
        this.EditorPane()
      }
    }
    .hideTitleBar(true)
    .onBackPressed(() => {
      // 拦截默认退栈：异步调 requestLeave()（成功才 pop），立即 return true 阻止默认回退。
      // 失败时 requestLeave 不 pop，用户留在本页可继续编辑。
      this.requestLeave()
      return true
    })
  }

  // Issue #629 评论14 第1项：编辑区域 Builder（窄屏和宽屏复用同一套编辑会话）。
  @Builder
  EditorPane() {
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => {
            this.requestLeave()
          })

        Text(this.chapterTitle)
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        if (this.saveMessage) {
          Text(this.saveMessage)
            .fontSize(12)
            .fontColor(this.getSaveStatusColor())
            .margin({ right: 8 })
        }

        Text(getContext(this).resourceManager.getStringSync($r('app.string.word_count_format').id).replace('%s', `${this.wordCount}`))
          .fontSize(12)
          .fontColor(this.themeConfig.colors.textSecondary)
          .margin({ right: 8 })

        Button($r('app.string.save'))
          .fontSize(14)
          .height(32)
          .backgroundColor(this.hasUnsavedChanges ? this.themeConfig.colors.primary : this.themeConfig.colors.divider)
          .fontColor(this.hasUnsavedChanges ? this.themeConfig.colors.onPrimary : this.themeConfig.colors.textSecondary)
          .enabled(!this.isSaving && this.hasUnsavedChanges)
          .onClick(() => this.saveChapter())

        Button($r('app.string.share'))
          .fontSize(14)
          .height(32)
          .margin({ left: 8 })
          .backgroundColor(this.themeConfig.colors.secondary)
          .fontColor(this.themeConfig.colors.onPrimary)
          .onClick(() => this.shareToSystem())
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 20, bottom: 14 })

      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
            .color(this.themeConfig.colors.primary)
          Text($r('app.string.loading'))
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textHint)
            .margin({ top: 12 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .fontColor(this.themeConfig.colors.error)
            .margin({ bottom: 16 })

          Button($r('app.string.retry'))
            .onClick(() => this.loadChapter())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else {
        SujianEditor({
          snapshot: this.editorSnapshot,
          harmonyImeConnection: this.harmonyImeConnection,
          inputAdapter: this.inputAdapter,
          selectionController: this.selectionController,
          dispatcher: this.dispatcher,
          viewportController: this.viewportController,
          onViewportChange: (state: EditorViewportState): void => {
            this.currentViewport = state
            this.updateContinuationState()
          },
          // Issue #629 评论14 第3项：内容宽度变化时更新 editorSnapshot 的 contentWidth。
          onContentWidthChange: (width: number): void => {
            this.contentWidthForSnapshot = width
            // 重新构造带正确 contentWidth 的 editorSnapshot（供行导航 resolver 身份匹配）。
            const snap = this.coordinator.getSnapshot()
            if (snap) {
              this.editorSnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap, this.settings.editorFontSize, width)
            }
          }
        })
          .width('100%')
          .layoutWeight(1)
          .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      }

      // Bottom toolbar
      Row() {
        if (this.settings.autoSaveEnabled) {
          Text($r('app.string.auto_save'))
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
        }

        Blank()

        Text(this.getSaveStatusText())
          .fontSize(12)
          .fontColor(this.getSaveStatusColor())
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 10, bottom: 10 })
      .backgroundColor(this.themeConfig.colors.background)
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
    .alignItems(HorizontalAlign.Center)
  }
}
