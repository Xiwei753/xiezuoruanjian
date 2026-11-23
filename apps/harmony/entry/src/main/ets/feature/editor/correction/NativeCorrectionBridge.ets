import { resourceManager } from '@kit.LocalizationKit'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import {
  CorrectionChapterSnapshotDto,
  CorrectionModelPathStatusDto,
  CorrectionResourceStatusDto,
  decodeCorrectionChapterSnapshot,
  decodeCorrectionModelPathStatus,
  decodeCorrectionResourceStatus
} from './CorrectionDtos'
import { CorrectionIgnoreScope, ICorrectionBridge } from './ICorrectionBridge'
import { decodeEditorEditResult } from '../../../corebridge/codec/CoreWireDecoders'
import hilog from '@ohos.hilog'

const CORRECTION_LOG_DOMAIN = 0x0000
const CORRECTION_LOG_TAG = 'NativeCorrectionBridge'

/** Native NAPI bridge. The native correction handlers run on an async worker. */
export class NativeCorrectionBridge implements ICorrectionBridge {
  private core: NativeCoreModule = new NativeCoreModule()

  async setModelPath(path: string): Promise<ResultEnvelope<boolean>> {
    const startedAt = Date.now()
    try {
      const pending: Promise<string> =
        this.core.getNativeModule().nativeCorrectionSetModelPath(path) as Promise<string>
      const jsonStr: string = await pending
      const result = this.core.parseEnvelope<boolean>(jsonStr)
      this.logEnvelope('setModelPath', startedAt, result, -1)
      return result
    } catch (e) {
      this.logThrown('setModelPath', startedAt, e)
      return this.nativeError<boolean>(e)
    }
  }

  async getResourceStatus(): Promise<ResultEnvelope<CorrectionResourceStatusDto>> {
    const startedAt = Date.now()
    try {
      const pending: Promise<string> =
        this.core.getNativeModule().nativeCorrectionGetResourceStatus() as Promise<string>
      const jsonStr: string = await pending
      const result = this.core.decodeEnvelope<CorrectionResourceStatusDto>(jsonStr, decodeCorrectionResourceStatus)
      const modelState: string = result.data !== undefined
        ? `configured=${result.data.modelConfigured} available=${result.data.modelFileAvailable}`
          + ` migration=${result.data.modelMigrationRequired} lexicon=${result.data.bundledLexiconAvailable}`
        : 'unknown'
      this.logEnvelope('getResourceStatus', startedAt, result, -1, modelState)
      return result
    } catch (e) {
      this.logThrown('getResourceStatus', startedAt, e)
      return this.nativeError<CorrectionResourceStatusDto>(e)
    }
  }

  async getModelPathStatus(path: string): Promise<ResultEnvelope<CorrectionModelPathStatusDto>> {
    try {
      const pending: Promise<string> =
        this.core.getNativeModule().nativeCorrectionGetModelPathStatus(path) as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<CorrectionModelPathStatusDto>(jsonStr, decodeCorrectionModelPathStatus)
    } catch (e) {
      return this.nativeError<CorrectionModelPathStatusDto>(e)
    }
  }

  /**
   * Issue #958: the native handler resolves the final sandbox path directly
   * (no envelope) and rejects with an Error when the rawfile copy or its
   * size/SHA-256 check fails; errors propagate to the caller.
   */
  async prepareBundledCorrectionModel(manager: resourceManager.ResourceManager, rawFileName: string,
    destinationPath: string, expectedSize: number, expectedSha256: string): Promise<string> {
    const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionPrepareBundledModel(
      manager, rawFileName, destinationPath, expectedSize, expectedSha256) as Promise<string>
    const path: string = await pending
    if (path.length === 0) {
      throw new Error('内置模型准备未返回沙箱路径')
    }
    return path
  }

