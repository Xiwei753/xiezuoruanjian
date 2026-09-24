import nativeModule from 'libwriter_core.so'
import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { DecodeResult } from '../codec/CoreWireDecoders'

interface NativeJsonEnvelope {
  success: boolean
  data?: Object
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
 * 提供 getNativeModule、parseRawEnvelope、notImplemented 等公共能力，
 * 供各分域 bridge 复用。
 *
 * Issue #753：envelope 解析只负责搬运，不做 `obj.data as T` 强转。
 * `parseRawEnvelope` 返回的 `data` 是原始未解码 JSON 对象（`Object | undefined`），
 * 各领域 Bridge 调用 `parseRawEnvelope` 后用 CoreWireDecoders 的 decoder 转成强类型。
 * 字段缺失/类型错误时 Bridge 层返回 `SCHEMA_MISMATCH`，不让 undefined 流到 UI。
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

  /**
   * 解析 envelope JSON 字符串，返回原始未解码的 `data`。
   *
   * `data` 类型是 `Object | undefined`（原始 JSON 对象/标量），不做任何 `as T` 强转。
   * 各领域 Bridge 拿到 `ResultEnvelope<Object>` 后，用 CoreWireDecoders 的 decoder
   * 把 `data` 转成强类型对象；decoder 失败时 Bridge 构造 `SCHEMA_MISMATCH` 错误。
   *
   * 这是 Issue #753 收口的核心入口：envelope 只搬运，不解码业务 DTO。
   */
  parseRawEnvelope(jsonStr: string): ResultEnvelope<Object> {
    try {
      const obj: NativeJsonEnvelope = JSON.parse(jsonStr) as NativeJsonEnvelope
      const result: ResultEnvelope<Object> = {
        success: obj.success,
        data: obj.data,
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
      const errResult: ResultEnvelope<Object> = {
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

  /**
   * 解析 envelope 并把 `data` 当作简单标量（boolean/string/number）或简单内部结构返回。
   *
   * 仅用于 Core 返回标量 data 的场景（save/delete 返回 boolean、getLastError 返回 string 等）。
   * 复杂 DTO（LocalSettings、Project、ChapterContent 等）不得用此方法，
   * 必须用 `parseRawEnvelope` + CoreWireDecoders decoder，否则字段不匹配时
   * `as T` 不会报错但实际字段是 undefined，流到 UI 后访问 undefined 属性会闪退。
   *
   * @deprecated 复杂 DTO 请用 parseRawEnvelope + decoder。仅保留用于标量 data。
   */
  parseEnvelope<T>(jsonStr: string): ResultEnvelope<T> {
    const raw: ResultEnvelope<Object> = this.parseRawEnvelope(jsonStr)
    const result: ResultEnvelope<T> = {
      success: raw.success,
      data: raw.data as T | undefined,
      errorCode: raw.errorCode,
      messageKey: raw.messageKey,
      messageArgs: raw.messageArgs,
      userMessage: raw.userMessage,
      rawError: raw.rawError,
      warnings: raw.warnings,
      changedPaths: raw.changedPaths,
      changedEntities: raw.changedEntities
    }
    return result
  }

  /**
   * 构造 `SCHEMA_MISMATCH` 错误 envelope。
   *
   * 各 Bridge 在 decoder 返回错误时调用此方法，把 decoder 的描述信息包进 rawError，
   * 阻止不一致数据流到 UI。
   */
  schemaMismatch<T>(message: string): ResultEnvelope<T> {
    const result: ResultEnvelope<T> = {
      success: false,
      errorCode: 'SCHEMA_MISMATCH',
      messageKey: 'error.schema_mismatch',
      messageArgs: { 'detail': message } as Record<string, string>,
      rawError: `SCHEMA_MISMATCH: ${message}`,
      warnings: [] as string[],
      changedPaths: [] as string[],
      changedEntities: [] as ChangedEntity[]
    }
    return result
  }

  /**
   * 解析 envelope 并用 decoder 把原始 data 转成强类型 T。
   *
   * 流程：parseRawEnvelope → 若 envelope 失败则透传错误 → 若 data 为空则 SCHEMA_MISMATCH →
   * decoder(data) → 若 decoder 失败则 SCHEMA_MISMATCH → 返回强类型 data。
   *
   * 各领域 Bridge 用此方法一行完成"解析 + 解码 + 错误拦截"。
   */
  decodeEnvelope<T>(jsonStr: string, decoder: (data: Object) => DecodeResult<T>): ResultEnvelope<T> {
    const raw: ResultEnvelope<Object> = this.parseRawEnvelope(jsonStr)
    if (!raw.success) {
      const errResult: ResultEnvelope<T> = {
        success: false,
        errorCode: raw.errorCode,
        messageKey: raw.messageKey,
        messageArgs: raw.messageArgs,
        userMessage: raw.userMessage,
        rawError: raw.rawError,
        warnings: raw.warnings,
        changedPaths: raw.changedPaths,
        changedEntities: raw.changedEntities
      }
      return errResult
    }
    if (raw.data === undefined || raw.data === null) {
      return this.schemaMismatch<T>('data is null or undefined')
    }
    const decoded: DecodeResult<T> = decoder(raw.data)
    if (!decoded.ok || decoded.value === undefined) {
      return this.schemaMismatch<T>(decoded.error ?? 'decode failed')
    }
    const result: ResultEnvelope<T> = {
      success: true,
      data: decoded.value,
      warnings: raw.warnings,
      changedPaths: raw.changedPaths,
      changedEntities: raw.changedEntities
    }
    return result
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
      const raw = this.parseRawEnvelope(jsonStr)
      if (raw.success && raw.data !== undefined && raw.data !== null) {
        const data = raw.data as Record<string, Object>
        const projectId: Object | undefined = data['projectId']
        const volumeId: Object | undefined = data['volumeId']
        if (typeof projectId === 'string' && typeof volumeId === 'string') {
          const loc: ChapterLocationResult = {
            projectId: projectId,
            volumeId: volumeId
          }
          return loc
        }
      }
      return null
    } catch (e) {
      return null
    }
  }

  async resolveVolumeProjectId(volumeId: string): Promise<string | null> {
    try {
      const jsonStr: string = this.getNativeModule().nativeResolveVolumeLocation(volumeId)
      const raw = this.parseRawEnvelope(jsonStr)
      if (raw.success && raw.data !== undefined && raw.data !== null) {
        const data = raw.data as Record<string, Object>
        const projectId: Object | undefined = data['projectId']
        if (typeof projectId === 'string') {
          return projectId
        }
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
