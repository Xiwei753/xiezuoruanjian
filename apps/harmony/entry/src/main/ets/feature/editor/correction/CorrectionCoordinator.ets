import { EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { utf16ToUtf8, utf8ToUtf16 } from '../input/text_offset_mapper'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { CorrectionIgnoreScope, ICorrectionBridge } from './ICorrectionBridge'
import {
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
  cursorByte: number
}

interface CorrectionByteRange {
  startByte: number
  endByte: number
}

const SENTENCE_ENDINGS = '。！？!?\n'

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
  private debounceTimer: number = -1

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

  /** Called after each Core editor snapshot; edits invalidate old marks immediately. */
  acceptSnapshot(projectId: string, chapterId: string, snapshot: EditorSessionSnapshot): void {
    const sessionId = this.editor.getSessionId()
    if (sessionId === null || projectId.length === 0 || chapterId.length === 0) {
      this.invalidate()
      return
    }

    if (snapshot.composition !== undefined) {
      this.current = null
      this.invalidate()
      return
    }

    const nextSnapshot: CorrectionRequestSnapshot = {
      projectId: projectId,
      chapterId: chapterId,
      sessionId: sessionId,
      text: snapshot.text,
      revision: snapshot.revision,
      cursorByte: snapshot.cursor
    }
    const previous = this.current
    if (previous !== null && previous.projectId === projectId && previous.chapterId === chapterId &&
        previous.sessionId === sessionId && previous.revision === snapshot.revision && previous.text === snapshot.text) {
      const previousRange = this.sentenceByteRange(previous)
      const nextRange = this.sentenceByteRange(nextSnapshot)
      if ((previousRange === null && nextRange === null) ||
          (previousRange !== null && nextRange !== null &&
            previousRange.startByte === nextRange.startByte && previousRange.endByte === nextRange.endByte)) {
        this.current = nextSnapshot
        return
      }
    }

    this.current = nextSnapshot
    this.requestGeneration++
    const generation = this.requestGeneration
    this.cancelDebounce()
    this.suggestions = []
    this.isChecking = false
    this.errorMessage = ''
    this.notify()
    this.debounceTimer = setTimeout((): void => {
      this.debounceTimer = -1
      void this.checkCurrentSegment(generation)
    }, 420)
  }

  private invalidate(): void {
    this.requestGeneration++
    this.cancelDebounce()
    this.suggestions = []
    this.isChecking = false
    this.errorMessage = ''
    this.notify()
  }

  clear(): void {
    this.current = null
    this.invalidate()
  }

  private cancelDebounce(): void {
    if (this.debounceTimer >= 0) {
      clearTimeout(this.debounceTimer)
      this.debounceTimer = -1
    }
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

  private sentenceByteRange(snapshot: CorrectionRequestSnapshot): CorrectionByteRange | null {
    const text = snapshot.text
    if (text.length === 0) {
      return null
    }
    const cursor = Math.min(text.length, Math.max(0, utf8ToUtf16(text, snapshot.cursorByte)))
    let startUtf16 = 0
    for (let i = Math.min(cursor, text.length) - 1; i >= 0; i--) {
      if (SENTENCE_ENDINGS.indexOf(text.charAt(i)) >= 0) {
        startUtf16 = i + 1
        break
      }
    }
    let endUtf16 = text.length
    for (let i = cursor; i < text.length; i++) {
      if (SENTENCE_ENDINGS.indexOf(text.charAt(i)) >= 0) {
        endUtf16 = i + 1
        break
      }
    }
    while (startUtf16 < endUtf16 && /\s/.test(text.charAt(startUtf16))) {
      startUtf16++
    }
    if (startUtf16 >= endUtf16) {
      return null
    }
    const range: CorrectionByteRange = {
      startByte: utf16ToUtf8(text, startUtf16),
      endByte: utf16ToUtf8(text, endUtf16)
    }
    return range
  }

  private async checkCurrentSegment(generation: number): Promise<void> {
    const snapshot = this.currentRequest(generation)
    if (snapshot === null) {
      if (generation === this.requestGeneration) {
        this.invalidate()
      }
      return
    }
    const range = this.sentenceByteRange(snapshot)
    if (range === null) {
      this.suggestions = []
      this.isChecking = false
      this.notify()
      return
    }
    this.isChecking = true
    this.errorMessage = ''
    this.notify()
    try {
      const result = await this.bridge.checkSegment(
        snapshot.sessionId, snapshot.projectId, snapshot.chapterId, snapshot.revision,
        range.startByte, range.endByte)
      this.acceptCheckResult(generation, snapshot, result)
    } finally {
      this.notifyScanCompleted()
    }
  }

  /** User-triggered whole-chapter correction uses the same Core engine and snapshot identity. */
  async checkChapter(): Promise<void> {
    this.cancelDebounce()
    const snapshot = this.current
    if (snapshot === null) {
      return
    }
    this.requestGeneration++
    const generation = this.requestGeneration
    this.suggestions = []
    this.isChecking = true
    this.errorMessage = ''
    this.notify()
    try {
      const result = await this.bridge.checkChapter(
        snapshot.sessionId, snapshot.projectId, snapshot.chapterId, snapshot.revision)
      this.acceptCheckResult(generation, snapshot, result)
    } finally {
      this.notifyScanCompleted()
    }
  }

  private acceptCheckResult(generation: number, snapshot: CorrectionRequestSnapshot,
    result: ResultEnvelope<CorrectionSuggestionDto[]>): void {
    if (generation !== this.requestGeneration) {
      return
    }
    const current = this.currentRequest(generation)
    if (current === null) {
      this.invalidate()
      return
    }
    if (current.revision !== snapshot.revision || current.chapterId !== snapshot.chapterId ||
        current.projectId !== snapshot.projectId || current.sessionId !== snapshot.sessionId) {
      return
    }
    this.isChecking = false
    if (!result.success || result.data === undefined) {
      this.suggestions = []
      this.errorMessage = result.userMessage ?? result.errorCode ?? '校对暂不可用'
      this.notify()
      return
    }
    const mapped: MappedCorrectionSuggestion[] = []
    for (let i = 0; i < result.data.length; i++) {
      const suggestion = this.mapSuggestion(snapshot.text, snapshot.revision, result.data[i])
      if (suggestion !== null) {
        mapped.push(suggestion)
      }
    }
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