  async checkSegment(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionCheckSegment(
        sessionId, projectId, chapterId, textRevision, startByte, endByte) as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<CorrectionChapterSnapshotDto>(jsonStr, decodeCorrectionChapterSnapshot)
    } catch (e) {
      return this.nativeError<CorrectionChapterSnapshotDto>(e)
    }
  }

  async checkEdits(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    baseRevision: number, text: string,
    editsJson: string): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionCheckEdits(
        sessionId, projectId, chapterId, textRevision, baseRevision, text, editsJson) as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<CorrectionChapterSnapshotDto>(jsonStr, decodeCorrectionChapterSnapshot)
    } catch (e) {
      return this.nativeError<CorrectionChapterSnapshotDto>(e)
    }
  }

  async applyEdits(sessionId: number, projectId: string, chapterId: string,
    textRevision: number, baseRevision: number,
    editsJson: string): Promise<ResultEnvelope<boolean>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionApplyEdits(
        sessionId, projectId, chapterId, textRevision, baseRevision, editsJson) as Promise<string>
      const jsonStr: string = await pending
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async scanPending(sessionId: number, projectId: string, chapterId: string,
    textRevision: number, text: string): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>> {
    const startedAt = Date.now()
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionScanPending(
        sessionId, projectId, chapterId, textRevision, text) as Promise<string>
      const jsonStr: string = await pending
      const result = this.core.decodeEnvelope<CorrectionChapterSnapshotDto>(
        jsonStr, decodeCorrectionChapterSnapshot)
      const count: number = result.data !== undefined ? result.data.suggestions.length : -1
      this.logEnvelope('scanPending', startedAt, result, count)
      return result
    } catch (e) {
      this.logThrown('scanPending', startedAt, e)
      return this.nativeError<CorrectionChapterSnapshotDto>(e)
    }
  }

  async checkChapter(sessionId: number, projectId: string, chapterId: string,
    textRevision: number): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>> {
    const startedAt = Date.now()
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionCheckChapter(
        sessionId, projectId, chapterId, textRevision) as Promise<string>
      const jsonStr: string = await pending
      const result = this.core.decodeEnvelope<CorrectionChapterSnapshotDto>(
        jsonStr, decodeCorrectionChapterSnapshot)
      const count: number = result.data !== undefined ? result.data.suggestions.length : -1
      this.logEnvelope('checkChapter', startedAt, result, count)
      return result
    } catch (e) {
      this.logThrown('checkChapter', startedAt, e)
      return this.nativeError<CorrectionChapterSnapshotDto>(e)
    }
  }

  async getChapterSnapshot(sessionId: number, projectId: string, chapterId: string,
    textRevision: number): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionGetChapterSnapshot(
        sessionId, projectId, chapterId, textRevision) as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<CorrectionChapterSnapshotDto>(jsonStr, decodeCorrectionChapterSnapshot)
    } catch (e) {
      return this.nativeError<CorrectionChapterSnapshotDto>(e)
    }
  }

  async ignore(projectId: string, chapterId: string, ruleId: string, original: string,
    scope: CorrectionIgnoreScope, textRevision: number, startByte: number,
    endByte: number): Promise<ResultEnvelope<boolean>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionIgnore(
        projectId, chapterId, ruleId, original, scope, textRevision, startByte, endByte) as Promise<string>
      const jsonStr: string = await pending
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async addDictionaryEntry(projectId: string, entry: string): Promise<ResultEnvelope<boolean>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionAddDictionaryEntry(
        projectId, entry) as Promise<string>
      const jsonStr: string = await pending
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async apply(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number, original: string,
    replacement: string): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionApply(
        sessionId, projectId, chapterId, textRevision, startByte, endByte, original, replacement) as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<EditorEditResult>(jsonStr, decodeEditorEditResult)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  // Issue #1054：只记录调用结果、耗时、模型状态与建议数量，不记录正文内容。
  private logEnvelope<T>(operation: string, startedAt: number, result: ResultEnvelope<T>,
    suggestionCount: number, extra: string = ''): void {
    const elapsedMs = Date.now() - startedAt
    const suffix = suggestionCount >= 0 ? ` suggestions=${suggestionCount}` : ''
    const detail = extra.length > 0 ? ` ${extra}` : ''
    if (result.success) {
      hilog.info(CORRECTION_LOG_DOMAIN, CORRECTION_LOG_TAG,
        `correction ${operation} ok elapsedMs=${elapsedMs}${suffix}${detail}`)
    } else {
      hilog.warn(CORRECTION_LOG_DOMAIN, CORRECTION_LOG_TAG,
        `correction ${operation} failed elapsedMs=${elapsedMs} errorCode=${result.errorCode ?? 'UNKNOWN'}`
          + ` message=${result.userMessage ?? ''}${suffix}${detail}`)
    }
  }

  private logThrown(operation: string, startedAt: number, e: Object): void {
    hilog.error(CORRECTION_LOG_DOMAIN, CORRECTION_LOG_TAG,
      `correction ${operation} threw elapsedMs=${Date.now() - startedAt} error=${String(e)}`)
  }

  private nativeError<T>(e: Object): ResultEnvelope<T> {
    return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
  }
}
