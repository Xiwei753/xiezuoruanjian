import { Routes, WritingRouteParams } from '../../../app/navigation/Routes'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { ChapterContent, ChapterSaveReceipt } from '../../../corebridge/dto/ProjectDtos'
import { LocalSettings } from '../../../corebridge/dto/SettingsDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeUpdateParams, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { HarmonyLifecycle, AutoSaveLifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { NativeEditorSessionBridge } from '../interop/NativeEditorSessionBridge'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorSessionSnapshot, EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { EditorInputAdapter } from '../input/EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
import { SujianEditor } from './SujianEditor'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'
import { HarmonyShareService } from '../../../platform/share/HarmonyShareService'
import { SharePayload } from '../../../platform/share/SharePayload'
import { EditorViewportState } from '../render/EditorViewportState'
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
  // Share 生命周期服务（Issue #629 评论 5 第 7 节）：碰一碰/隔空传送由页面拥有生命周期。
  private shareService: HarmonyShareService = HarmonyShareService.getInstance()
  // Share 生命周期观察者：onForeground 重新注册，onBackground stop（成对）。
  private shareLifecycleObserver: LifecycleObserver | null = null
  // 接续进入时携带的编辑器状态（selection/cursor/viewport/contentHash），loadChapter 恢复时消费。
  private continuationParams: WritingRouteParams | null = null

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
    // 注册编辑会话状态监听器：open/编辑命令成功后回流 editorSnapshot/content/hasUnsaved。
    // 选区/正文变化后持续写入当前接续状态，保证 onContinue 读取到最新编辑器状态。
    // Issue #629 评论 5 第 4 节：wordCount 不用 snap.text.length（JS UTF-16 code unit 数，
    // 不是 Core 字数语义）。wordCount 只在加载（ChapterContent.meta.wordCount）、保存
    // （ChapterSaveReceipt.wordCount）、清空（contentDelta）时更新，始终来自 Core。
    this.coordinator.setStateListener((snap: EditorSessionSnapshot) => {
      self.editorSnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap, self.settings.editorFontSize)
      self.content = snap.text
      self.hasUnsavedChanges = snap.text !== self.lastSavedContent
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

    this.autoSaveObserver = new AutoSaveLifecycleObserver(
      (): Promise<void> => this.saveChapter(true).then(() => {}),
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

  aboutToDisappear() {
    if (this.hasUnsavedChanges) {
      this.saveChapter()
    } else if (this.sessionOldText !== this.content && this.chapterId) {
      const deviceId = this.settings.statsDeviceId || 'unknown'
      const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
      this.bridge.processWritingEvent(
        deviceId, 'harmony', this.projectId, this.volumeId,
        this.chapterId, this.sessionOldText, this.content, durationSeconds, this.sessionId
      )
    }

    // 清除状态监听器，避免 close 后回调。
    this.coordinator.setStateListener(null)

    // 关闭编辑会话
    this.coordinator.close()

    // detach IME 连接
    this.harmonyImeConnection.detach().then(() => {})

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
    if (params.viewport !== undefined) {
      this.currentViewport = params.viewport
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
            (): Promise<void> => this.saveChapter(true).then(() => {}),
            this.settings.autoSaveDelayMs
          )
          this.lifecycle.addObserver(this.autoSaveObserver)
        }
      }
    } catch (err) {
      // Ignore settings load error
    }
  }

  async saveChapter(isAutoSave: boolean = false): Promise<boolean> {
    if (this.isSaving || !this.hasUnsavedChanges) return true

    this.isSaving = true
    this.isAutoSaving = isAutoSave
    this.saveMessage = ''

    try {
      // 从 coordinator snapshot 取最新正文
      const snap = this.coordinator.getSnapshot()
      if (snap) {
        this.content = snap.text
      }
      const result = await this.bridge.saveChapter(this.chapterId, this.content)
      if (result.success && result.data) {
        if (this.sessionOldText !== this.content) {
          const deviceId = this.settings.statsDeviceId || 'unknown'
          const durationSeconds = Math.round((Date.now() - this.sessionStartTime) / 1000)
          this.bridge.processWritingEvent(
            deviceId, 'harmony', this.projectId, this.volumeId,
            this.chapterId, this.sessionOldText, this.content, durationSeconds, this.sessionId
          )
          this.sessionOldText = this.content
          this.sessionId = `hm-${this.chapterId}-${Date.now()}`
          this.sessionStartTime = Date.now()
        }

        this.lastSavedContent = this.content
        // Issue #629 评论 5 第 8 节：保存成功后跨设备内容身份来自 ChapterSaveReceipt.contentHash。
        this.lastSavedContentHash = result.data.contentHash
        this.hasUnsavedChanges = false
        this.lastSaveFailed = false
        this.wordCount = result.data.wordCount
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

  // Issue #629 评论 5 第 4 节：清空正文必须是一条编辑事务，不在活跃 TextEditSession 外直接 clear 文件。
  // 取 Coordinator 当前 snapshot，把 0..UTF8正文末尾 replace 为 ''，cause 用 Programmatic。
  // 等 state listener 回流空正文后，再走同一套 saveChapter() 持久化。
  // 不直接 bridge.clearChapter()（会让磁盘正文和编辑器正文拆成两份）。
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
        // state listener 已回流空正文（content=''、hasUnsavedChanges=true）。
        // 用 contentDelta 即时校正 wordCount（Core 字数语义，不用 JS .length）。
        const delta = replaceResult.data.contentDelta
        this.wordCount = this.wordCount + delta.insertedChars - delta.deletedChars
        if (this.wordCount < 0) {
          this.wordCount = 0
        }
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
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(async () => {
            if (this.hasUnsavedChanges) {
              const ok = await this.saveChapter()
              if (ok) { this.navPathStack.pop() }
            } else {
              this.navPathStack.pop()
            }
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
        SujianEditor({
          snapshot: this.editorSnapshot,
          onSelectionTap: (offset: number): void => { this.selectionController.onTap(offset) },
          harmonyImeConnection: this.harmonyImeConnection,
          inputAdapter: this.inputAdapter,
          selectionController: this.selectionController,
          // Issue #629 评论 5 第 8 节：滚动时把 viewport 状态回调给 WritingScreen，
          // 持续写入接续状态；接续恢复时由 editor.restoreViewport 消费。
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
}
