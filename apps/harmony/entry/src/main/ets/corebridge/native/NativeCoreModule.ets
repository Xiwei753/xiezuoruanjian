import nativeModule from 'libwriter_core.so'
import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'

interface NativeJsonEnvelope {
  success: boolean
  data?: Record<string, Object>
  errorCode?: string
  messageKey?: string
  messageArgs?: Record<string, string>
  userMessage?: string
  rawError?: string
  warnings?: string[]
  changedPaths?: string[]
  changedEntities?: ChangedEntity[]
}

interface ChapterLocationResult {
  projectId: string
  volumeId: string
}

interface VolumeLocationResult {
  projectId: string
  volumeId: string
}

/**
 * Native Core 公共模块。
 *
 * 提供 getNativeModule、parseEnvelope、notImplemented 等公共能力，
 * 供各分域 bridge 复用。
 */
export class NativeCoreModule {
  private nativeModuleAvailable: boolean = true

  getNativeModule(): Record<string, Function> {
    if (!this.nativeModuleAvailable) {
      throw new Error('Native module is not available (previously failed to load)')
    }
    if (nativeModule === null || nativeModule === undefined) {
      this.nativeModuleAvailable = false
      throw new Error('Native module is null or undefined')
    }
    return nativeModule as Record<string, Function>
  }

  parseEnvelope<T>(jsonStr: string): ResultEnvelope<T> {
    try {
      const obj: NativeJsonEnvelope = JSON.parse(jsonStr) as NativeJsonEnvelope
      const result: ResultEnvelope<T> = {
        success: obj.success,
        data: obj.data as T | undefined,
        errorCode: obj.errorCode as string | undefined,
        messageKey: obj.messageKey,
        messageArgs: obj.messageArgs,
        userMessage: obj.userMessage,
        rawError: obj.rawError,
        warnings: obj.warnings ?? [] as string[],
        changedPaths: obj.changedPaths ?? [] as string[],
        changedEntities: obj.changedEntities ?? [] as ChangedEntity[]
      }
      return result
    } catch (e) {
      const errResult: ResultEnvelope<T> = {
        success: false,
        errorCode: 'PARSE_ERROR',
        messageKey: 'error.parse_error',
        messageArgs: { 'detail': `${e}` } as Record<string, string>,
        rawError: `JSON parse error: ${e}`,
        warnings: [] as string[],
        changedPaths: [] as string[],
        changedEntities: [] as ChangedEntity[]
      }
      return errResult
    }
  }

  notImplemented<T>(): ResultEnvelope<T> {
    const result: ResultEnvelope<T> = {
      success: false,
      errorCode: 'NOT_IMPLEMENTED',
      messageKey: 'error.not_implemented_bridge',
      rawError: 'Native bridge not implemented yet',
      warnings: [] as string[],
      changedPaths: [] as string[],
      changedEntities: [] as ChangedEntity[]
    }
    return result
  }

  async resolveChapterLocation(chapterId: string): Promise<ChapterLocationResult | null> {
    try {
      const jsonStr: string = this.getNativeModule().nativeResolveChapterLocation(chapterId)
      const result = this.parseEnvelope<ChapterLocationResult>(jsonStr)
      if (result.success && result.data) {
        const loc: ChapterLocationResult = {
          projectId: result.data.projectId,
          volumeId: result.data.volumeId
        }
        return loc
      }
      return null
    } catch (e) {
      return null
    }
  }

  async resolveVolumeProjectId(volumeId: string): Promise<string | null> {
    try {
      const jsonStr: string = this.getNativeModule().nativeResolveVolumeLocation(volumeId)
      const result = this.parseEnvelope<VolumeLocationResult>(jsonStr)
      if (result.success && result.data) {
        return result.data.projectId
      }
      return null
    } catch (e) {
      return null
    }
  }

  markNativeUnavailable(): void {
    this.nativeModuleAvailable = false
  }
}
