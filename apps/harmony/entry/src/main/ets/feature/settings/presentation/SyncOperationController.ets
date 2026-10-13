// SyncOperationController — 同步操作统一控制器。
// 确保应用内一次只运行一个同步事务，共用原生 Bridge。
// 解析必须先判 ResultEnvelope.success，再判 FullSyncResult.overallStatus，
// 且只允许明确的成功终态显示"完成"，未知值是协议错误。

import hilog from '@ohos.hilog'
import emitter from '@ohos.events.emitter'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { FullSyncResult, FullSyncDiagnosticsResult, FullSyncDryRunResult } from '../../../corebridge/dto/SyncDtos'
import { NativeSyncBridge } from '../../../corebridge/native/NativeSyncBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'

// 同步完成事件 ID
export const SYNC_CONTENT_CHANGED_EVENT_ID: number = 1001

const TAG = 'SyncOperationController'
const DOMAIN = 0x0000

export type SyncPhase = 'idle' | 'in_progress' | 'success' | 'failed'

export interface SyncOutcome {
  phase: SyncPhase
  message: string
  // Core 类型化错误字段——直接透传，不做手写映射
  messageKey?: string
  messageArgs?: Record<string, string>
  errorCategory?: string
  errorCode?: string
  rawError?: string
  totalDownloaded?: number
  totalUploaded?: number
  overallStatus?: string
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
          messageKey: result.messageKey,
          messageArgs: this.mapToRecord(result.messageArgs),
          errorCode: result.errorCode,
          rawError: result.rawError
        }
      }

      const syncResult = result.data
      if (syncResult === undefined || syncResult === null) {
        hilog.error(DOMAIN, TAG, 'performSync: success but data is null')
        return { phase: 'failed', message: '同步结果数据缺失', errorCode: 'SCHEMA_MISMATCH' }
      }

      // 只允许明确的成功终态显示"完成"（与 Core sync_status_to_wire 对齐）
      const status = syncResult.overallStatus
      if (status === 'success' || status === 'no_changes' || status === 'latest_wins_applied') {
        hilog.info(DOMAIN, TAG, 'performSync: success, status=%{public}s', status)
        // 同步成功（包括有变更和无变更）时发送内容更新通知
        this.notifyContentChanged()
        return {
          phase: 'success',
          message: status === 'no_changes' ? '同步完成，无变更' :
            status === 'latest_wins_applied' ? '同步完成，最新版本已应用' :
            `同步完成，下载 ${syncResult.totalDownloaded} 项`,
          totalDownloaded: syncResult.totalDownloaded,
          totalUploaded: syncResult.totalUploaded,
          overallStatus: status
        }
      }

      // 明确的失败终态
      if (status === 'conflict' || status === 'partial_conflict') {
        return {
          phase: 'failed',
          message: '同步冲突，需要手动解决',
          errorCode: 'SYNC_CONFLICT',
          overallStatus: status
        }
      }
      if (status === 'recoverable_error' || status === 'fatal_error' || status === 'error') {
        // 优先使用 Core 返回的类型化字段，不再手写映射
        // 缺失/不识别的 category 明确回退 messageKey 和 error
        const messageKey = syncResult.messageKey ?? ''
        const errorCategory = syncResult.errorCategory ?? ''
        const errorMsg = syncResult.error ?? ''

        // 技术细节写日志，用户给对应中文原因
        if (messageKey.length === 0 && errorCategory.length === 0) {
          hilog.warn(DOMAIN, TAG,
            'performSync: missing typed error fields, status=%{public}s, fallback to raw error',
            status)
        }

        let userMessage: string
        if (messageKey.length > 0) {
          // 有 messageKey 时，由 i18n 系统处理，这里只透传 key
          // 暂时用 fallback 映射作为临时中文显示（后续应接入 i18n）
          userMessage = this.errorKeyToChinese(messageKey) || errorMsg || '同步失败'
        } else if (errorCategory.length > 0) {
          userMessage = this.errorCategoryToChinese(errorCategory) || errorMsg || '同步失败'
        } else {
          userMessage = errorMsg || '同步失败'
        }

        return {
          phase: 'failed',
          message: userMessage,
          messageKey: messageKey.length > 0 ? messageKey : undefined,
          errorCategory: errorCategory.length > 0 ? errorCategory : undefined,
          errorCode: errorCategory || 'SYNC_ERROR',
          rawError: errorMsg.length > 0 ? errorMsg : undefined,
          overallStatus: status
        }
      }
      if (status === 'syncing') {
        return { phase: 'failed', message: '同步仍在进行中（状态异常）', errorCode: 'SYNC_STATE_ERROR' }
      }
      if (status === 'idle' || status === 'configured_not_tested') {
        return { phase: 'failed', message: '同步未执行（状态异常）', errorCode: 'SYNC_STATE_ERROR' }
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

  /**
   * 发送内容更新通知
   * 同步成功后通知页面刷新数据
   */
  private notifyContentChanged(): void {
    try {
      const eventData: emitter.EventData = {
        data: {
          timestamp: Date.now()
        }
      }
      const event: emitter.InnerEvent = {
        eventId: SYNC_CONTENT_CHANGED_EVENT_ID
      }
      emitter.emit(event, eventData)
      hilog.info(DOMAIN, TAG, 'notifyContentChanged: content changed event emitted')
    } catch (e) {
      hilog.error(DOMAIN, TAG, 'notifyContentChanged: failed to emit event, %{public}s', String(e))
    }
  }

  /** 将 Map<string, string> 转换为 Record<string, string>（JSON.stringify 不支持 Map） */
  private mapToRecord(map?: Map<string, string>): Record<string, string> | undefined {
    if (!map || map.size === 0) {
      return undefined
    }
    const record: Record<string, string> = {}
    map.forEach((value: string, key: string) => {
      record[key] = value
    })
    return record
  }

  /** 从 ResultEnvelope 提取用户可读的错误消息 */
  private errorMessage(result: ResultEnvelope<Object>): string {
    // 直接透传 Core 类型化值，不再手写映射
    // messageKey 是 i18n key，由上层 UI 做本地化
    if (result.messageKey && result.messageKey.length > 0) {
      // 暂时用 fallback 映射作为临时中文显示（后续应接入 i18n）
      return this.errorKeyToChinese(result.messageKey) || result.messageKey
    }
    if (result.userMessage && result.userMessage.length > 0) {
      return result.userMessage
    }
    if (result.errorCode) {
      return this.errorCodeToChinese(result.errorCode) || result.errorCode
    }
    return '同步失败'
  }

  /** messageKey → 中文临时映射（后续应接入 i18n 系统） */
  private errorKeyToChinese(key: string): string {
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
      'error.sync_dns_failed': 'DNS 解析失败',
      'error.sync_retryable_network': '网络连接失败，可重试',
      'error.sync_retryable_io': '磁盘读写错误，可重试',
      'error.sync_failed': '同步失败',
      'error.sync_fatal': '同步失败',
      'error.sync_dirty_repository': '本地仓库有未提交的修改',
      'error.sync_protocol_error': '同步协议错误',
      'error.io': 'IO 错误',
      'error.json': 'JSON 解析错误',
      'error.project_not_found': '项目不存在',
      'error.volume_not_found': '卷不存在',
      'error.chapter_not_found': '章节不存在',
      'error.empty_overwrite_blocked': '空内容覆盖被阻止',
      'error.refuse_delete_root': '拒绝删除根目录',
      'error.invalid_delete_target': '无效的删除目标',
      'error.other': '未知错误'
    }
    return messageMap[key] ?? ''
  }

  /** errorCategory → 中文临时映射（后续应接入 i18n 系统） */
  private errorCategoryToChinese(category: string): string {
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
    return messageMap[category] ?? ''
  }

  /** errorCode → 中文临时映射（后续应接入 i18n 系统） */
  private errorCodeToChinese(code: string): string {
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
      'NATIVE_ERROR': '原生调用异常',
      'RETRYABLE_NETWORK': '网络连接失败，可重试',
      'RETRYABLE_IO': '磁盘读写错误，可重试',
      'AUTHENTICATION': '认证失败，请检查 Token',
      'CONFLICT': '同步冲突',
      'DIRTY_REPOSITORY': '本地仓库有未提交的修改',
      'PROTOCOL': '同步协议错误',
      'FATAL': '同步失败',
      'SYNC_FAILED': '同步失败',
      'IO_ERROR': 'IO 错误',
      'JSON_ERROR': 'JSON 解析错误',
      'PROJECT_NOT_FOUND': '项目不存在',
      'VOLUME_NOT_FOUND': '卷不存在',
      'CHAPTER_NOT_FOUND': '章节不存在',
      'EMPTY_OVERWRITE_BLOCKED': '空内容覆盖被阻止',
      'REFUSE_DELETE_ROOT': '拒绝删除根目录',
      'INVALID_DELETE_TARGET': '无效的删除目标',
      'OTHER': '未知错误'
    }
    return messageMap[code] ?? ''
  }
}
