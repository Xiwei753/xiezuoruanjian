import { ChangedEntity, ResultEnvelope, LocalSettings, SyncableSettings } from '../model/CoreDtos'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeSettingsBridge {
  constructor(private core: NativeCoreModule) {}

  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadLocalSettings()
      return this.core.parseEnvelope<LocalSettings>(jsonStr)
    } catch (e) { return nativeError<LocalSettings>(e) }
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveLocalSettings(JSON.stringify(settings))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadSyncableSettings()
      return this.core.parseEnvelope<SyncableSettings>(jsonStr)
    } catch (e) { return nativeError<SyncableSettings>(e) }
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveSyncableSettings(JSON.stringify(settings))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }
}
