import { ResultEnvelope } from '../dto/ResultEnvelope'
import { FullSyncDiagnosticsResult, FullSyncDryRunResult, FullSyncResult, SyncConfig, SyncSecrets, SyncState } from '../dto/SyncDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeSyncConfig, decodeSyncState, decodeFullSyncResult, decodeFullSyncDryRunResult, decodeFullSyncDiagnosticsResult, decodeSyncSecrets } from '../codec/CoreWireDecoders'

/**
 * 构造 NATIVE_ERROR 信封，仅用于真正的 JS 异常（如 native 模块不存在）。
 *
 * 业务错误信封（success:false + errorCode）由 decodeEnvelope/parseEnvelope 正常解析，
 * 不要在这里改写，否则会丢失原始 errorCode（如 ASYNC_QUEUE_FAILED）。
 */
function nativeError<T>(e: Object): ResultEnvelope<T> {
  return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e), rawError: String(e) }
}

export class NativeSyncBridge {
  private core: NativeCoreModule

  constructor(core: NativeCoreModule) {
    this.core = core
  }

  // ── 全量同步统一入口（Issue #630） ──
  // 一个全局 SyncConfig + 一份全局凭据，App target + 所有 Project target 一次同步。
  // 旧的 per-project / app-level 双套入口已删除。

  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadSyncConfig()
      // nativeLoadSyncConfig() 现在 resolve 标准错误信封（包括 success:false），
      // decodeEnvelope 会正确解析并透传 errorCode/userMessage。
      // catch 只处理真正的 JS 异常（如 native 模块不存在）。
      return this.core.decodeEnvelope<SyncConfig>(jsonStr, decodeSyncConfig)
    } catch (e) { return nativeError<SyncConfig>(e) }
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveSyncConfig(JSON.stringify(config))
      // nativeSaveSyncConfig() 现在 resolve 标准错误信封，parseEnvelope 会正确解析。
      // catch 只处理真正的 JS 异常。
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async loadSyncSecrets(): Promise<ResultEnvelope<SyncSecrets>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadSyncSecrets()
      // nativeLoadSyncSecrets() 现在 resolve 标准错误信封，decodeEnvelope 会正确解析。
      // catch 只处理真正的 JS 异常。
      return this.core.decodeEnvelope<SyncSecrets>(jsonStr, decodeSyncSecrets)
    } catch (e) { return nativeError<SyncSecrets>(e) }
  }

  async saveSyncSecrets(secrets: SyncSecrets): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveSyncSecrets(JSON.stringify(secrets))
      // nativeSaveSyncSecrets() 现在 resolve 标准错误信封，parseEnvelope 会正确解析。
      // catch 只处理真正的 JS 异常。
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }

  async syncDryRun(): Promise<ResultEnvelope<FullSyncDryRunResult>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeSyncDryRun()
      // nativeSyncDryRun() 现在 resolve 标准错误信封，decodeEnvelope 会正确解析。
      // catch 只处理真正的 JS 异常。
      return this.core.decodeEnvelope<FullSyncDryRunResult>(jsonStr, decodeFullSyncDryRunResult)
    } catch (e) { return nativeError<FullSyncDryRunResult>(e) }
  }

  async performSync(): Promise<ResultEnvelope<FullSyncResult>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativePerformSync()
      // nativePerformSync() 现在 resolve 标准错误信封（包括 success:false），
      // decodeEnvelope 会正确解析并透传 errorCode/userMessage（如 ASYNC_QUEUE_FAILED）。
      // catch 只处理真正的 JS 异常（如 native 模块不存在）。
      return this.core.decodeEnvelope<FullSyncResult>(jsonStr, decodeFullSyncResult)
    } catch (e) { return nativeError<FullSyncResult>(e) }
  }

  async syncDiagnostics(): Promise<ResultEnvelope<FullSyncDiagnosticsResult>> {
    try {
      const jsonStr: string = await this.core.getNativeModule().nativeSyncDiagnostics()
      return this.core.decodeEnvelope<FullSyncDiagnosticsResult>(jsonStr, decodeFullSyncDiagnosticsResult)
    } catch (err) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: `syncDiagnostics failed: ${err}`, rawError: String(err) }
    }
  }

  // ── 应用级同步状态（Issue #600 评论 #5） ──
  // 路径：<app_data_root>/app-meta/sync/state.local.json

  async loadAppSyncState(): Promise<ResultEnvelope<SyncState>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeLoadAppSyncState()
      return this.core.decodeEnvelope<SyncState>(jsonStr, decodeSyncState)
    } catch (e) { return nativeError<SyncState>(e) }
  }

  async saveAppSyncState(state: SyncState): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeSaveAppSyncState(JSON.stringify(state))
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }
}
