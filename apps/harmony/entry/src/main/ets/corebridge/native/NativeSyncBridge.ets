import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { FullSyncDiagnosticsResult, FullSyncDryRunResult, FullSyncResult, SyncConfig, SyncState } from '../dto/SyncDtos'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeSyncBridge {
  constructor(private core: NativeCoreModule) {}

  // ── 全量同步统一入口（Issue #630） ──
  // 一个全局 SyncConfig + 一份全局凭据，App target + 所有 Project target 一次同步。
  // 旧的 per-project / app-level 双套入口已删除。

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

  async syncDryRun(): Promise<ResultEnvelope<FullSyncDryRunResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSyncDryRun()
      return this.core.parseEnvelope<FullSyncDryRunResult>(jsonStr)
    } catch (e) { return nativeError<FullSyncDryRunResult>(e) }
  }

  async performSync(): Promise<ResultEnvelope<FullSyncResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativePerformSync()
      return this.core.parseEnvelope<FullSyncResult>(jsonStr)
    } catch (e) { return nativeError<FullSyncResult>(e) }
  }

  async syncDiagnostics(): Promise<ResultEnvelope<FullSyncDiagnosticsResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSyncDiagnostics()
      return this.core.parseEnvelope<FullSyncDiagnosticsResult>(jsonStr)
    } catch (err) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.sync_failed', messageArgs: { 'detail': String(err) } as Record<string, string>, rawError: `syncDiagnostics failed: ${err}`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }

  // ── 应用级同步状态（Issue #600 评论 #5） ──
  // 路径：<app_data_root>/app-meta/sync/state.local.json

  async loadAppSyncState(): Promise<ResultEnvelope<SyncState>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadAppSyncState()
      return this.core.parseEnvelope<SyncState>(jsonStr)
    } catch (e) { return nativeError<SyncState>(e) }
  }

  async saveAppSyncState(state: SyncState): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveAppSyncState(JSON.stringify(state))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }
}
