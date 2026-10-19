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

/** Native NAPI bridge. The native correction handlers run on an async worker. */
export class NativeCorrectionBridge implements ICorrectionBridge {
  private core: NativeCoreModule = new NativeCoreModule()

  async setModelPath(path: string): Promise<ResultEnvelope<boolean>> {
    try {
      const pending: Promise<string> =
        this.core.getNativeModule().nativeCorrectionSetModelPath(path) as Promise<string>
      const jsonStr: string = await pending
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async getResourceStatus(): Promise<ResultEnvelope<CorrectionResourceStatusDto>> {
    try {
      const pending: Promise<string> =
        this.core.getNativeModule().nativeCorrectionGetResourceStatus() as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<CorrectionResourceStatusDto>(jsonStr, decodeCorrectionResourceStatus)
    } catch (e) {
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

  async checkChapter(sessionId: number, projectId: string, chapterId: string,
    textRevision: number): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionCheckChapter(
        sessionId, projectId, chapterId, textRevision) as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<CorrectionChapterSnapshotDto>(jsonStr, decodeCorrectionChapterSnapshot)
    } catch (e) {
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

  private nativeError<T>(e: Object): ResultEnvelope<T> {
    return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
  }
}
