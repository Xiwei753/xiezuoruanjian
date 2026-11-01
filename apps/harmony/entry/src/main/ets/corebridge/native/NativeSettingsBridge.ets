import { ResultEnvelope } from '../dto/ResultEnvelope'
import { LocalSettings, SyncableSettings, BuiltinThemeDto } from '../dto/SettingsDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeLocalSettings, decodeSyncableSettings, decodeBuiltinTheme, decodeArray } from '../codec/CoreWireDecoders'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
}

export class NativeSettingsBridge {
  private core: NativeCoreModule
  // Issue #1026：LocalSettings 是整份 DTO 写入，异步 NAPI 不保证完成顺序。
  // 所有实例（设置页、写作页等）共用同一条 Promise 写入链：
  // 调用时先序列化并占据队列位置，逐项送入 NAPI；单次业务失败不阻断后续写入。
  private static localSaveQueue: Promise<void> = Promise.resolve()

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  /** 把一次 LocalSettings 写操作排入全局顺序队列。 */
  private static enqueueLocalSettingsTask<T>(operation: () => Promise<ResultEnvelope<T>>): Promise<ResultEnvelope<T>> {
    const task: Promise<ResultEnvelope<T>> = NativeSettingsBridge.localSaveQueue.then(
      (): Promise<ResultEnvelope<T>> => operation()
    )
    // 队列只关心前一项结束，不关心业务成功/失败；失败也必须让后续写入继续。
    NativeSettingsBridge.localSaveQueue = task.then(
      (): void => {},
      (): void => {},
    )
    return task
  }

  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativeLoadLocalSettings() as Promise<string>)
      return this.core.decodeEnvelope<LocalSettings>(jsonStr, decodeLocalSettings)
    } catch (e) { return nativeError<LocalSettings>(e) }
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    // 必须在调用时冻结 payload，不能等轮到队列时再读调用方可能已被修改的对象。
    const payload: string = JSON.stringify(settings)
    return await NativeSettingsBridge.enqueueLocalSettingsTask<boolean>(
      async (): Promise<ResultEnvelope<boolean>> => {
        try {
          const jsonStr: string =
            await (this.core.getNativeModule().nativeSaveLocalSettings(payload) as Promise<string>)
          return this.core.parseEnvelope<boolean>(jsonStr)
        } catch (e) { return nativeError<boolean>(e) }
      }
    )
  }

  /**
   * Issue #1026：在全局写入序列内完成“读取最新设置 → 局部修改 → 落盘 → 读回”。
   * 写作页只改字号/行距时用它，避免先读到的旧快照覆盖设置页刚写入的其它字段。
   */
  async updateLocalSettings(
    mutator: (settings: LocalSettings) => LocalSettings
  ): Promise<ResultEnvelope<LocalSettings>> {
    return await NativeSettingsBridge.enqueueLocalSettingsTask<LocalSettings>(
      async (): Promise<ResultEnvelope<LocalSettings>> => {
        try {
          const loadedJson: string =
            await (this.core.getNativeModule().nativeLoadLocalSettings() as Promise<string>)
          const loaded: ResultEnvelope<LocalSettings> =
            this.core.decodeEnvelope<LocalSettings>(loadedJson, decodeLocalSettings)
          if (!loaded.success || !loaded.data) {
            return { success: false, errorCode: loaded.errorCode || 'SETTINGS_NOT_FOUND', userMessage: loaded.userMessage }
          }
          const next: LocalSettings = mutator(loaded.data)
          const savedJson: string =
            await (this.core.getNativeModule().nativeSaveLocalSettings(JSON.stringify(next)) as Promise<string>)
          const saved: ResultEnvelope<boolean> = this.core.parseEnvelope<boolean>(savedJson)
          if (!saved.success) {
            return { success: false, errorCode: saved.errorCode || 'UNKNOWN_ERROR', userMessage: saved.userMessage }
          }
          const persistedJson: string =
            await (this.core.getNativeModule().nativeLoadLocalSettings() as Promise<string>)
          return this.core.decodeEnvelope<LocalSettings>(persistedJson, decodeLocalSettings)
        } catch (e) { return nativeError<LocalSettings>(e) }
      }
    )
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativeLoadSyncableSettings() as Promise<string>)
      return this.core.decodeEnvelope<SyncableSettings>(jsonStr, decodeSyncableSettings)
    } catch (e) { return nativeError<SyncableSettings>(e) }
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativeSaveSyncableSettings(JSON.stringify(settings)) as Promise<string>)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async listBuiltinThemes(): Promise<ResultEnvelope<BuiltinThemeDto[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListBuiltinThemes()
      return this.core.decodeEnvelope<BuiltinThemeDto[]>(jsonStr,
        (d: Object) => decodeArray<BuiltinThemeDto>(d, decodeBuiltinTheme, 'BuiltinTheme'))
    } catch (e) { return nativeError<BuiltinThemeDto[]>(e) }
  }
}
