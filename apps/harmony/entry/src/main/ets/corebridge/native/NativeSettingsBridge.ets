import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { LocalSettings, SyncableSettings, ThemePaletteRecordDto, BuiltinThemeDto } from '../dto/SettingsDtos'
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

  async listPaletteRecords(): Promise<ResultEnvelope<ThemePaletteRecordDto[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListPaletteRecords()
      return this.core.parseEnvelope<ThemePaletteRecordDto[]>(jsonStr)
    } catch (e) { return nativeError<ThemePaletteRecordDto[]>(e) }
  }

  async loadPaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<ThemePaletteRecordDto>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadPaletteRecord(deviceId, fingerprint)
      return this.core.parseEnvelope<ThemePaletteRecordDto>(jsonStr)
    } catch (e) { return nativeError<ThemePaletteRecordDto>(e) }
  }

  async deletePaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeDeletePaletteRecord(deviceId, fingerprint)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async listBuiltinThemes(): Promise<ResultEnvelope<BuiltinThemeDto[]>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeListBuiltinThemes()
      return this.core.parseEnvelope<BuiltinThemeDto[]>(jsonStr)
    } catch (e) { return nativeError<BuiltinThemeDto[]>(e) }
  }
}
