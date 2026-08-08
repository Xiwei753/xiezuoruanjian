import { ChangedEntity, ResultEnvelope, SyncConfig, SyncResult, SyncDiagnosticsResult } from '../model/CoreDtos'
import { NativeCoreModule } from './NativeCoreModule'

function nativeError<T>(e: unknown): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
}

function notImplemented<T>(feature: string): ResultEnvelope<T> {
  return { success: false, errorCode: 'NOT_IMPLEMENTED', messageKey: 'error.not_implemented', messageArgs: { 'feature': feature } as Record<string, string>, rawError: `${feature} not implemented`, warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
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
  // Core FFI 层尚未导出应用级 sync C ABI，这些入口暂返回 NOT_IMPLEMENTED，
  // 等 Core FFI 补充后替换为真实 native 调用。

  async loadAppSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    return notImplemented<SyncConfig>('load_app_sync_config')
  }

  async saveAppSyncConfig(_config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    return notImplemented<boolean>('save_app_sync_config')
  }

  async performAppSync(): Promise<ResultEnvelope<SyncResult>> {
    return notImplemented<SyncResult>('perform_app_sync')
  }

  async appSyncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    return notImplemented<SyncDiagnosticsResult>('app_sync_diagnostics')
  }
}
