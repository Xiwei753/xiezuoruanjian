import { EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { utf16ToUtf8, utf8ToUtf16 } from '../input/text_offset_mapper'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { CorrectionIgnoreScope, ICorrectionBridge } from './ICorrectionBridge'
import {
  CorrectionChapterSnapshotDto,
  CorrectionModelPathStatusDto,
  CorrectionResourceStatusDto,
  CorrectionSuggestionDto,
  MappedCorrectionSuggestion
} from './CorrectionDtos'

// Issue #1054：校对资源/模型的真实生命周期状态，界面必须能区分。
export type CorrectionResourceState = 'uninitialized' | 'preparing' | 'ready' | 'rulesOnly' | 'error'

export interface CorrectionPresentationState {
  suggestions: MappedCorrectionSuggestion[]
  isChecking: boolean
  errorMessage: string
  resourceState: CorrectionResourceState
  resourceMessage: string
  checkMessage: string
}

export type CorrectionStateListener = (state: CorrectionPresentationState) => void
export type CorrectionScanCompletionListener = () => void

interface CorrectionRequestSnapshot {
  projectId: string
  chapterId: string
  sessionId: number
  text: string
  revision: number
}

/**
 * Coordinates background correction requests and projects Core byte ranges to ArkTS UTF-16.
 * It owns request identity only; all correction rules and edit transactions stay in Core.
 */
export class CorrectionCoordinator {
  private bridge: ICorrectionBridge
  private editor: EditorSessionCoordinator
  private listener: CorrectionStateListener | null = null
  private scanCompletionListener: CorrectionScanCompletionListener | null = null
  private current: CorrectionRequestSnapshot | null = null
  private suggestions: MappedCorrectionSuggestion[] = []
  private isChecking: boolean = false
  private errorMessage: string = ''
  // Issue #1054：资源状态与最近一次“校对本章”的结果。
  private resourceState: CorrectionResourceState = 'uninitialized'
  private resourceMessage: string = '校对资源尚未初始化'
  private checkMessage: string = ''
  private requestGeneration: number = 0
  private pendingScanTimer: number = -1
  private pendingScanSnapshot: CorrectionRequestSnapshot | null = null
  private pendingScanGeneration: number = 0
  /** Stage-one edit mappings run one at a time in transaction order. */
  private applyChain: Promise<void> = Promise.resolve()
  /** Cleared on chapter/session reset so old mappings cannot target a new document. */
  private applyEpoch: number = 0
  // Issue #958：内置模型兜底由会话打开回调触发；只有确认已有用户模型或内置模型
  // 成功交给 Core 后，本实例才不再重复尝试（失败保持可重试，不阻断编辑）。
  // Issue #1054：同一会话单飞 Promise + 明确状态机，避免并发重复准备/重复切换。
  private bundledModelAttempted: boolean = false
  private bundledModelPromise: Promise<ResultEnvelope<boolean>> | null = null

  constructor(bridge: ICorrectionBridge, editor: EditorSessionCoordinator) {
    this.bridge = bridge
    this.editor = editor
  }

  setStateListener(listener: CorrectionStateListener | null): void {
    this.listener = listener
    this.notify()
  }

  setScanCompletionListener(listener: CorrectionScanCompletionListener | null): void {
    this.scanCompletionListener = listener
  }

  private notifyScanCompleted(): void {
    this.scanCompletionListener?.()
  }

  private notify(): void {
    if (this.listener === null) {
      return
    }
    const state: CorrectionPresentationState = {
      suggestions: this.suggestions,
      isChecking: this.isChecking,
      errorMessage: this.errorMessage,
      resourceState: this.resourceState,
      resourceMessage: this.resourceMessage,
      checkMessage: this.checkMessage
    }
    this.listener(state)
  }

  /**
   * Called after each Core editor snapshot. The edit transaction's real
   * `displayPatches` are forwarded to Core; cursor movement only restores
   * cached results and never implies an edit.
   */
  acceptSnapshot(projectId: string, chapterId: string, snapshot: EditorSessionSnapshot,
    editResult: EditorEditResult | null = null): void {
    const sessionId = this.editor.getSessionId()
    if (sessionId === null || projectId.length === 0 || chapterId.length === 0) {
      this.applyEpoch++
      this.invalidate()
      return
    }

    if (snapshot.composition !== undefined) {
      this.current = null
      this.applyEpoch++
      this.invalidate()
      return
    }

    const previous = this.current
    if (previous !== null && (previous.projectId !== projectId || previous.chapterId !== chapterId ||
        previous.sessionId !== sessionId)) {
      // A reopened chapter must not receive queued mappings from the old one.
      this.applyEpoch++
    }
    const nextSnapshot: CorrectionRequestSnapshot = {
      projectId: projectId,
      chapterId: chapterId,
      sessionId: sessionId,
      text: snapshot.text,
      revision: snapshot.revision
    }
    if (previous !== null && previous.projectId === projectId && previous.chapterId === chapterId &&
        previous.sessionId === sessionId && previous.revision === snapshot.revision &&
        previous.text === snapshot.text) {
      this.current = nextSnapshot
      return
    }

    this.current = nextSnapshot
    this.requestGeneration++
    const generation = this.requestGeneration
    this.errorMessage = ''
    this.notify()

    if (editResult !== null && editResult.displayPatches.length > 0 &&
        editResult.newRevision === snapshot.revision &&
        editResult.baseRevision <= snapshot.revision) {
      this.isChecking = true
      this.notify()
      this.enqueueApplyEdits(generation, nextSnapshot, editResult)
    } else {
      this.isChecking = false
      this.notify()
      void this.restoreChapterSnapshot(generation)
    }
  }

  private invalidate(): void {
    this.requestGeneration++
    if (this.pendingScanTimer >= 0) {
      clearTimeout(this.pendingScanTimer)
      this.pendingScanTimer = -1
    }
    this.pendingScanSnapshot = null
    this.suggestions = []
    this.isChecking = false
    this.errorMessage = ''
    this.checkMessage = ''
    this.notify()
  }

  clear(): void {
    this.current = null
    this.applyEpoch++
    this.invalidate()
  }

  /** Forward a user-selected local model file to Core; no model path is assumed by the platform. */
  async setModelPath(path: string): Promise<ResultEnvelope<boolean>> {
    return this.bridge.setModelPath(path)
  }

  async getResourceStatus(): Promise<ResultEnvelope<CorrectionResourceStatusDto>> {
    return this.bridge.getResourceStatus()
  }

  async getModelPathStatus(path: string): Promise<ResultEnvelope<CorrectionModelPathStatusDto>> {
    return this.bridge.getModelPathStatus(path)
  }

  // ── 资源状态（Issue #1054） ─────────────────────────────────────

  // 主动读取 Core 资源状态：面板打开、模型准备完成、失败后都可刷新。
  async refreshResourceState(): Promise<void> {
    try {
      const status = await this.bridge.getResourceStatus()
      if (!status.success || status.data === undefined) {
        this.resourceState = 'error'
        this.resourceMessage = `无法读取校对资源：${status.userMessage ?? status.errorCode ?? '未知错误'}`
        this.notify()
        return
      }
      this.applyResourceStatus(status.data)
    } catch (e) {
      this.resourceState = 'error'
      this.resourceMessage = `读取校对资源失败：${String(e)}`
      this.notify()
    }
  }

  private applyResourceStatus(status: CorrectionResourceStatusDto): void {
    const modelUsable: boolean = status.modelConfigured && status.modelFileAvailable &&
      !status.modelMigrationRequired
    if (modelUsable) {
      this.resourceState = 'ready'
      this.resourceMessage = 'AI 校对模型可用'
    } else if (status.bundledLexiconAvailable) {
      this.resourceState = 'rulesOnly'
      this.resourceMessage = status.modelMigrationRequired
        ? '旧模型格式待迁移，当前仅使用内置词典/固定规则校对'
        : '当前仅使用内置词典/固定规则校对'
    } else {
      this.resourceState = 'error'
      this.resourceMessage = '未找到可用的校对模型或词典资源'
    }
    this.notify()
  }

  /**
   * Issue #985 / #1054：内置 MacBERT4CSC ONNX 模型兜底。
   * - 同一会话单飞：并发调用复用同一个 Promise，状态为 preparing；
   * - Core 已配置有效用户模型时直接跳过，用户模型优先；
   * - 未配置时由调用方准备内置模型文件，再走既有 setModelPath 交给 Core；
   * - 成功标记 attempted；失败进入 error 状态并保留可重试，不阻断编辑。
   */
  async ensureBundledModel(prepare: () => Promise<string>): Promise<ResultEnvelope<boolean>> {
    if (this.bundledModelAttempted) {
      return { success: true, data: false }
    }
    const inFlight = this.bundledModelPromise
    if (inFlight !== null) {
      return inFlight
    }
    const task: Promise<ResultEnvelope<boolean>> = this.prepareBundledModel(prepare)
    this.bundledModelPromise = task
    try {
      return await task
    } finally {
      if (this.bundledModelPromise === task) {
        this.bundledModelPromise = null
      }
    }
  }

  private async prepareBundledModel(prepare: () => Promise<string>): Promise<ResultEnvelope<boolean>> {
    this.resourceState = 'preparing'
    this.resourceMessage = '正在准备内置校对模型…'
    this.notify()
    const status = await this.bridge.getResourceStatus()
    if (!status.success || status.data === undefined) {
      this.resourceState = 'error'
      this.resourceMessage = `无法读取校对资源：${status.userMessage ?? status.errorCode ?? '未知错误'}`
      this.notify()
      return {
        success: false,
        errorCode: status.errorCode ?? 'RESOURCE_STATUS_UNAVAILABLE',
        userMessage: this.resourceMessage
      }
    }
    const modelUsable: boolean = status.data.modelConfigured && status.data.modelFileAvailable &&
      !status.data.modelMigrationRequired
    if (modelUsable) {
      this.bundledModelAttempted = true
      this.applyResourceStatus(status.data)
      return { success: true, data: false }
    }
    let path = ''
    try {
      path = await prepare()
    } catch (e) {
      this.resourceState = 'error'
      this.resourceMessage = `内置模型准备失败：${String(e)}`
      this.notify()
      return { success: false, errorCode: 'BUNDLED_MODEL_PREPARE_FAILED', userMessage: this.resourceMessage }
    }
    if (path.length === 0) {
      this.resourceState = 'error'
      this.resourceMessage = '内置模型路径为空'
      this.notify()
      return { success: false, errorCode: 'BUNDLED_MODEL_PREPARE_FAILED', userMessage: this.resourceMessage }
    }
    const result = await this.bridge.setModelPath(path)
    if (result.success) {
      this.bundledModelAttempted = true
      this.resourceState = 'ready'
      this.resourceMessage = '内置 AI 校对模型已就绪'
      this.notify()
    } else {
      this.resourceState = 'error'
      this.resourceMessage = `校对模型加载失败：${result.userMessage ?? result.errorCode ?? '未知错误'}`
      this.notify()
    }
    return result
  }

  private currentRequest(generation: number): CorrectionRequestSnapshot | null {
    if (generation !== this.requestGeneration || this.current === null) {
      return null
    }
    const editorSnapshot = this.editor.getSnapshot()
    if (editorSnapshot === null || this.editor.getSessionId() !== this.current.sessionId ||
        editorSnapshot.revision !== this.current.revision || editorSnapshot.text !== this.current.text) {
      return null
    }
    return this.current
  }

  /**
   * Issue #954: read Core's cached, still-valid marks for the current revision.
   * This never runs the model; chapter open and plain edits restore instantly.
   */
  private async restoreChapterSnapshot(generation: number): Promise<void> {
    const snapshot = this.currentRequest(generation)
    if (snapshot === null) {
      return
    }
    const result = await this.bridge.getChapterSnapshot(
      snapshot.sessionId, snapshot.projectId, snapshot.chapterId, snapshot.revision)
    this.acceptCheckResult(generation, snapshot, result, true)
  }

  private enqueueApplyEdits(generation: number, snapshot: CorrectionRequestSnapshot,
    editResult: EditorEditResult): void {
    const epoch = this.applyEpoch
    this.applyChain = this.applyChain
      .then(async (): Promise<void> => {
        if (epoch !== this.applyEpoch) {
          return
        }
        await this.applyEdits(generation, snapshot, editResult)
      })
      .catch((): void => {})
  }

  private async applyEdits(generation: number, snapshot: CorrectionRequestSnapshot,
    editResult: EditorEditResult): Promise<void> {
    const epoch = this.applyEpoch
    const ranges: number[][] = []
    for (let i = 0; i < editResult.displayPatches.length; i++) {
      const patch = editResult.displayPatches[i]
      if (patch.newRevision !== snapshot.revision) {
        continue
      }
      ranges.push([
        patch.replaceByteStart,
        patch.replaceByteEndExclusive,
        patch.insertedByteStart,
        patch.insertedByteEndExclusive
      ])
    }
    if (ranges.length === 0) {
      this.isChecking = false
      this.notify()
      void this.restoreChapterSnapshot(generation)
      return
    }
    const result = await this.bridge.applyEdits(
      snapshot.sessionId, snapshot.projectId, snapshot.chapterId, snapshot.revision,
      editResult.baseRevision, JSON.stringify(ranges))
    if (epoch !== this.applyEpoch) {
      return
    }
    if (!result.success || generation !== this.requestGeneration) {
      if (!result.success && generation === this.requestGeneration) {
        this.isChecking = false
        this.errorMessage = result.userMessage ?? result.errorCode ?? '校对暂不可用'
        this.notify()
      }
      return
    }
    this.pendingScanSnapshot = snapshot
    this.pendingScanGeneration = generation
    if (this.pendingScanTimer >= 0) {
      clearTimeout(this.pendingScanTimer)
    }
    this.pendingScanTimer = setTimeout(() => {
      this.pendingScanTimer = -1
      const pending = this.pendingScanSnapshot
      if (pending === null || this.pendingScanGeneration !== this.requestGeneration) {
        return
      }
      this.pendingScanSnapshot = null
      void this.scanPending(this.pendingScanGeneration, pending)
    }, 250)
  }

  private async scanPending(generation: number, snapshot: CorrectionRequestSnapshot): Promise<void> {
    try {
      const result = await this.bridge.scanPending(
        snapshot.sessionId, snapshot.projectId, snapshot.chapterId,
        snapshot.revision, snapshot.text)
      this.acceptCheckResult(generation, snapshot, result, false)
    } finally {
      this.notifyScanCompleted()
    }
  }

  /**
   * Issue #1054 复审：页面在开始完整输入屏障（原生队列 + Core 队列）前先占用一次
   * “校对本章”操作。同步置 isChecking，避免屏障期间重复点击启动第二次校对。
   */
  beginCheckRequest(): boolean {
    if (this.isChecking) {
      return false
    }
    this.isChecking = true
    this.errorMessage = ''
    this.checkMessage = ''
    if (this.pendingScanTimer >= 0) {
      clearTimeout(this.pendingScanTimer)
      this.pendingScanTimer = -1
    }
    this.pendingScanSnapshot = null
    // 先作废旧扫描；真正的 revision 在完整屏障结束后再冻结。
    this.requestGeneration++
    this.notify()
    return true
  }

  // 页面侧完整输入屏障失败（IME 未结算、原生/镜像/Core 未对齐、章节已切换）：
  // 用同一条校对状态通道显示原因，不能让面板继续展示旧建议或误报“未发现问题”。
  reportCheckBarrierFailure(message: string): void {
    this.isChecking = false
    this.suggestions = []
    this.errorMessage = message
    this.checkMessage = ''
    this.notify()
    this.notifyScanCompleted()
  }

  private resourceLabel(): string {
    switch (this.resourceState) {
      case 'ready':
        return 'AI 校对'
      case 'rulesOnly':
        return '仅规则/词典'
      case 'preparing':
        return '模型准备中，本次仅规则/词典'
      default:
        return ''
    }
  }

  /**
   * 完整输入屏障之后的校对入口：调用方已 await bridge.finishEditingAndWhenIdle()
   * 与 editor.whenIdle()，并固定了 project/chapter/session。这里只做：
   * 等待进行中的模型准备结束 → 重读真实资源状态 → 冻结当前 revision → Core 校对 →
   * 仅投影同一 session/revision 的结果。报错全部走 errorMessage/checkMessage。
   */
  async checkChapterAfterInputBarrier(expectedProjectId: string, expectedChapterId: string,
    expectedSessionId: number): Promise<void> {
    if (!this.isChecking) {
      if (!this.beginCheckRequest()) {
        return
      }
    }
    try {
      const pendingModel = this.bundledModelPromise
      if (pendingModel !== null) {
        // 等待准备结束；失败原因由 prepareBundledModel 写入资源状态，随后重读。
        await pendingModel.then((): void => {}, (): void => {})
      }
      await this.refreshResourceState()
      const sessionId = this.editor.getSessionId()
      const snap = this.editor.getSnapshot()
      const baseRequest = this.current
      if (sessionId === null || snap === null || baseRequest === null) {
        this.errorMessage = '章节会话已关闭，校对未执行'
        this.checkMessage = ''
        return
      }
      if (sessionId !== expectedSessionId || baseRequest.projectId !== expectedProjectId ||
          baseRequest.chapterId !== expectedChapterId || expectedProjectId.length === 0 ||
          expectedChapterId.length === 0) {
        this.errorMessage = '章节会话已切换，请重新打开校对面板后再试'
        this.checkMessage = ''
        return
      }
      const request: CorrectionRequestSnapshot = {
        projectId: expectedProjectId,
        chapterId: expectedChapterId,
        sessionId: sessionId,
        text: snap.text,
        revision: snap.revision
      }
      this.current = request
      this.requestGeneration++
      const generation = this.requestGeneration
      const result = await this.bridge.checkChapter(
        request.sessionId, request.projectId, request.chapterId, request.revision)
      this.acceptCheckResult(generation, request, result, false)
    } catch (e) {
      this.errorMessage = `校对请求失败：${String(e)}`
      this.checkMessage = ''
    } finally {
      this.isChecking = false
      this.notify()
      this.notifyScanCompleted()
    }
  }

  /**
   * 兼容入口：调用方没有做原生输入屏障时，至少等 Core 命令队列空闲后复用上面的
   * 冻结/投影逻辑。面板按钮走 WritingScreen.checkCurrentChapter() 的完整屏障入口。
   */
  async checkChapter(): Promise<void> {
    if (!this.beginCheckRequest()) {
      return
    }
    const baseRequest = this.current
    const initialSessionId = this.editor.getSessionId()
    if (baseRequest === null || initialSessionId === null) {
      this.reportCheckBarrierFailure('当前没有打开的章节会话，无法校对本章')
      return
    }
    try {
      await this.editor.whenIdle()
    } catch (e) {
      this.reportCheckBarrierFailure(`校对前输入结算失败：${String(e)}`)
      return
    }
    if (this.editor.getSessionId() !== initialSessionId) {
      this.reportCheckBarrierFailure('章节会话已切换，请重新打开校对面板后再试')
      return
    }
    await this.checkChapterAfterInputBarrier(
      baseRequest.projectId, baseRequest.chapterId, initialSessionId)
  }

  // Issue #1056：Core 会话身份错误（target_id 与 project/chapter 不匹配）表示编辑会话与章节错位，
  // 不是模型或资源不可用。其它失败仍按真实 errorCode/userMessage 呈现。
  private describeCheckFailure(result: ResultEnvelope<CorrectionChapterSnapshotDto>): string {
    const errorCode: string = result.errorCode ?? ''
    const userMessage: string = result.userMessage ?? ''
    if (errorCode === 'CORRECTION_ERROR' &&
        userMessage.indexOf('does not match the active chapter session') >= 0) {
      return '当前编辑会话与章节不一致，请重新打开章节后再试'
    }
    if (userMessage.length > 0) {
      return userMessage
    }
    return errorCode.length > 0 ? errorCode : '未知错误'
  }

  private acceptCheckResult(generation: number, snapshot: CorrectionRequestSnapshot,
    result: ResultEnvelope<CorrectionChapterSnapshotDto>, silent: boolean): void {
    if (generation !== this.requestGeneration) {
      return
    }
    const current = this.currentRequest(generation)
    if (current === null) {
      if (!silent) {
        // 校对返回时正文/会话已推进：结果不能投影到新 revision，给出可见原因。
        this.invalidate()
        this.errorMessage = '章节内容或会话已变化，本次校对结果已丢弃，请重新校对'
        this.notify()
      }
      return
    }
    if (current.revision !== snapshot.revision || current.chapterId !== snapshot.chapterId ||
        current.projectId !== snapshot.projectId || current.sessionId !== snapshot.sessionId) {
      return
    }
    const data = result.data
    if (!result.success || data === undefined || data.textRevision !== snapshot.revision) {
      if (silent) {
        return
      }
      this.isChecking = false
      this.suggestions = []
      this.checkMessage = ''
      this.errorMessage = `校对失败：${this.describeCheckFailure(result)}`
      this.notify()
      return
    }
    if (!silent) {
      this.isChecking = false
    }
    const mapped: MappedCorrectionSuggestion[] = []
    for (let i = 0; i < data.suggestions.length; i++) {
      const suggestion = this.mapSuggestion(snapshot.text, snapshot.revision, data.suggestions[i])
      if (suggestion !== null) {
        mapped.push(suggestion)
      }
    }
    // Core already merged cached and freshly scanned segments; replace wholesale.
    this.suggestions = mapped
    this.errorMessage = ''
    if (!silent) {
      const baseMessage = mapped.length > 0
        ? `检查完成：发现 ${mapped.length} 条建议`
        : '检查完成：未发现问题'
      // 资源标签必须来自刚重读的 Core 状态：模型准备失败但固定规则可用时，
      // 不能把“规则未发现”展示成 AI 校对完成。
      const label = this.resourceLabel()
      this.checkMessage = label.length > 0 ? `[${label}] ${baseMessage}` : baseMessage
    }
    this.notify()
  }

  private mapSuggestion(text: string, revision: number,
    suggestion: CorrectionSuggestionDto): MappedCorrectionSuggestion | null {
    if (suggestion.textRevision !== revision || !Number.isInteger(suggestion.startByte) ||
        !Number.isInteger(suggestion.endByte) || suggestion.startByte < 0 ||
        suggestion.endByte <= suggestion.startByte) {
      return null
    }
    const startUtf16 = utf8ToUtf16(text, suggestion.startByte)
    const endUtf16 = utf8ToUtf16(text, suggestion.endByte)
    if (utf16ToUtf8(text, startUtf16) !== suggestion.startByte ||
        utf16ToUtf8(text, endUtf16) !== suggestion.endByte ||
        text.substring(startUtf16, endUtf16) !== suggestion.original) {
      return null
    }
    return {
      startByte: suggestion.startByte,
      endByte: suggestion.endByte,
      original: suggestion.original,
      replacement: suggestion.replacement,
      source: suggestion.source,
      ruleId: suggestion.ruleId,
      severity: suggestion.severity,
      textRevision: suggestion.textRevision,
      startUtf16: startUtf16,
      endUtf16: endUtf16
    }
  }

  async apply(suggestion: MappedCorrectionSuggestion): Promise<ResultEnvelope<EditorEditResult>> {
    const request = this.current
    if (request === null) {
      return { success: false, errorCode: 'NO_SESSION' }
    }
    if (this.currentRequest(this.requestGeneration) === null ||
        request.revision !== suggestion.textRevision) {
      this.invalidate()
      return { success: false, errorCode: 'STALE_REVISION' }
    }
    const generation = this.requestGeneration
    const result = await this.editor.enqueueExternalEdit((sessionId: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> => {
      if (sessionId !== request.sessionId || expectedRevision !== suggestion.textRevision) {
        const stale: ResultEnvelope<EditorEditResult> = {
          success: false,
          errorCode: 'STALE_REVISION'
        }
        return Promise.resolve(stale)
      }
      return this.bridge.apply(sessionId, request.projectId, request.chapterId,
        suggestion.textRevision, suggestion.startByte, suggestion.endByte,
        suggestion.original, suggestion.replacement)
    })
    if (!result.success && generation === this.requestGeneration &&
        this.currentRequest(generation) === null) {
      this.invalidate()
    }
    return result
  }

  async ignore(suggestion: MappedCorrectionSuggestion, scope: CorrectionIgnoreScope): Promise<ResultEnvelope<boolean>> {
    const request = this.current
    if (request === null) {
      return { success: false, errorCode: 'NO_SESSION' }
    }
    if (request.revision !== suggestion.textRevision) {
      this.invalidate()
      return { success: false, errorCode: 'STALE_REVISION' }
    }
    const latest = this.currentRequest(this.requestGeneration)
    if (latest === null || latest.revision !== suggestion.textRevision) {
      this.invalidate()
      return { success: false, errorCode: 'STALE_REVISION' }
    }
    const result = await this.bridge.ignore(request.projectId, request.chapterId,
      suggestion.ruleId, suggestion.original, scope, suggestion.textRevision,
      suggestion.startByte, suggestion.endByte)
    if (this.currentRequest(this.requestGeneration) === null) {
      this.invalidate()
      return result
    }
    if (result.success && result.data === true && this.current === request) {
      this.removeSuggestion(suggestion)
    }
    return result
  }

  async addToDictionary(suggestion: MappedCorrectionSuggestion): Promise<ResultEnvelope<boolean>> {
    const request = this.current
    if (request === null) {
      return { success: false, errorCode: 'NO_SESSION' }
    }
    if (request.revision !== suggestion.textRevision) {
      this.invalidate()
      return { success: false, errorCode: 'STALE_REVISION' }
    }
    if (this.currentRequest(this.requestGeneration) === null) {
      this.invalidate()
      return { success: false, errorCode: 'STALE_REVISION' }
    }
    const result = await this.bridge.addDictionaryEntry(request.projectId, suggestion.original)
    if (this.currentRequest(this.requestGeneration) === null) {
      this.invalidate()
      return result
    }
    if (result.success && result.data === true && this.current === request) {
      this.removeSuggestion(suggestion)
    }
    return result
  }

  private removeSuggestion(suggestion: MappedCorrectionSuggestion): void {
    const remaining: MappedCorrectionSuggestion[] = []
    for (let i = 0; i < this.suggestions.length; i++) {
      const current = this.suggestions[i]
      if (current.startByte !== suggestion.startByte || current.endByte !== suggestion.endByte ||
          current.ruleId !== suggestion.ruleId) {
        remaining.push(current)
      }
    }
    this.suggestions = remaining
    this.notify()
  }

  dispose(): void {
    this.clear()
    this.listener = null
    this.scanCompletionListener = null
  }
}
