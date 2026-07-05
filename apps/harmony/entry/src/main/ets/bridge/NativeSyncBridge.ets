import { ChangedEntity, ResultEnvelope, SyncConfig, SyncResult, SyncDiagnosticsResult } from '../model/CoreDtos'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeSyncBridge {
  constructor(private core: NativeCoreModule) {}

  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadSyncConfig()
      return this.core.parseEnvelope<SyncConfig>(jsonStr)
    } catch (e) { return nativeError<SyncConfig>(e) }
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveSyncConfig(JSON.stringify(config))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async syncDryRun(): Promise<ResultEnvelope<SyncResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSyncDryRun()
      return this.core.parseEnvelope<SyncResult>(jsonStr)
    } catch (e) { return nativeError<SyncResult>(e) }
  }

  async performSync(): Promise<ResultEnvelope<SyncResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativePerformSync()
      return this.core.parseEnvelope<SyncResult>(jsonStr)
    } catch (e) { return nativeError<SyncResult>(e) }
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSyncDiagnostics()
      return this.core.parseEnvelope<SyncDiagnosticsResult>(jsonStr)
    } catch (err) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.sync_failed', messageArgs: { 'detail': String(err) } as Record<string, string>, rawError: `syncDiagnostics failed: ${err}`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }
}
