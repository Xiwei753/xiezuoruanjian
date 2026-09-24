import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { LocalSettings, SyncableSettings, ThemePaletteRecordDto, BuiltinThemeDto } from '../dto/SettingsDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeLocalSettings, decodeSyncableSettings, decodeThemePaletteRecord, decodeBuiltinTheme, decodeArray } from '../codec/CoreWireDecoders'

function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeSettingsBridge {
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadLocalSettings()
      return this.core.decodeEnvelope<LocalSettings>(jsonStr, decodeLocalSettings)
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
      return this.core.decodeEnvelope<SyncableSettings>(jsonStr, decodeSyncableSettings)
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
      return this.core.decodeEnvelope<ThemePaletteRecordDto[]>(jsonStr,
        (d: Object) => decodeArray<ThemePaletteRecordDto>(d, decodeThemePaletteRecord, 'ThemePaletteRecord'))
    } catch (e) { return nativeError<ThemePaletteRecordDto[]>(e) }
  }

  async loadPaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<ThemePaletteRecordDto>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadPaletteRecord(deviceId, fingerprint)
      return this.core.decodeEnvelope<ThemePaletteRecordDto>(jsonStr, decodeThemePaletteRecord)
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
      return this.core.decodeEnvelope<BuiltinThemeDto[]>(jsonStr,
        (d: Object) => decodeArray<BuiltinThemeDto>(d, decodeBuiltinTheme, 'BuiltinTheme'))
    } catch (e) { return nativeError<BuiltinThemeDto[]>(e) }
  }
}
