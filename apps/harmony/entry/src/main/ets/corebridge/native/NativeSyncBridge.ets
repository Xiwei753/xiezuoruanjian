import { ResultEnvelope } from '../dto/ResultEnvelope'
import { FullSyncDiagnosticsResult, FullSyncDryRunResult, FullSyncResult, SyncConfig, SyncSecrets, SyncState, FullSyncState, FullSyncReport, AllSyncConflictEntry, SyncConflictPreview } from '../dto/SyncDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { decodeSyncConfig, decodeSyncState, decodeFullSyncResult, decodeFullSyncDryRunResult, decodeFullSyncDiagnosticsResult, decodeSyncSecrets, decodeFullSyncState, decodeFullSyncReport, decodeAllSyncConflictEntry, decodeSyncConflictPreview, decodeArray } from '../codec/CoreWireDecoders'

/**
 * 构造 NATIVE_ERROR 信封，仅用于真正的 JS 异常（如 native 模块不存在）。
 *
 * 业务错误信封（success:false + errorCode）由 decodeEnvelope/parseEnvelope 正常解析，
 * 不要在这里改写，否则会丢失原始 errorCode（如 ASYNC_QUEUE_FAILED）。
 */
function nativeError<T>(e: Object): ResultEnvelope<T> {
  return {
    success: false,
    errorCode: 'NATIVE_ERROR',
    userMessage: String(e),
    rawError: String(e),
    messageKey: undefined,
    messageArgs: undefined
  }
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
      const jsonStr: string = await (this.core.getNativeModule().nativeSyncDryRun() as Promise<string>)
      // nativeSyncDryRun() 现在 resolve 标准错误信封，decodeEnvelope 会正确解析。
      // catch 只处理真正的 JS 异常。
      return this.core.decodeEnvelope<FullSyncDryRunResult>(jsonStr, decodeFullSyncDryRunResult)
    } catch (e) { return nativeError<FullSyncDryRunResult>(e) }
  }

  async performSync(): Promise<ResultEnvelope<FullSyncResult>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativePerformSync() as Promise<string>)
      // nativePerformSync() 现在 resolve 标准错误信封（包括 success:false），
      // decodeEnvelope 会正确解析并透传 errorCode/userMessage（如 ASYNC_QUEUE_FAILED）。
      // catch 只处理真正的 JS 异常（如 native 模块不存在）。
      return this.core.decodeEnvelope<FullSyncResult>(jsonStr, decodeFullSyncResult)
    } catch (e) { return nativeError<FullSyncResult>(e) }
  }

  async syncDiagnostics(): Promise<ResultEnvelope<FullSyncDiagnosticsResult>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativeSyncDiagnostics() as Promise<string>)
      return this.core.decodeEnvelope<FullSyncDiagnosticsResult>(jsonStr, decodeFullSyncDiagnosticsResult)
    } catch (err) {
      return nativeError<FullSyncDiagnosticsResult>(err)
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

  // ── 全量同步持久状态 / 冲突查询与解决（Issue #925 / #926） ──

  /**
   * 读取 Core 持久化的全量同步状态（full_state.local.json）。
   *
   * `data` 为 null 表示 Core 尚未写入任何同步结果（从未成功同步），是合法状态，
   * 不是错误；成功与否由 success 区分，`last_success_time` 为 Unix 秒。
   */
  async loadFullSyncState(): Promise<ResultEnvelope<FullSyncState | null>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativeLoadFullSyncState() as Promise<string>)
      // Core `Option<FullSyncStateDto>` 用 data:null 表达"从未写入状态"；
      // decodeEnvelope 会把 null 归为 SCHEMA_MISMATCH，所以先按空状态返回。
      const raw: ResultEnvelope<Object> = this.core.parseRawEnvelope(jsonStr)
      if (raw.success && (raw.data === undefined || raw.data === null)) {
        return { success: true, data: null }
      }
      const decoded: ResultEnvelope<FullSyncState> = this.core.decodeEnvelope<FullSyncState>(jsonStr, decodeFullSyncState)
      if (!decoded.success) {
        return {
          success: false,
          errorCode: decoded.errorCode,
          userMessage: decoded.userMessage,
          messageKey: decoded.messageKey,
          messageArgs: decoded.messageArgs,
          rawError: decoded.rawError,
          diagnosticOperationId: decoded.diagnosticOperationId,
          failureStage: decoded.failureStage,
          failureCategory: decoded.failureCategory,
          warnings: decoded.warnings,
          changedPaths: decoded.changedPaths,
          changedEntities: decoded.changedEntities
        }
      }
      return { success: true, data: decoded.data ?? null }
    } catch (e) { return nativeError<FullSyncState | null>(e) }
  }

  /** 读取 Core 保存的最近一次完整同步报告；data:null 表示尚无报告。 */
  async loadLatestFullSyncReport(): Promise<ResultEnvelope<FullSyncReport | null>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule()
        .nativeLoadLatestFullSyncReport() as Promise<string>)
      const raw: ResultEnvelope<Object> = this.core.parseRawEnvelope(jsonStr)
      if (raw.success && (raw.data === undefined || raw.data === null)) {
        return { success: true, data: null }
      }
      const decoded: ResultEnvelope<FullSyncReport> =
        this.core.decodeEnvelope<FullSyncReport>(jsonStr, decodeFullSyncReport)
      if (!decoded.success) {
        return {
          success: false,
          errorCode: decoded.errorCode,
          userMessage: decoded.userMessage,
          messageKey: decoded.messageKey,
          messageArgs: decoded.messageArgs,
          rawError: decoded.rawError,
          diagnosticOperationId: decoded.diagnosticOperationId,
          failureStage: decoded.failureStage,
          failureCategory: decoded.failureCategory,
          warnings: decoded.warnings,
          changedPaths: decoded.changedPaths,
          changedEntities: decoded.changedEntities
        }
      }
      return { success: true, data: decoded.data ?? null }
    } catch (e) { return nativeError<FullSyncReport | null>(e) }
  }

  /** 列出 App target + 所有 Project target 的未解决冲突摘要（不含正文）。 */
  async listAllSyncConflicts(): Promise<ResultEnvelope<AllSyncConflictEntry[]>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativeListAllSyncConflicts() as Promise<string>)
      return this.core.decodeEnvelope<AllSyncConflictEntry[]>(jsonStr,
        (d: Object) => decodeArray<AllSyncConflictEntry>(d, decodeAllSyncConflictEntry, 'AllSyncConflictEntry'))
    } catch (e) { return nativeError<AllSyncConflictEntry[]>(e) }
  }

  /**
   * 读取单条冲突的本地/远端内容预览。
   *
   * `targetKind` 为 "app" / "project"；App target 传 projectId = null。
   */
  async loadSyncConflictPreview(targetKind: string, projectId: string | null, path: string): Promise<ResultEnvelope<SyncConflictPreview>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativeLoadSyncConflictPreview(targetKind, projectId, path) as Promise<string>)
      return this.core.decodeEnvelope<SyncConflictPreview>(jsonStr, decodeSyncConflictPreview)
    } catch (e) { return nativeError<SyncConflictPreview>(e) }
  }

  /**
   * 按用户决定解决一条冲突。
   *
   * `decision` 只接受 "keep_local" / "take_remote" / "mark_merged"；
   * 返回值透传 Core：take_remote 的 false 表示仅排队 pending（未立即替换 live 正文）。
   */
  async resolveSyncConflict(targetKind: string, projectId: string | null, path: string, decision: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = await (this.core.getNativeModule().nativeResolveSyncConflict(targetKind, projectId, path, decision) as Promise<string>)
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) { return nativeError<boolean>(e) }
  }
}
