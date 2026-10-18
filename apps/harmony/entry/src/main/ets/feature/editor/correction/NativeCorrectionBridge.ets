import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { CorrectionSuggestionDto, decodeCorrectionSuggestions } from './CorrectionDtos'
import { CorrectionIgnoreScope, ICorrectionBridge } from './ICorrectionBridge'
import { decodeEditorEditResult } from '../../../corebridge/codec/CoreWireDecoders'

/** Native NAPI bridge. The native correction handlers run on an async worker. */
export class NativeCorrectionBridge implements ICorrectionBridge {
  private core: NativeCoreModule = new NativeCoreModule()

  async setModelPath(path: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeCorrectionSetModelPath(path) as string
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async checkSegment(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number): Promise<ResultEnvelope<CorrectionSuggestionDto[]>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeCorrectionCheckSegment(
        sessionId, projectId, chapterId, textRevision, startByte, endByte) as string
      return this.core.decodeEnvelope<CorrectionSuggestionDto[]>(jsonStr, decodeCorrectionSuggestions)
    } catch (e) {
      return this.nativeError<CorrectionSuggestionDto[]>(e)
    }
  }

  async checkChapter(sessionId: number, projectId: string, chapterId: string,
    textRevision: number): Promise<ResultEnvelope<CorrectionSuggestionDto[]>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeCorrectionCheckChapter(
        sessionId, projectId, chapterId, textRevision) as string
      return this.core.decodeEnvelope<CorrectionSuggestionDto[]>(jsonStr, decodeCorrectionSuggestions)
    } catch (e) {
      return this.nativeError<CorrectionSuggestionDto[]>(e)
    }
  }

  async ignore(projectId: string, chapterId: string, ruleId: string, original: string,
    scope: CorrectionIgnoreScope, textRevision: number, startByte: number,
    endByte: number): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeCorrectionIgnore(
        projectId, chapterId, ruleId, original, scope, textRevision, startByte, endByte) as string
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async addDictionaryEntry(projectId: string, entry: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeCorrectionAddDictionaryEntry(
        projectId, entry) as string
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async apply(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number, original: string,
    replacement: string): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeCorrectionApply(
        sessionId, projectId, chapterId, textRevision, startByte, endByte, original, replacement) as string
      return this.core.decodeEnvelope<EditorEditResult>(jsonStr, decodeEditorEditResult)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  private nativeError<T>(e: Object): ResultEnvelope<T> {
    return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
  }
}
