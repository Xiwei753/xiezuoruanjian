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
import { NativeWriterCoreBridge } from '../../../corebridge/NativeWriterCoreBridge'
import { NetworkStatusService, NetworkSnapshot } from '../../../platform/network/NetworkStatusService'
import {
  ActiveWritingSessionRegistry,
  SyncPreflightError,
  SyncBarrierScope,
  SYNC_PREFLIGHT_SAVE_FAILED
} from '../../editor/session/ActiveWritingSessionRegistry'

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
  // 本次同步涉及的冲突总数（Core FullSyncResult.totalConflicts）；冲突结果带出给设置页入口用。
  totalConflicts?: number
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

  // Issue #923：同步是一个统一事务——活跃章节保存屏障 → 网络状态刷新 → Core 全量同步
  // → 按终态回填活跃会话（下载内容替换 live 正文时重开同一编辑会话）→ 事务结束 → 内容变化通知。
  // 所有手动（顶栏/设置页）与自动同步都必须经过这里（performSync 是唯一对外入口）。
  private async executeSync(): Promise<SyncOutcome> {
    hilog.info(DOMAIN, TAG, 'performSync: starting')
    let contentChanged: boolean = false
    let outcome: SyncOutcome
    try {
      outcome = await ActiveWritingSessionRegistry.getInstance().withSyncBarrier<SyncOutcome>(
        async (scope: SyncBarrierScope): Promise<SyncOutcome> => {
          // 同步前刷新网络状态，让 Core 使用当前而非启动时的网络快照。
          await this.refreshNetworkState()
          const result = await this.syncBridge.performSync()
          if (!result.success) {
            // Issue #906：HiLog 只记稳定错误码，不记 rawError（可能含 URL/Token/正文）。
            hilog.error(DOMAIN, TAG, 'performSync: envelope failed: errorCode=%{public}s',
              result.errorCode ?? 'UNKNOWN')
            return {
              phase: 'failed',
              message: result.rawError ?? result.userMessage ?? result.errorCode ?? '同步失败',
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

          // 本地可见内容（下载/覆盖/本地删除）是否实际变化。远端删除（totalRemoteDeletes）
          // 不代表当前本地作品/卷章变化，不应触发两页重载。判断依据是 FullSyncResult 的真实变更计数。
          const changed: boolean =
            syncResult.totalDownloaded > 0 ||
            syncResult.totalOverwritten > 0 ||
            syncResult.totalLocalDeletes > 0

          // Issue #923 复核：Core 全量同步按 target 逐个提交，即使整体终态是 recoverable/fatal，
          // 排在前面的 target 也可能已提交到 live。只要本次同步确实改动过本地内容，就先按落盘
          // 身份回填活跃写作会话，再组装 UI 终态——否则旧内存正文会在下一次保存中覆盖已提交版本。
          // 无本地变更时无需回填（磁盘未动，会话内容必然仍一致）。
          const aligned: boolean = changed ? await scope.reconcileAfterSync() : true
          contentChanged = changed
          if (!aligned) {
            // 活跃会话存在未提交的本地变化：明确冲突，不把远端内容悄悄覆盖成旧文本，
            // 也不在此后报告"已对齐"。
            return {
              phase: 'failed',
              message: '同步已改动本地内容，但当前写作会话存在未提交内容，需要先处理',
              messageKey: 'sync.result.conflict_summary',
              errorCode: 'SYNC_CONFLICT',
              totalConflicts: syncResult.totalConflicts,
              overallStatus: syncResult.overallStatus
            }
          }

          // 只允许明确的成功终态显示"完成"（与 Core sync_status_to_wire 对齐）
          const status = syncResult.overallStatus
          if (status === 'success' || status === 'no_changes' || status === 'latest_wins_applied') {
            hilog.info(DOMAIN, TAG, 'performSync: success, status=%{public}s', status)
            return {
              phase: 'success',
              message: `sync success: ${status}`,
              totalDownloaded: syncResult.totalDownloaded,
              totalUploaded: syncResult.totalUploaded,
              overallStatus: status
            }
          }

          if (status === 'conflict' || status === 'partial_conflict') {
            return {
              phase: 'failed',
              message: '同步冲突，需要手动解决',
              messageKey: 'sync.result.conflict_summary',
              errorCode: 'SYNC_CONFLICT',
              totalConflicts: syncResult.totalConflicts,
              overallStatus: status
            }
          }

          // 明确的失败终态
          if (status === 'recoverable_error' || status === 'fatal_error' || status === 'error') {
            // 直接透传 Core 的类型化字段，由 MessageKeyMapper 统一映射，不再手写中文字典。
            const messageKey = syncResult.messageKey ?? ''
            const errorCategory = syncResult.errorCategory ?? ''
            const errorMsg = syncResult.error ?? ''

            if (messageKey.length === 0 && errorCategory.length === 0) {
              hilog.warn(DOMAIN, TAG,
                'performSync: missing typed error fields, status=%{public}s, fallback to raw error',
                status)
            }

            return {
              phase: 'failed',
              message: errorMsg.length > 0 ? errorMsg : (messageKey.length > 0 ? messageKey : errorCategory),
              messageKey: messageKey.length > 0 ? messageKey : undefined,
              errorCategory: errorCategory.length > 0 ? errorCategory : undefined,
              errorCode: errorCategory.length > 0 ? errorCategory : 'SYNC_ERROR',
              rawError: errorMsg.length > 0 ? errorMsg : undefined,
              overallStatus: status
            }
          }
          if (status === 'syncing') {
            return { phase: 'failed', message: '同步仍在进行中（状态异常）', errorCode: 'SYNC_STATE_ERROR', overallStatus: status }
          }
          if (status === 'idle' || status === 'configured_not_tested') {
            return { phase: 'failed', message: '同步未执行（状态异常）', errorCode: 'SYNC_STATE_ERROR', overallStatus: status }
          }

          // 未知值是协议错误
          hilog.error(DOMAIN, TAG, 'performSync: unknown overallStatus=%{public}s', status)
          return { phase: 'failed', message: `未知的同步状态: ${status}`, errorCode: 'SCHEMA_MISMATCH', overallStatus: status }
        }
      )
    } catch (e) {
      if (e instanceof SyncPreflightError) {
        // 保存屏障失败：正文没有可靠落盘，禁止进入 Core 同步（不制造"假成功"）。
        hilog.error(DOMAIN, TAG, 'performSync: preflight save failed: errorCode=%{public}s', e.errorCode)
        return {
          phase: 'failed',
          message: '同步前保存当前正文失败，已取消同步',
          errorCode: e.errorCode.length > 0 ? e.errorCode : SYNC_PREFLIGHT_SAVE_FAILED
        }
      }
      // Issue #906：异常只记类型，不记 String(e)（可能含 URL/Token）。
      hilog.error(DOMAIN, TAG, 'performSync: exception: %{public}s',
        e instanceof Error ? e.name : 'Unknown')
      return { phase: 'failed', message: `同步异常: ${e}`, errorCode: 'NATIVE_ERROR', rawError: String(e) }
    }
    // 同步屏障（含活跃会话回填）已释放，再通知页面刷新，避免刷新读到未回填的旧会话状态。
    if (contentChanged) {
      this.notifyContentChanged()
    }
    return outcome
  }

  /** 同步操作前刷新 Core 的网络状态（三态；未知时不伪造在线）。 */
  private async refreshNetworkState(): Promise<void> {
    let known: boolean
    let isConnected: boolean
    let isMetered: boolean

    try {
      const snapshot: NetworkSnapshot = await NetworkStatusService.getInstance().query()
      if (snapshot.queryFailed) {
        // 查询失败：明确用 known=false 通知 Core 清除旧缓存，不伪造在线
        // Issue #906：HiLog 只记失败标记，不记 snapshot.error 原文（可能含网络细节）。
        hilog.warn(DOMAIN, TAG, 'refreshNetworkState: network query failed, queryFailed=true')
        known = false
        isConnected = false
        isMetered = false
      } else {
        known = snapshot.known
        isConnected = snapshot.isConnected
        isMetered = snapshot.isMetered
      }

      // 无论查询是否成功，都将三态写给 Core
      const result: number = NativeWriterCoreBridge.getInstance().updateNetworkState(
        known, isConnected, isMetered
      )
      if (result !== 0) {
        // Issue #906：只记阶段、返回码与三态布尔，不记查询错误原文。
        hilog.error(DOMAIN, TAG,
          'NETWORK_STATE_UPDATE_FAILED: updateNetworkState returned %{public}s, ' +
          'stage=write_core, known=%{public}s connected=%{public}s',
          result.toString(), known.toString(), isConnected.toString())
      }
    } catch (e) {
      hilog.error(DOMAIN, TAG,
        'NETWORK_STATE_UPDATE_FAILED: native bridge exception, stage=query_bridge, errorType=%{public}s',
        e instanceof Error ? e.name : 'Unknown')
    }
  }

  /** 执行同步诊断 */
  async runDiagnostics(): Promise<ResultEnvelope<FullSyncDiagnosticsResult>> {
    // 诊断结果应反映当前网络状态，而不是启动时的旧快照。
    await this.refreshNetworkState()
    return await this.syncBridge.syncDiagnostics()
  }

  /** 执行 dry-run */
  async runDryRun(): Promise<ResultEnvelope<FullSyncDryRunResult>> {
    await this.refreshNetworkState()
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
      // Issue #906：只记异常类型，不记 String(e)。
      hilog.error(DOMAIN, TAG, 'notifyContentChanged: failed to emit event, errorType=%{public}s',
        e instanceof Error ? e.name : 'Unknown')
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
}
