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

export interface CorrectionPresentationState {
  suggestions: MappedCorrectionSuggestion[]
  isChecking: boolean
  errorMessage: string
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
  private bundledModelAttempted: boolean = false

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
      errorMessage: this.errorMessage
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

  /**
   * Issue #985: 内置 MacBERT4CSC ONNX 模型兜底，由会话打开回调触发：
   * - Core 已配置用户模型（`modelConfigured`）时直接跳过，用户模型优先、不覆盖；
   * - 未配置时先让调用方准备内置模型文件（prepare 返回沙箱路径），再走既有
   *   `setModelPath` 交给 Core 校验切换；
   * - 只有确认已有用户模型或内置模型成功交给 Core 后才记为已处理；首次初始化
   *   失败（状态不可用 / 准备失败 / 切换失败）保持可重试，由后续会话打开再试；
   * - 失败返回错误 envelope，由调用方记 warning；编辑与固定规则不受影响。
   * `data === true` 表示本次真正配置了内置模型。
   */
  async ensureBundledModel(prepare: () => Promise<string>): Promise<ResultEnvelope<boolean>> {
    if (this.bundledModelAttempted) {
      return { success: true, data: false }
    }
    const status = await this.bridge.getResourceStatus()
    if (!status.success || status.data === undefined) {
      return {
        success: false,
        errorCode: status.errorCode ?? 'RESOURCE_STATUS_UNAVAILABLE',
        userMessage: status.userMessage
      }
    }
    // 旧版 .klm 也会是 modelConfigured=true，但 Core 已拒绝该格式；
    // 只有新 ONNX 目录完整可用时才跳过迁移。
    if (status.data.modelConfigured && !status.data.modelMigrationRequired &&
        status.data.modelFileAvailable) {
      this.bundledModelAttempted = true
      return { success: true, data: false }
    }
    let path = ''
    try {
      path = await prepare()
    } catch (e) {
      return { success: false, errorCode: 'BUNDLED_MODEL_PREPARE_FAILED', userMessage: String(e) }
    }
    if (path.length === 0) {
      return { success: false, errorCode: 'BUNDLED_MODEL_PREPARE_FAILED', userMessage: '内置模型路径为空' }
    }
    const result = await this.bridge.setModelPath(path)
    if (result.success) {
      this.bundledModelAttempted = true
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

  /** User-triggered whole-chapter correction uses the same Core engine and snapshot identity. */
  async checkChapter(): Promise<void> {
    const snapshot = this.current
    if (snapshot === null) {
      return
    }
    if (this.pendingScanTimer >= 0) {
      clearTimeout(this.pendingScanTimer)
      this.pendingScanTimer = -1
    }
    this.pendingScanSnapshot = null
    this.requestGeneration++
    const generation = this.requestGeneration
    this.isChecking = true
    this.errorMessage = ''
    this.notify()
    try {
      const result = await this.bridge.checkChapter(
        snapshot.sessionId, snapshot.projectId, snapshot.chapterId, snapshot.revision)
      this.acceptCheckResult(generation, snapshot, result, false)
    } finally {
      this.notifyScanCompleted()
    }
  }

  private acceptCheckResult(generation: number, snapshot: CorrectionRequestSnapshot,
    result: ResultEnvelope<CorrectionChapterSnapshotDto>, silent: boolean): void {
    if (generation !== this.requestGeneration) {
      return
    }
    const current = this.currentRequest(generation)
    if (current === null) {
      if (!silent) {
        this.invalidate()
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
      this.errorMessage = result.userMessage ?? result.errorCode ?? '校对暂不可用'
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
