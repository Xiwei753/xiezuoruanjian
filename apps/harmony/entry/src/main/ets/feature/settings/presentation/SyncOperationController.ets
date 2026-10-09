// SyncOperationController — 同步操作统一控制器。
// 确保应用内一次只运行一个同步事务，共用原生 Bridge。
// 解析必须先判 ResultEnvelope.success，再判 FullSyncResult.overallStatus，
// 且只允许明确的成功终态显示"完成"，未知值是协议错误。

import hilog from '@ohos.hilog'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { FullSyncResult, FullSyncDiagnosticsResult, FullSyncDryRunResult } from '../../../corebridge/dto/SyncDtos'
import { NativeSyncBridge } from '../../../corebridge/native/NativeSyncBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'

const TAG = 'SyncOperationController'
const DOMAIN = 0x0000

export type SyncPhase = 'idle' | 'in_progress' | 'success' | 'failed'

export interface SyncOutcome {
  phase: SyncPhase
  message: string
  errorCode?: string
  rawError?: string
}

export class SyncOperationController {
  private static instance: SyncOperationController
  private syncBridge: NativeSyncBridge
  private currentOperation: Promise<SyncOutcome> | null = null

  private constructor() {
    this.syncBridge = new NativeSyncBridge(new NativeCoreModule())
  }

  static getInstance(): SyncOperationController {
    if (!SyncOperationController.instance) {
      SyncOperationController.instance = new SyncOperationController()
    }
    return SyncOperationController.instance
  }

  /** 是否正在同步 */
  isSyncing(): boolean {
    return this.currentOperation !== null
  }

  /** 执行同步 — 一次只运行一个事务 */
  async performSync(): Promise<SyncOutcome> {
    if (this.currentOperation !== null) {
      hilog.warn(DOMAIN, TAG, 'sync already in progress, waiting for existing operation')
      return this.currentOperation
    }

    this.currentOperation = this.executeSync()
    try {
      return await this.currentOperation
    } finally {
      this.currentOperation = null
    }
  }

  private async executeSync(): Promise<SyncOutcome> {
    hilog.info(DOMAIN, TAG, 'performSync: starting')
    try {
      const result = await this.syncBridge.performSync()
      if (!result.success) {
        hilog.error(DOMAIN, TAG, 'performSync: envelope failed: errorCode=%{public}s rawError=%{public}s',
          result.errorCode ?? '', result.rawError ?? '')
        return {
          phase: 'failed',
          message: this.errorMessage(result),
          errorCode: result.errorCode,
          rawError: result.rawError
        }
      }

      const syncResult = result.data
      if (syncResult === undefined || syncResult === null) {
        hilog.error(DOMAIN, TAG, 'performSync: success but data is null')
        return { phase: 'failed', message: '同步结果数据缺失', errorCode: 'SCHEMA_MISMATCH' }
      }

      // 只允许明确的成功终态显示"完成"
      const status = syncResult.overallStatus
      if (status === 'all_synced' || status === 'no_changes') {
        hilog.info(DOMAIN, TAG, 'performSync: success, status=%{public}s', status)
        return {
          phase: 'success',
          message: status === 'no_changes' ? '同步完成，无变更' : `同步完成，下载 ${syncResult.totalDownloaded} 项`
        }
      }

      // 明确的失败终态
      if (status === 'conflict' || status === 'partial_conflict') {
        return { phase: 'failed', message: '同步冲突，需要手动解决', errorCode: 'SYNC_CONFLICT' }
      }
      if (status === 'dirty_repo_blocked') {
        return { phase: 'failed', message: '本地仓库有未提交变更，同步被阻止', errorCode: 'SYNC_DIRTY_REPO' }
      }
      if (status === 'recoverable_error' || status === 'fatal_error' || status === 'error') {
        const errorCategory = syncResult.errorCategory ?? ''
        return {
          phase: 'failed',
          message: this.syncErrorMessage(errorCategory),
          errorCode: errorCategory || 'SYNC_ERROR'
        }
      }
      if (status === 'syncing') {
        return { phase: 'failed', message: '同步仍在进行中（状态异常）', errorCode: 'SYNC_STATE_ERROR' }
      }

      // 未知值是协议错误
      hilog.error(DOMAIN, TAG, 'performSync: unknown overallStatus=%{public}s', status)
      return { phase: 'failed', message: `未知的同步状态: ${status}`, errorCode: 'SCHEMA_MISMATCH' }
    } catch (e) {
      hilog.error(DOMAIN, TAG, 'performSync: exception: %{public}s', String(e))
      return { phase: 'failed', message: `同步异常: ${e}`, rawError: String(e) }
    }
  }

