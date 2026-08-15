import { ChangedEntity, ResultEnvelope } from '../dto/ResultEnvelope'
import { SyncConfig, SyncResult, SyncDiagnosticsResult, SyncState } from '../dto/SyncDtos'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

export class NativeSyncBridge {
  constructor(private core: NativeCoreModule) {}

  // ── 作品级同步（per-project: projectId selects the sync root） ──

  async loadSyncConfig(projectId: string): Promise<ResultEnvelope<SyncConfig>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadSyncConfig(projectId)
      return this.core.parseEnvelope<SyncConfig>(jsonStr)
    } catch (e) { return nativeError<SyncConfig>(e) }
  }

  async saveSyncConfig(projectId: string, config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveSyncConfig(projectId, JSON.stringify(config))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async syncDryRun(projectId: string): Promise<ResultEnvelope<SyncResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSyncDryRun(projectId)
      return this.core.parseEnvelope<SyncResult>(jsonStr)
    } catch (e) { return nativeError<SyncResult>(e) }
  }

  async performSync(projectId: string): Promise<ResultEnvelope<SyncResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativePerformSync(projectId)
      return this.core.parseEnvelope<SyncResult>(jsonStr)
    } catch (e) { return nativeError<SyncResult>(e) }
  }

  async syncDiagnostics(projectId: string): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSyncDiagnostics(projectId)
      return this.core.parseEnvelope<SyncDiagnosticsResult>(jsonStr)
    } catch (err) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.sync_failed', messageArgs: { 'detail': String(err) } as Record<string, string>, rawError: `syncDiagnostics failed: ${err}`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }

  // ── 应用级同步通道（Issue #600 评论 #3 问题四） ──
  // 应用级同步根 = app_data_root，同步设置/全局星图/主题调色板。
  // Core FFI 已导出应用级 sync C ABI，Harmony(NAPI) 通过这些入口接入，
  // 与作品级通道对称，但不接收 projectId — 应用级同步目标唯一。

  async loadAppSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadAppSyncConfig()
      return this.core.parseEnvelope<SyncConfig>(jsonStr)
    } catch (e) { return nativeError<SyncConfig>(e) }
  }

  async saveAppSyncConfig(_config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveAppSyncConfig(JSON.stringify(_config))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async performAppSync(): Promise<ResultEnvelope<SyncResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativePerformAppSync()
      return this.core.parseEnvelope<SyncResult>(jsonStr)
    } catch (e) { return nativeError<SyncResult>(e) }
  }

  async appSyncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeAppSyncDiagnostics()
      return this.core.parseEnvelope<SyncDiagnosticsResult>(jsonStr)
    } catch (e) { return nativeError<SyncDiagnosticsResult>(e) }
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
