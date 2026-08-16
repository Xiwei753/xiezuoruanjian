import { Routes, WritingRouteParams } from '../../../app/navigation/Routes'
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
import { EditorSessionSnapshot, EditorTransactionCause, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
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
  // 当前 UI 滚动位置（viewport），接续恢复时只设 UI，不写 Core。
  // 强类型 EditorViewportState，不用裸 object（Issue #629 评论 5 第 8 节）。
  @State currentViewport: EditorViewportState | null = null

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
  // Issue #629 评论8 第1项：保存事务复用。正在保存时后续调用必须 await/复用同一任务，
  // 不再 if (isSaving) return true。事务完成后释放（成功失败都释放，允许下次新保存）。
  private saveTask: Promise<boolean> | null = null

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
    }

    const self = this
    // 注册编辑会话状态监听器：open/编辑命令成功后回流 editorSnapshot/content/hasUnsaved/wordCount。
    // 选区/正文变化后持续写入当前接续状态，保证 onContinue 读取到最新编辑器状态。
    // Issue #629 评论9 第4项：listener 改 typed EditorStateUpdate{snapshot, editResult}。
    // wordCount 实时来自 editResult.contentDelta（insertedNonWhitespaceChars/deletedNonWhitespaceChars，
    // Core 字数语义），不用 snap.text.length（JS UTF-16 code unit 数）。
    // editResult=null（open 初始加载 / snapshot 恢复）时不更新 wordCount（保持上次值）。
    this.coordinator.setStateListener((update: EditorStateUpdate) => {
      const snap = update.snapshot
      self.editorSnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap, self.settings.editorFontSize)
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
      await this.dispatcher.finishActiveComposition()
      // 3. drain dispatcher（所有已排队语义输入应用完）
      await this.dispatcher.flush()
      // 4. 等 coordinator 命令队列空闲（所有 Core 编辑命令完成，snapshot 已包含最后输入）
      await this.coordinator.whenIdle()
      // 5. 保存稳定 snapshot（此时不再读可能已变化的 this.content）
      const snap = this.coordinator.getSnapshot()
      const savedText: string = snap ? snap.text : this.content
      if (savedText !== this.lastSavedContent) {
        // 保存事务可能复用了进行中任务（自动保存/手动保存冻结的是更早 snapshot）：
        // 复用任务完成后必须校验 lastSavedContent 正是本次 close 的 savedText，
        // 不是则再保存一轮（此时 saveTask 已释放，新事务冻结当前最新 snapshot）。
        // 这样“保存期间新输入”在 close 路径上也不会被误标成已保存后离开。
        let settled: boolean = await this.saveChapter()
        if (!settled) {
          // 保存失败：不 detach、不 close、不 pop；解锁输入让用户继续编辑（下次返回重试）。
          this.dispatcher.unseal()
          return false
        }
        let guard: number = 0
        while (this.lastSavedContent !== savedText && guard < 5) {
          settled = await this.saveChapter()
          if (!settled) {
            this.dispatcher.unseal()
            return false
          }
          guard++
        }
        if (this.lastSavedContent !== savedText) {
          // 连续多轮仍未把 close 时的 savedText 落盘（异常情况）：不离开，正文留在活跃会话。
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
      // 6. 关闭编辑会话。Issue #629 评论9 第2项：先 closeAsync，成功后才 detach IME。
      const closeResult = await this.coordinator.closeAsync()
      if (!closeResult.success) {
        // close 失败：不 detach、不 pop；unseal 让用户继续编辑（不需要重新 attach IME）。
        this.dispatcher.unseal()
        return false
      }
      // 7. Core close 成功后才 detach IME（不可逆清理放到最后，失败路径不触不可逆动作）
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
  private async requestLeave(): Promise<void> {
    const closed: boolean = await this.performGracefulClose()
    if (closed === true) {
      this.navPathStack.pop()
    }
  }

  // Issue #629 评论9 第3项：aboutToDisappear 不再承担“阻止丢正文”职责——返回拦截由
  // NavDestination.onBackPressed + requestLeave() 统一处理。aboutToDisappear 只做页面已确定
  // 离开后的 observer/share/view 清理（removeObserver、stopShareLifecycle、setStateListener(null)）。
  // 不能把它当返回拦截器：此时路由已退栈，无法阻止返回。
  async aboutToDisappear() {
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

  // Issue #629 评论8 第1项：保存事务入口（复用同一任务）。
  // 正在保存时后续调用必须 await/复用同一任务，不再 if (isSaving) return true。
  // 手动保存/自动保存/关闭保存都走 doSaveChapter 这一套保存事务，不各写一套状态结算。
  async saveChapter(isAutoSave: boolean = false): Promise<boolean> {
    if (this.saveTask !== null) {
      return this.saveTask
    }
    const task: Promise<boolean> = this.doSaveChapter(isAutoSave)
    this.saveTask = task
    // 事务完成后释放（成功失败都释放，允许下次保存新任务）；
    // 用同一性检查避免误清新任务（并发窗口：旧任务完成瞬间新任务已启动）。
    task.then(
      () => {
        if (this.saveTask === task) {
          this.saveTask = null
        }
      },
      () => {
        if (this.saveTask === task) {
          this.saveTask = null
        }
      }
    )
    return task
  }

  // Issue #629 评论8 第1项：真实保存事务。
  // 流程：flush dispatcher → await coordinator idle → 只读一次 Core snapshot 得到不可变
  // savedText → bridge.saveChapter(chapterId, savedText) 只保存这份文本 → 返回后按 savedText
  // 结算（processWritingEvent/sessionOldText/lastSavedContent）→ 再取 latestSnapshot：
  // this.content = latestSnapshot.text；hasUnsavedChanges = latestSnapshot.text !== savedText
  // （保存期间新输入保持 unsaved，不把 A+B 标成已保存）。
  // contentHash 是这次 savedText 的回执。wordCount 是当前编辑器正文的实时字数（listener 实时更新），
  // 只有保存期间无新输入时才用 receipt.wordCount 校正；有新输入时保留实时 wordCount。
  private async doSaveChapter(isAutoSave: boolean): Promise<boolean> {
    this.isSaving = true
    this.isAutoSaving = isAutoSave
    this.saveMessage = ''

    try {
      // 1+2. 取得稳定 committed snapshot：flush 所有排队语义输入 + 等 Core 命令队列空闲。
      // 之后 savedText 不再受保存期间新输入影响（此快照是本次保存的结算基准）。
      if (this.dispatcher) {
        await this.dispatcher.flush()
      }
      await this.coordinator.whenIdle()
      // 3. 不可变 savedText：只读一次 Core snapshot，此后不再读可能已变化的 this.content。
      const snap = this.coordinator.getSnapshot()
      const savedText: string = snap ? snap.text : this.content
      // 4. 只保存这份文本
      const result = await this.bridge.saveChapter(this.chapterId, savedText)
      if (result.success && result.data) {
        // 5. 按 savedText 结算统计（不读可能已变化的 this.content）
        if (this.sessionOldText !== savedText) {
          const deviceId = this.settings.statsDeviceId || 'unknown'
          const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
          this.bridge.processWritingEvent(
            deviceId, 'harmony', this.projectId, this.volumeId,
            this.chapterId, this.sessionOldText, savedText, durationSeconds, this.sessionId
          )
          this.sessionOldText = savedText
          this.sessionId = `hm-${this.chapterId}-${Date.now()}`
          this.sessionStartTime = Date.now()
        }
        this.lastSavedContent = savedText
        // Issue #629 评论 5 第 8 节：保存成功后跨设备内容身份来自 ChapterSaveReceipt.contentHash。
        this.lastSavedContentHash = result.data.contentHash
        this.hasUnsavedChanges = false
        this.lastSaveFailed = false
        // 6. 重新取 latestSnapshot 结算：保存期间若又产生新输入，hasUnsavedChanges 保持 true，
        //    不把新版正文标成已保存。
        // Issue #629 评论9 第4项：wordCount 表示当前编辑器正文的实时字数（listener 按
        // editResult.contentDelta 实时更新）。只有保存期间无新输入（latest.text === savedText）
        // 时才用 receipt.wordCount 校正当前实时字数；有新输入时保留实时 wordCount，
        // 不拿旧 savedText 的 receipt.wordCount 覆盖当前 A+B 的实时字数。
        const latest = this.coordinator.getSnapshot()
        if (latest) {
          this.content = latest.text
          this.hasUnsavedChanges = latest.text !== savedText
          if (latest.text === savedText) {
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

  // Issue #629 评论 5 第 4 节 + 评论9 第4项：清空正文必须是一条编辑事务，不在活跃 TextEditSession 外直接 clear 文件。
  // 取 Coordinator 当前 snapshot，把 0..UTF8正文末尾 replace 为 ''，cause 用 Programmatic。
  // replace 成功后 state listener 自动回流空正文（content=''、hasUnsavedChanges=true）并按
  // editResult.contentDelta 实时更新 wordCount——doClearContent 不再单独算字数，走同一 listener 路径。
  // 等 listener 回流后走同一套 saveChapter() 持久化。不直接 bridge.clearChapter()（会让磁盘正文和
  // 编辑器正文拆成两份）。
  private async doClearContent() {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.clear_failed').id)
      return
    }
    // replace 阶段用 isSaving 保护，避免并发；replace 完成后释放，让 saveChapter 自己管 isSaving。
    this.isSaving = true
    let replaceOk: boolean = false
    try {
      // 把当前正文 0..末尾 replace 为 ''，cause=Programmatic。
      // replace 的 byte offset 是 UTF-8；snap.text.length 是 UTF-16 code unit 数，
      // 用 TextOffsetMapper.utf16ToUtf8 转成 UTF-8 byte 长度作为 replace 末尾。
      const fullText: string = snap.text
      const utf8EndByte: number = TextOffsetMapper.utf16ToUtf8(fullText, fullText.length)
      const replaceResult = await this.coordinator.replace(
        0, utf8EndByte, '', fullText, EditorTransactionCause.Programmatic
      )
      if (replaceResult.success && replaceResult.data) {
        // state listener 已按 editResult.contentDelta 实时更新 wordCount（非空白字符增量），
        // 并回流空正文（content=''、hasUnsavedChanges=true）。此处不再单独算字数。
        replaceOk = true
      }
    } catch (err) {
      replaceOk = false
    }
    this.isSaving = false
    if (!replaceOk) {
      this.lastSaveFailed = true
      this.saveMessage = getContext(this).resourceManager.getStringSync($r('app.string.clear_failed').id)
      return
    }
    // 走同一套 saveChapter 持久化空正文（saveChapter 内部自己管 isSaving；
    // 保存后 ChapterSaveReceipt.wordCount/contentHash 再校正）。
    const saved: boolean = await this.saveChapter()
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
      Column() {
        // Header
        Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => {
            // Issue #629 评论9 第3项：返回按钮走统一离开入口 requestLeave()（performGracefulClose
            // 成功才 pop），与系统返回键/侧滑返回走同一链，禁止多套关闭流程。
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

        // 普通系统分享按钮（Issue #629 评论 5 第 7 节）：只调用 share(payload, 'system')。
        // 碰一碰/隔空传送不走此入口，走其生命周期接口（startTapSharing/startAirTransferSharing）。
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
        // 自研写作区组件，消费 EditorLayoutSnapshot 静态渲染
        // Issue #629 评论6 Part A+D：传 dispatcher（统一串行边界），删除 onSelectionTap（双 owner）。
        SujianEditor({
          snapshot: this.editorSnapshot,
          harmonyImeConnection: this.harmonyImeConnection,
          inputAdapter: this.inputAdapter,
          selectionController: this.selectionController,
          dispatcher: this.dispatcher,
          // Issue #629 评论6 Part C：父子 viewport 恢复通道 controller。
          // SujianEditor 在 aboutToAppear 注册 scrollTo + getDimensions，
          // WritingScreen 调 controller.restore(state) 时按目标设备当前尺寸 clamp + scrollTo。
          viewportController: this.viewportController,
          // Issue #629 评论 5 第 8 节：滚动时把 viewport 状态回调给 WritingScreen，
          // 持续写入接续状态。
          onViewportChange: (state: EditorViewportState): void => {
            this.currentViewport = state
            this.updateContinuationState()
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
    .hideTitleBar(true)
    .onBackPressed(() => {
      // 拦截默认退栈：异步调 requestLeave()（成功才 pop），立即 return true 阻止默认回退。
      // 失败时 requestLeave 不 pop，用户留在本页可继续编辑。
      this.requestLeave()
      return true
    })
  }
}
