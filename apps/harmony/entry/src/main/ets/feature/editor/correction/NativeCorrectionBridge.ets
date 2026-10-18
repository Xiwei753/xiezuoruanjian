import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import {
  CorrectionResourceStatusDto,
  CorrectionSuggestionDto,
  decodeCorrectionResourceStatus,
  decodeCorrectionSuggestions
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

  async checkSegment(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number): Promise<ResultEnvelope<CorrectionSuggestionDto[]>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionCheckSegment(
        sessionId, projectId, chapterId, textRevision, startByte, endByte) as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<CorrectionSuggestionDto[]>(jsonStr, decodeCorrectionSuggestions)
    } catch (e) {
      return this.nativeError<CorrectionSuggestionDto[]>(e)
    }
  }

  async checkChapter(sessionId: number, projectId: string, chapterId: string,
    textRevision: number): Promise<ResultEnvelope<CorrectionSuggestionDto[]>> {
    try {
      const pending: Promise<string> = this.core.getNativeModule().nativeCorrectionCheckChapter(
        sessionId, projectId, chapterId, textRevision) as Promise<string>
      const jsonStr: string = await pending
      return this.core.decodeEnvelope<CorrectionSuggestionDto[]>(jsonStr, decodeCorrectionSuggestions)
    } catch (e) {
      return this.nativeError<CorrectionSuggestionDto[]>(e)
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