  /** 执行同步诊断 */
  async runDiagnostics(): Promise<ResultEnvelope<FullSyncDiagnosticsResult>> {
    return await this.syncBridge.syncDiagnostics()
  }

  /** 执行 dry-run */
  async runDryRun(): Promise<ResultEnvelope<FullSyncDryRunResult>> {
    return await this.syncBridge.syncDryRun()
  }

  /** 从 ResultEnvelope 提取用户可读的错误消息 */
  private errorMessage(result: ResultEnvelope<Object>): string {
    if (result.messageKey && result.messageKey.length > 0) {
      // 有 i18n message key 时优先使用
      return this.mapMessageKey(result.messageKey, result.messageArgs)
    }
    if (result.userMessage && result.userMessage.length > 0) {
      return result.userMessage
    }
    if (result.errorCode) {
      return this.mapErrorCode(result.errorCode)
    }
    return '同步失败'
  }

  /** 从 errorCategory 提取用户可读的错误消息 */
  private syncErrorMessage(errorCategory: string): string {
    const messageMap: Record<string, string> = {
      'auth_failed': '认证失败，请检查 Token',
      'permission_denied': '权限不足，仓库不可访问',
      'repo_not_found': '仓库不存在',
      'network_error': '网络连接失败',
      'timeout': '同步超时',
      'dns_failed': 'DNS 解析失败',
      'config_invalid': '同步配置无效',
      'disk_error': '磁盘读写错误',
      'conflict': '同步冲突',
      'not_implemented': '同步功能未启用',
      'transport_error': '传输层错误',
      'provider_error': '同步提供者错误'
    }
    return messageMap[errorCategory] ?? `同步失败: ${errorCategory}`
  }

  /** messageKey → 中文映射 */
  private mapMessageKey(key: string, args?: Map<string, string>): string {
    const messageMap: Record<string, string> = {
      'error.sync_auth_failed': '认证失败，请检查 Token',
      'error.sync_permission_denied': '权限不足，仓库不可访问',
      'error.sync_repo_not_found': '仓库不存在',
      'error.sync_network_error': '网络连接失败',
      'error.sync_conflict': '同步冲突',
      'error.sync_config_invalid': '同步配置无效',
      'error.sync_disk_error': '磁盘读写错误',
      'error.sync_transport_error': '传输层错误',
      'error.sync_provider_error': '同步提供者错误',
      'error.not_implemented': '同步功能未启用',
      'error.sync_timeout': '同步超时',
      'error.sync_dns_failed': 'DNS 解析失败'
    }
    return messageMap[key] ?? key
  }

  /** errorCode → 中文映射 */
  private mapErrorCode(code: string): string {
    const messageMap: Record<string, string> = {
      'NOT_IMPLEMENTED': '同步功能未启用',
      'SYNC_NETWORK_ERROR': '网络连接失败',
      'SYNC_AUTH_FAILED': '认证失败，请检查 Token',
      'SYNC_PERMISSION_DENIED': '权限不足，仓库不可访问',
      'SYNC_REPO_NOT_FOUND': '仓库不存在',
      'SYNC_CONFLICT': '同步冲突',
      'SYNC_CONFIG_INVALID': '同步配置无效',
      'SYNC_DISK_ERROR': '磁盘读写错误',
      'SYNC_TRANSPORT_ERROR': '传输层错误',
      'SYNC_PROVIDER_ERROR': '同步提供者错误',
      'SYNC_STATE_ERROR': '同步状态异常',
      'SCHEMA_MISMATCH': '同步结果数据格式错误',
      'NATIVE_ERROR': '原生调用异常'
    }
    return messageMap[code] ?? `同步失败: ${code}`
  }
}
