import { EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { utf16ToUtf8, utf8ToUtf16 } from '../input/text_offset_mapper'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { CorrectionIgnoreScope, ICorrectionBridge } from './ICorrectionBridge'
import {
  CorrectionChapterSnapshotDto,
  CorrectionCheckedRangeDto,
  CorrectionModelPathStatusDto,
  CorrectionResourceStatusDto,
  CorrectionSuggestionDto,
  MappedCorrectionSuggestion
} from './CorrectionDtos'

// Issue #1054：校对资源/模型的真实生命周期状态，界面必须能区分。
export type CorrectionResourceState = 'uninitialized' | 'preparing' | 'ready' | 'rulesOnly' | 'error'

// Issue #1064：前台活动章节的周期增量扫描间隔。连续输入不重置，首轮在开章后 60s。
const CHAPTER_SCAN_INTERVAL_MS: number = 60000

/** Where a chapter check came from; restore-only never runs the model. */
type CorrectionCheckOrigin = 'restore' | 'periodic' | 'manual'

export interface CorrectionPresentationState {
  suggestions: MappedCorrectionSuggestion[]
  isChecking: boolean
  errorMessage: string
  resourceState: CorrectionResourceState
  resourceMessage: string
  checkMessage: string
  // Issue #1064：面板显示已校对/待校对进度，数据来自 Core 的 checkedRanges。
  checkedBytes: number
  totalBytes: number
  pendingBytes: number
  hasUncheckedSegments: boolean
  progressKnown: boolean
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
  // Issue #1064：前台活动章节的单周期调度。编辑只标脏，不再启动/重置扫描定时器。
  private chapterScanTimer: number = -1
  private foreground: boolean = false
  private scanInFlight: boolean = false
  private pendingScanFlag: boolean = false
  private dirtySinceLastScan: boolean = false
  private hasUncheckedSegments: boolean = false
  /** Core 缓存是否已给出当前章进度；未知期间视为有待检查内容，避免误报“已校对完成”。 */
  private progressKnown: boolean = false
  private checkedBytes: number = 0
  private totalBytes: number = 0
  private pendingBytes: number = 0
  private lastResourceSignature: string = ''
  private modelPreparation: (() => Promise<void>) | null = null
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
      checkMessage: this.checkMessage,
      checkedBytes: this.checkedBytes,
      totalBytes: this.totalBytes,
      pendingBytes: this.pendingBytes,
      hasUncheckedSegments: this.hasUncheckedSegments,
      progressKnown: this.progressKnown
    }
    this.listener(state)
  }

  /**
   * Issue #1064：模型准备延迟到真正需要扫描时。冷启动/开章若缓存全部有效，
   * 恢复路径不得初始化 ONNX 模型。
   */
  setModelPreparationHandler(handler: (() => Promise<void>) | null): void {
    this.modelPreparation = handler
  }

  /** Page became foreground/visible again: restore cache only, then restart the 60s cycle. */
  onPageForeground(): void {
    this.foreground = true
    if (this.current !== null) {
      void this.refreshResourceState()
      this.restoreCachedResults()
    }
    this.startChapterScanTimer()
  }

  /** Page left foreground/visibility: stop this chapter's timer and keep pending dirty state. */
  onPageBackground(): void {
    this.foreground = false
    this.stopChapterScanTimer()
  }

  /** Sync/reopen/foreground refresh: read Core's cache, never start a model scan. */
  restoreCachedResults(): void {
    if (this.current === null) {
      return
    }
    void this.restoreChapterSnapshot(this.requestGeneration)
  }

  private startChapterScanTimer(): void {
    if (!this.foreground || this.current === null || this.chapterScanTimer >= 0) {
      return
    }
    this.chapterScanTimer = setInterval((): void => {
      this.onChapterScanTick()
    }, CHAPTER_SCAN_INTERVAL_MS)
  }

  private stopChapterScanTimer(): void {
    if (this.chapterScanTimer >= 0) {
      clearInterval(this.chapterScanTimer)
      this.chapterScanTimer = -1
    }
  }

  /**
   * 60s tick：只有前台活动章节、存在待检查区间且当前没有扫描时才提交增量扫描；
   * 扫描进行中只置待办标志，不并行启动第二次模型扫描。
   */
  private onChapterScanTick(): void {
    if (!this.foreground || this.current === null) {
      this.stopChapterScanTimer()
      return
    }
    if (this.scanInFlight) {
      this.pendingScanFlag = true
      return
    }
    if (this.isChecking || !this.hasPendingCheck()) {
      return
    }
    void this.requestIncrementalScanFromCurrent('periodic')
  }

  private async requestIncrementalScanFromCurrent(origin: 'periodic' | 'manual'): Promise<void> {
    const snapshot = this.current
    const sessionId = this.editor.getSessionId()
    const editorSnapshot = this.editor.getSnapshot()
    if (snapshot === null || sessionId === null || editorSnapshot === null ||
        snapshot.sessionId !== sessionId || snapshot.revision !== editorSnapshot.revision ||
        snapshot.text !== editorSnapshot.text) {
      return
    }
    await this.performIncrementalScan(origin, snapshot, this.requestGeneration)
  }

  private hasPendingCheck(): boolean {
    return !this.progressKnown || this.dirtySinceLastScan || this.hasUncheckedSegments ||
      this.pendingBytes > 0
  }

  private markCorrectionDirty(): void {
    this.dirtySinceLastScan = true
    this.hasUncheckedSegments = true
    this.notify()
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
      this.stopChapterScanTimer()
      this.invalidate()
      return
    }

    if (snapshot.composition !== undefined) {
      this.current = null
      this.applyEpoch++
      this.stopChapterScanTimer()
      this.invalidate()
      return
    }

    const previous = this.current
    const chapterChanged: boolean = previous === null || previous.projectId !== projectId ||
      previous.chapterId !== chapterId || previous.sessionId !== sessionId
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
    if (chapterChanged) {
      // 开章：只恢复缓存；60s 后首轮周期检查。旧章节尚未完成的脏区保留在 Core。
      this.stopChapterScanTimer()
      this.pendingScanFlag = false
      this.dirtySinceLastScan = false
      this.hasUncheckedSegments = false
      this.progressKnown = false
      this.checkedBytes = 0
      this.totalBytes = 0
      this.pendingBytes = 0
    }
    this.notify()
    this.startChapterScanTimer()

    if (editResult !== null && editResult.displayPatches.length > 0 &&
        editResult.newRevision === snapshot.revision &&
        editResult.baseRevision <= snapshot.revision) {
      // 编辑事务只做轻量段落映射和脏区标记，不启动扫描，也不重置周期。
      this.enqueueApplyEdits(generation, nextSnapshot, editResult)
    } else {
      if (!this.scanInFlight) {
        this.isChecking = false
      }
      this.notify()
      void this.restoreChapterSnapshot(generation)
    }
  }

  private invalidate(): void {
    this.requestGeneration++
    this.pendingScanFlag = false
    this.dirtySinceLastScan = false
    this.hasUncheckedSegments = false
    this.progressKnown = false
    this.checkedBytes = 0
    this.totalBytes = 0
    this.pendingBytes = 0
    this.suggestions = []
    this.isChecking = false
    this.errorMessage = ''
    this.checkMessage = ''
    this.notify()
  }

  clear(): void {
    this.current = null
    this.applyEpoch++
    this.stopChapterScanTimer()
    this.invalidate()
  }

  /** Forward a user-selected local model file to Core; no model path is assumed by the platform. */
  async setModelPath(path: string): Promise<ResultEnvelope<boolean>> {
    const result = await this.bridge.setModelPath(path)
    if (result.success) {
      // 模型身份变化只标脏，等周期或手动请求，不立即扫描。
      this.markCorrectionDirty()
    }
    return result
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
    // 词典/规则文件在设置页变化时没有页面内事件；这里比较 Core 状态签名，
    // 变化只标脏，等周期或手动检查。
    const signature: string = [
      status.modelPath ?? '',
      String(status.modelConfigured),
      String(status.modelFileAvailable),
      String(status.modelMigrationRequired),
      String(status.bundledLexiconAvailable),
      status.installedDictionaryFiles.join(','),
      status.missingDictionaryFiles.join(',')
    ].join('|')
    if (this.lastResourceSignature.length > 0 && signature !== this.lastResourceSignature) {
      this.dirtySinceLastScan = true
      this.hasUncheckedSegments = true
    }
    this.lastResourceSignature = signature
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
      // 模型身份已变化，本轮扫描必须让 Core 按新 scanner fingerprint 重新校验。
      this.dirtySinceLastScan = true
      this.hasUncheckedSegments = true
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
    this.acceptCheckResult(generation, snapshot, result, 'restore')
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
      if (!this.scanInFlight) {
        this.isChecking = false
      }
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
        if (!this.scanInFlight) {
          this.isChecking = false
        }
        this.errorMessage = result.userMessage ?? result.errorCode ?? '校对暂不可用'
        this.notify()
      }
      return
    }
    // 轻量事务已完成段落位置映射；只标脏，扫描交给前台 60s 周期或手动请求。
    this.dirtySinceLastScan = true
    this.hasUncheckedSegments = true
    this.notify()
  }

  /**
   * 唯一的增量扫描入口（复用 Core 的 scanPending 单飞队列）。调用方必须提供
   * 当前已结算正文的冻结快照；扫描期间到达的周期/手动请求只置 pending 标志，
   * 结束后补跑一次，不并行运行第二个模型扫描。
   */
  private async performIncrementalScan(origin: 'periodic' | 'manual',
    snapshot: CorrectionRequestSnapshot, generation: number): Promise<void> {
    if (this.scanInFlight) {
      this.pendingScanFlag = true
      return
    }
    this.scanInFlight = true
    this.isChecking = true
    this.errorMessage = ''
    this.notify()
    try {
      // Issue #1064 复审 #7196305554：先用 restore-only 快照判断是否真有未检查段，
      // 已由同步/本地缓存完整覆盖时直接收敛，不初始化 ONNX 模型。
      const preflight = await this.bridge.getChapterSnapshot(
        snapshot.sessionId, snapshot.projectId, snapshot.chapterId, snapshot.revision)
      if (preflight.success && preflight.data !== undefined &&
          !preflight.data.hasUncheckedSegments) {
        this.acceptCheckResult(generation, snapshot, preflight, 'restore')
        return
      }
      await this.prepareModelForScan()
      if (this.currentRequest(generation) === null) {
        return
      }
      const result = await this.bridge.scanPending(
        snapshot.sessionId, snapshot.projectId, snapshot.chapterId,
        snapshot.revision, snapshot.text)
      this.acceptCheckResult(generation, snapshot, result, origin)
    } catch (e) {
      if (origin === 'manual') {
        this.errorMessage = `校对请求失败：${String(e)}`
        this.checkMessage = ''
      } else {
        this.checkMessage = `自动检查失败：${String(e)}`
      }
      this.notify()
    } finally {
      this.scanInFlight = false
      this.isChecking = false
      this.notify()
      this.notifyScanCompleted()
      const rerun: boolean = this.pendingScanFlag
      this.pendingScanFlag = false
      if (rerun && this.foreground && this.current !== null) {
        void this.requestIncrementalScanFromCurrent(origin)
      }
    }
  }

  /** 只有真正需要扫描时才准备内置模型；缓存全部有效时冷启动不加载模型。 */
  private async prepareModelForScan(): Promise<void> {
    const prepare = this.modelPreparation
    if (prepare === null) {
      return
    }
    try {
      await prepare()
    } catch (e) {
      // 模型准备失败不阻断固定规则/词典扫描；资源状态由准备流程写入。
      this.checkMessage = `模型准备失败，使用固定规则/词典：${String(e)}`
    }
  }

  /**
   * Issue #1054 复审：页面在开始完整输入屏障（原生队列 + Core 队列）前先占用一次
   * “校对本章”操作。Issue #1064：扫描进行中点击手动校对只合并待办，当前扫描结束后
   * 补跑一次；绝不再启动第二个模型扫描。
   */
  beginCheckRequest(): boolean {
    if (this.scanInFlight) {
      this.pendingScanFlag = true
      return false
    }
    if (this.isChecking) {
      return false
    }
    this.isChecking = true
    this.errorMessage = ''
    this.checkMessage = ''
    this.pendingScanFlag = false
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
   * 完整输入屏障之后的手动校对入口：调用方已 await bridge.finishEditingAndWhenIdle()
   * 与 editor.whenIdle()，并固定了 project/chapter/session。Issue #1064：手动的
   * “校对本章”立即提交当前已结算正文的增量检查（Core scanPending，单飞），
   * 默认只补查剩余未校对区间；不会启动整章 All 扫描。
   */
  async checkChapterAfterInputBarrier(expectedProjectId: string, expectedChapterId: string,
    expectedSessionId: number): Promise<void> {
    if (!this.isChecking) {
      if (!this.beginCheckRequest()) {
        return
      }
    }
    let completedByScan: boolean = false
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
      if (!this.hasPendingCheck()) {
        this.errorMessage = ''
        this.checkMessage = '本章已校对完成，暂无待检查内容'
        return
      }
      if (this.scanInFlight) {
        // 周期扫描恰在屏障期间启动：不并行，交给它结束后补跑一次。
        this.pendingScanFlag = true
        return
      }
      completedByScan = true
      await this.performIncrementalScan('manual', request, generation)
    } catch (e) {
      this.errorMessage = `校对请求失败：${String(e)}`
      this.checkMessage = ''
    } finally {
      if (!completedByScan) {
        this.isChecking = false
        this.notify()
      }
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
    result: ResultEnvelope<CorrectionChapterSnapshotDto>, origin: CorrectionCheckOrigin): void {
    if (generation !== this.requestGeneration) {
      return
    }
    const silent: boolean = origin === 'restore'
    const current = this.currentRequest(generation)
    if (current === null) {
      if (origin === 'manual') {
        // 校对返回时正文/会话已推进：结果不能投影到新 revision，给出可见原因。
        this.invalidate()
        this.errorMessage = '章节内容或会话已变化，本次校对结果已丢弃，请重新校对'
        this.notify()
      } else if (origin === 'periodic') {
        // 自动扫描结果作废时不打扰用户；下一轮周期继续补查。
        this.hasUncheckedSegments = true
        this.checkMessage = '正文已更新，本轮自动检查已跳过'
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
      if (origin === 'periodic') {
        this.checkMessage = `自动检查未完成：${this.describeCheckFailure(result)}`
        this.notify()
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
    this.updateCheckedProgress(snapshot.text, data.checkedRanges, data.hasUncheckedSegments)
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
    if (origin === 'manual') {
      const baseMessage = mapped.length > 0
        ? `检查完成：发现 ${mapped.length} 条建议`
        : '检查完成：未发现问题'
      // 资源标签必须来自刚重读的 Core 状态：模型准备失败但固定规则可用时，
      // 不能把“规则未发现”展示成 AI 校对完成。
      const label = this.resourceLabel()
      this.checkMessage = label.length > 0 ? `[${label}] ${baseMessage}` : baseMessage
    } else if (origin === 'periodic') {
      const baseMessage = mapped.length > 0
        ? `自动检查：发现 ${mapped.length} 条建议`
        : '自动检查：未发现新问题'
      const label = this.resourceLabel()
      this.checkMessage = label.length > 0 ? `[${label}] ${baseMessage}` : baseMessage
    }
    this.notify()
  }

  /** 用 Core 的 checkedRanges 合并区间，得到当前 revision 的已校对/待校对字节进度。 */
  private updateCheckedProgress(text: string, checkedRanges: CorrectionCheckedRangeDto[],
    hasUncheckedSegments: boolean): void {
    const totalBytes: number = utf16ToUtf8(text, text.length)
    let checked: number = 0
    let cursor: number = 0
    for (let i = 0; i < checkedRanges.length; i++) {
      const range = checkedRanges[i]
      const start: number = Math.max(cursor, range.startByte)
      const end: number = Math.min(range.endByte, totalBytes)
      if (end > start) {
        checked += end - start
        cursor = end
      }
    }
    this.totalBytes = totalBytes
    this.checkedBytes = Math.min(checked, totalBytes)
    this.pendingBytes = this.totalBytes - this.checkedBytes
    this.hasUncheckedSegments = hasUncheckedSegments
    this.progressKnown = true
    this.dirtySinceLastScan = false
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
      if (scope === 'rule') {
        // 停用规则改变 scanner 指纹：旧结果只保留展示，重新检查等到周期/手动。
        this.markCorrectionDirty()
      }
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
      // 词典变化只重标脏区，等下一轮周期或手动请求，不立即扫描。
      this.markCorrectionDirty()
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
    this.foreground = false
    this.stopChapterScanTimer()
    this.clear()
    this.modelPreparation = null
    this.listener = null
    this.scanCompletionListener = null
  }
}
