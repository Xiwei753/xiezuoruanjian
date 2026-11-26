// SyncOperationController — 同步操作统一控制器。
// 确保应用内一次只运行一个同步事务，共用原生 Bridge。
// 解析必须先判 ResultEnvelope.success，再判 FullSyncResult.overallStatus，
// 且只允许明确的成功终态显示"完成"，未知值是协议错误。

import hilog from '@ohos.hilog'
import emitter from '@ohos.events.emitter'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { FullSyncResult, FullSyncDiagnosticsResult, FullSyncDryRunResult, SyncConfig } from '../../../corebridge/dto/SyncDtos'
import { NativeSyncBridge } from '../../../corebridge/native/NativeSyncBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { NativeWriterCoreBridge } from '../../../corebridge/NativeWriterCoreBridge'
import { NetworkStatusService, NetworkSnapshot } from '../../../platform/network/NetworkStatusService'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../../../platform/diagnostics/HarmonyDiagnosticEvents'
import { SyncOutcome, SyncTriggerReason, bypassSyncDebounce, isHandoffTrigger } from './SyncRunTypes'
import { SyncEntrySource, SyncRunPresentationState } from './SyncRunPresentationState'
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

// 运行中任务期间排队的一个后续同步（Issue #1058）。字段用函数类型属性而不是方法签名，
// 与 ActiveWritingSessionRegistry 的 SyncBarrierScope 同样的 ArkTS 字面量限制。
interface SyncFollowUpRequest {
  readonly trigger: SyncTriggerReason
  readonly promise: Promise<SyncOutcome>
  readonly resolve: (outcome: SyncOutcome) => void
}

export class SyncOperationController {
  private static instance: SyncOperationController
  private syncBridge: NativeSyncBridge
  private currentOperation: Promise<SyncOutcome> | null = null
  /**
   * 运行中任务期间至多一个后续同步请求（Issue #1058）。
   * 退后台保存发生在运行中任务已越过保存/快照边界之后时，新正文不会被旧运行任务带上远端；
   * 这里不把调用方简单挂到旧 Promise 上，而是在旧任务结束后再走一次完整 performSync
   * （重新经过 ActiveWritingSessionRegistry.withSyncBarrier 保存屏障）。多个交接请求合并为一个。
   */
  private pendingFollowUp: SyncFollowUpRequest | null = null

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

  /** Issue #952：当前运行中任务的来源；没有运行任务时为 null。
   *  手动入口据此判断本次点击是否合并进后台自动任务，从而决定是否自行给出“进行中/结果”反馈。 */
  currentRunningSource(): SyncEntrySource | null {
    if (this.currentOperation === null) {
      return null
    }
    return SyncRunPresentationState.getInstance().snapshot().source
  }

  /**
   * 执行同步 — 一次只运行一个事务；开始快照先于保存屏障和 NAPI。
   *
   * 触发原因（Issue #1058）：manual/auto_timer 维持原有「运行中则合并到当前任务」语义；
   * 交接类触发（startup / foreground_handoff / background_handoff / handoff_retry）在运行中时
   * 不并入旧任务——旧任务可能已越过保存/快照边界，退后台的新保存不会被它上传——
   * 而是合并为至多一个后续同步，等旧任务结束后重新经过保存屏障再执行。
   */
  performSync(source: SyncEntrySource = 'manual',
    trigger: SyncTriggerReason = 'manual'): Promise<SyncOutcome> {
    if (this.currentOperation !== null) {
      if (!isHandoffTrigger(trigger)) {
        hilog.warn(DOMAIN, TAG,
          'sync already in progress, waiting for existing operation: trigger=%{public}s', trigger)
        return this.currentOperation
      }
      return this.queueHandoffFollowUp(trigger)
    }
    const startedAtMs: number = Date.now()
    const state: SyncRunPresentationState = SyncRunPresentationState.getInstance()
    let resolveTrackedOperation: ((outcome: SyncOutcome) => void) | null = null
    const trackedOperation: Promise<SyncOutcome> = new Promise<SyncOutcome>((resolve: (outcome: SyncOutcome) => void) => {
      resolveTrackedOperation = resolve
    })
    this.currentOperation = trackedOperation
    state.begin(source)
    hilog.info(DOMAIN, TAG, 'performSync: starting source=%{public}s trigger=%{public}s', source, trigger)
    this.recordSyncStarted(source, trigger)

    const execution: Promise<SyncOutcome> = this.executeSync(startedAtMs, trigger).catch((e: Error): SyncOutcome => {
      // executeSync 已覆盖预检和 NAPI 路径；这里兜住完成通知等外围意外，仍产出唯一终态。
      const errorType: string = e instanceof Error ? e.name : 'Unknown'
      hilog.error(DOMAIN, TAG,
        'performSync: unhandled operation error: errorType=%{public}s failureStage=arkts_bridge ' +
        'failureCategory=native_bridge_exception durationMs=%{public}s',
        errorType, String(Math.max(0, Date.now() - startedAtMs)))
      return {
        phase: 'failed',
        message: '同步异常',
        errorCode: 'NATIVE_ERROR',
        failureStage: 'arkts_bridge',
        errorCategory: 'native_bridge_exception'
      }
    })
    execution.then((outcome: SyncOutcome): void => {
      try {
        try {
          state.finish(outcome)
        } catch (_) {
          hilog.error(DOMAIN, TAG, 'performSync: failed to publish terminal snapshot')
        }
        try {
          this.recordSyncFinished(source, trigger, startedAtMs, outcome)
        } catch (_) {
          hilog.error(DOMAIN, TAG, 'performSync: failed to record terminal event')
        }
      } finally {
        if (this.currentOperation === trackedOperation) {
          this.currentOperation = null
        }
        if (resolveTrackedOperation !== null) {
          resolveTrackedOperation(outcome)
        }
        this.startPendingFollowUp().catch((e: Error): void => {
          hilog.error(DOMAIN, TAG, 'follow-up sync start failed: errorType=%{public}s', e.name)
        })
      }
    })
    return trackedOperation
  }

  /**
   * 运行中任务期间收到交接请求：合并成至多一个后续同步并返回它的 Promise。
   * 后续同步会重新进入 performSync → withSyncBarrier，绝不沿用旧任务的保存/快照边界。
   */
  private queueHandoffFollowUp(trigger: SyncTriggerReason): Promise<SyncOutcome> {
    const pending: SyncFollowUpRequest | null = this.pendingFollowUp
    if (pending !== null) {
      hilog.info(DOMAIN, TAG,
        'follow-up sync already queued, merging trigger=%{public}s into trigger=%{public}s',
        trigger, pending.trigger)
      return pending.promise
    }
    let resolveFollowUp: ((outcome: SyncOutcome) => void) | null = null
    const promise: Promise<SyncOutcome> = new Promise<SyncOutcome>((resolve: (outcome: SyncOutcome) => void): void => {
      resolveFollowUp = resolve
    })
    const request: SyncFollowUpRequest = {
      trigger: trigger,
      promise: promise,
      resolve: (outcome: SyncOutcome): void => {
        if (resolveFollowUp !== null) {
          resolveFollowUp(outcome)
        }
      }
    }
    this.pendingFollowUp = request
    hilog.info(DOMAIN, TAG,
      'running sync may have passed its save boundary; queued one follow-up sync trigger=%{public}s', trigger)
    return promise
  }

  /** 当前任务结束后启动排队的唯一后续同步；没有排队请求时什么也不做。 */
  private async startPendingFollowUp(): Promise<void> {
    const pending: SyncFollowUpRequest | null = this.pendingFollowUp
    if (pending === null) {
      return
    }
    this.pendingFollowUp = null
    // Issue #1058 复审：真正启动排队的自动后续任务前，重新读取 Core 已提交配置；
    // 用户在运行期间关闭自动同步后，不再发起已排队的自动任务。
    // 读取失败按"未知"处理并继续，不能把"读不到配置"当成"已关闭"。
    const autoSyncActive: boolean | null = await this.committedAutoSyncActive()
    if (autoSyncActive === false) {
      hilog.info(DOMAIN, TAG,
        'queued follow-up skipped: auto sync disabled in committed config trigger=%{public}s',
        pending.trigger)
      pending.resolve(SyncOperationController.followUpSkippedOutcome())
      return
    }
    hilog.info(DOMAIN, TAG, 'starting queued follow-up sync trigger=%{public}s', pending.trigger)
    try {
      const followUp: Promise<SyncOutcome> = this.performSync('auto', pending.trigger)
      followUp.then((outcome: SyncOutcome): void => {
        pending.resolve(outcome)
      })
    } catch (e) {
      // performSync 正常不抛；这里兜住启动期意外，仍给排队调用方一个唯一终态，
      // 避免后台交接事务一直等一个永远不会 resolve 的 Promise。
      const errorType: string = e instanceof Error ? e.name : 'Unknown'
      hilog.error(DOMAIN, TAG, 'follow-up sync failed to start: errorType=%{public}s', errorType)
      pending.resolve(SyncOperationController.startFailureOutcome())
    }
  }

  /**
   * 读取 Core 已提交的自动同步开关。
   * `false` = 已确认关闭；`true` = 已确认开启；`null` = 读取失败/未知（不能当成关闭）。
   */
  private async committedAutoSyncActive(): Promise<boolean | null> {
    try {
      const result: ResultEnvelope<SyncConfig> = await this.syncBridge.loadSyncConfig()
      if (!result.success) {
        hilog.warn(DOMAIN, TAG, 'loadSyncConfig failed before follow-up: errorCode=%{public}s',
          result.errorCode ?? 'UNKNOWN')
        return null
      }
      const config: SyncConfig | null = result.data ?? null
      return config !== null && config.enabled && config.autoSync
    } catch (e) {
      hilog.warn(DOMAIN, TAG, 'loadSyncConfig threw before follow-up: errorType=%{public}s',
        e instanceof Error ? e.name : 'Unknown')
      return null
    }
  }

  /** 排队的后续同步因自动同步已关闭而未启动时的唯一终态（不假报成功）。 */
  private static followUpSkippedOutcome(): SyncOutcome {
    return {
      phase: 'failed',
      message: '自动同步已关闭，跳过排队的后续同步',
      errorCode: 'AUTO_SYNC_DISABLED',
      failureStage: 'config_gate',
      errorCategory: 'auto_sync_disabled'
    }
  }

  /** 后续同步未能启动时的唯一失败终态（与 performSync 的外围异常终态一致）。 */
  private static startFailureOutcome(): SyncOutcome {
    return {
      phase: 'failed',
      message: '同步异常',
      errorCode: 'NATIVE_ERROR',
      failureStage: 'arkts_bridge',
      errorCategory: 'native_bridge_exception'
    }
  }

  private recordSyncStarted(source: SyncEntrySource, trigger: SyncTriggerReason): void {
    const fields: DiagnosticField[] = [
      { key: 'source', value: source },
      { key: 'trigger', value: trigger },
      { key: 'phase', value: 'in_progress' },
      { key: 'overallStatus', value: '' },
      { key: 'errorCode', value: '' },
      { key: 'operationId', value: '' },
      { key: 'diagnosticOperationId', value: '' },
      { key: 'failureStage', value: '' },
      { key: 'errorCategory', value: '' },
      { key: 'failureCategory', value: '' },
      { key: 'durationMs', value: '0' }
    ]
    HarmonyDiagnosticEvents.record('Info', source === 'manual' ? 'User' : 'App',
      'sync.started', 'SyncOperationController', fields)
  }

  private recordSyncFinished(source: SyncEntrySource, trigger: SyncTriggerReason, startedAtMs: number,
    outcome: SyncOutcome): void {
    const fields: DiagnosticField[] = [
      { key: 'source', value: source },
      { key: 'trigger', value: trigger },
      { key: 'phase', value: outcome.phase },
      { key: 'overallStatus', value: outcome.overallStatus ?? '' },
      { key: 'errorCode', value: outcome.errorCode ?? '' },
      { key: 'operationId', value: outcome.operationId ?? '' },
      { key: 'diagnosticOperationId', value: outcome.operationId ?? '' },
      { key: 'failureStage', value: outcome.failureStage ?? '' },
      { key: 'errorCategory', value: outcome.errorCategory ?? '' },
      { key: 'failureCategory', value: outcome.errorCategory ?? '' },
      { key: 'durationMs', value: String(Math.max(0, Date.now() - startedAtMs)) }
    ]
    const level = outcome.phase === 'success' ? 'Info' : 'Warn'
    HarmonyDiagnosticEvents.record(level, source === 'manual' ? 'User' : 'App',
      'sync.finished', 'SyncOperationController', fields)
  }

  // Issue #923：同步是一个统一事务——活跃章节保存屏障 → 网络状态刷新 → Core 全量同步
  // → 按终态回填活跃会话（下载内容替换 live 正文时重开同一编辑会话）→ 事务结束 → 内容变化通知。
  // 所有手动（顶栏/设置页）与自动同步都必须经过这里（performSync 是唯一对外入口）。
  private async executeSync(startedAtMs: number, trigger: SyncTriggerReason): Promise<SyncOutcome> {
    hilog.info(DOMAIN, TAG, 'performSync: starting trigger=%{public}s', trigger)
    let contentChanged: boolean = false
    let outcome: SyncOutcome
    try {
      outcome = await ActiveWritingSessionRegistry.getInstance().withSyncBarrier<SyncOutcome>(
        async (scope: SyncBarrierScope): Promise<SyncOutcome> => {
          // 同步前刷新网络状态，让 Core 使用当前而非启动时的网络快照。
          await this.refreshNetworkState()
          // Issue #1058 / #1065：把触发原因传给 Core 诊断，并按触发类型决定是否
          // 只跳过 Core 的调度防抖；不改变对象级 LWW 与正文三路比较（force_sync
          // 语义仅"跳过防抖窗口"）。用户手动入口与交接类触发跳过防抖，周期 timer
          // 与 durable_commit 唤醒由调度器/间隔合并决定，仍走 Core 防抖。
          const forceSync: boolean = bypassSyncDebounce(trigger)
          const result = await this.syncBridge.performSync(trigger, forceSync)
          if (!result.success) {
            // Issue #1017：失败必须带稳定阶段/分类/operationId/耗时，HiLog 不记 rawError。
            const failureStage: string = result.failureStage ?? 'core_sync'
            const failureCategory: string = result.failureCategory ?? 'sync_error'
            const durationMs: number = Math.max(0, Date.now() - startedAtMs)
            hilog.error(DOMAIN, TAG,
              'performSync: envelope failed: errorCode=%{public}s failureStage=%{public}s ' +
              'failureCategory=%{public}s diagnosticOperationId=%{public}s durationMs=%{public}s',
              result.errorCode ?? 'UNKNOWN', failureStage, failureCategory,
              result.diagnosticOperationId ?? '', String(durationMs))
            return {
              phase: 'failed',
              message: this.safeSyncFailureMessage(failureStage, failureCategory,
                result.errorCode),
              messageKey: result.messageKey,
              messageArgs: this.mapToRecord(result.messageArgs),
              operationId: result.diagnosticOperationId,
              failureStage: failureStage,
              errorCategory: failureCategory,
              errorCode: result.errorCode,
              rawError: result.rawError
            }
          }

          const syncResult = result.data
          if (syncResult === undefined || syncResult === null) {
            const durationMs: number = Math.max(0, Date.now() - startedAtMs)
            hilog.error(DOMAIN, TAG,
              'performSync: success but data is null: errorCode=SCHEMA_MISMATCH failureStage=core_sync ' +
              'failureCategory=schema_mismatch diagnosticOperationId=%{public}s durationMs=%{public}s',
              result.diagnosticOperationId ?? '', String(durationMs))
            return {
              phase: 'failed', message: '同步结果数据缺失', errorCode: 'SCHEMA_MISMATCH',
              operationId: result.diagnosticOperationId,
              failureStage: 'core_sync', errorCategory: 'schema_mismatch'
            }
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
            const durationMs: number = Math.max(0, Date.now() - startedAtMs)
            hilog.warn(DOMAIN, TAG,
              'performSync: session unaligned: errorCode=SYNC_SESSION_UNALIGNED ' +
              'failureStage=session_reconcile failureCategory=session_unaligned ' +
              'diagnosticOperationId=%{public}s durationMs=%{public}s',
              result.diagnosticOperationId ?? '', String(durationMs))
            return {
              phase: 'failed',
              message: '同步已改动本地内容，但当前写作会话存在未提交内容，需要先处理',
              messageKey: 'sync.result.session_unaligned',
              errorCode: 'SYNC_SESSION_UNALIGNED',
              operationId: result.diagnosticOperationId,
              failureStage: 'session_reconcile',
              errorCategory: 'session_unaligned',
              totalDownloaded: syncResult.totalDownloaded,
              totalUploaded: syncResult.totalUploaded,
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
              operationId: result.diagnosticOperationId,
              totalDownloaded: syncResult.totalDownloaded,
              totalUploaded: syncResult.totalUploaded,
              overallStatus: status
            }
          }

          if (status === 'conflict' || status === 'partial_conflict') {
            const failureStage: string = result.failureStage ?? syncResult.failureStage ?? 'core_sync'
            const failureCategory: string = result.failureCategory ?? 'conflict'
            const durationMs: number = Math.max(0, Date.now() - startedAtMs)
            hilog.warn(DOMAIN, TAG,
              'performSync: conflict: status=%{public}s failureStage=%{public}s ' +
              'failureCategory=%{public}s diagnosticOperationId=%{public}s durationMs=%{public}s',
              status, failureStage, failureCategory, result.diagnosticOperationId ?? '',
              String(durationMs))
            return {
              phase: 'failed',
              message: '同步冲突，需要手动解决',
              messageKey: 'sync.result.conflict_summary',
              errorCode: 'SYNC_CONFLICT',
              operationId: result.diagnosticOperationId,
              failureStage: failureStage,
              errorCategory: failureCategory,
              totalDownloaded: syncResult.totalDownloaded,
              totalUploaded: syncResult.totalUploaded,
              totalConflicts: syncResult.totalConflicts,
              overallStatus: status
            }
          }

          // 明确的失败终态
          if (status === 'recoverable_error' || status === 'fatal_error' || status === 'error') {
            // 直接透传 Core 的类型化字段，由 MessageKeyMapper 统一映射，不再手写中文字典。
            const messageKey = syncResult.messageKey ?? ''
            const errorCategory = result.failureCategory ?? syncResult.errorCategory ?? ''
            const failureStage = result.failureStage ?? syncResult.failureStage ?? 'core_sync'
            const stableCategory = errorCategory.length > 0 ? errorCategory : 'sync_error'
            const errorCode = errorCategory.length > 0 ? errorCategory : 'SYNC_ERROR'
            const errorMsg = syncResult.error ?? ''
            const durationMs: number = Math.max(0, Date.now() - startedAtMs)

            hilog.error(DOMAIN, TAG,
              'performSync: terminal failure: status=%{public}s errorCode=%{public}s ' +
              'failureStage=%{public}s failureCategory=%{public}s diagnosticOperationId=%{public}s ' +
              'durationMs=%{public}s',
              status, errorCode, failureStage, stableCategory,
              result.diagnosticOperationId ?? '', String(durationMs))
            if (messageKey.length === 0 && errorCategory.length === 0) {
              hilog.warn(DOMAIN, TAG,
                'performSync: missing typed error fields, status=%{public}s, fallback to raw error',
                status)
            }

            return {
              phase: 'failed',
              message: this.safeSyncFailureMessage(failureStage, stableCategory, errorCode),
              messageKey: messageKey.length > 0 ? messageKey : undefined,
              errorCategory: stableCategory,
              errorCode: errorCode,
              rawError: errorMsg.length > 0 ? errorMsg : undefined,
              operationId: result.diagnosticOperationId,
              failureStage,
              totalDownloaded: syncResult.totalDownloaded,
              totalUploaded: syncResult.totalUploaded,
              totalConflicts: syncResult.totalConflicts,
              overallStatus: status
            }
          }
          if (status === 'syncing') {
            return {
              phase: 'failed', message: '同步仍在进行中（状态异常）', errorCode: 'SYNC_STATE_ERROR',
              operationId: result.diagnosticOperationId,
              failureStage: 'core_sync', errorCategory: 'protocol_error',
              totalDownloaded: syncResult.totalDownloaded, totalUploaded: syncResult.totalUploaded,
              totalConflicts: syncResult.totalConflicts, overallStatus: status
            }
          }
          if (status === 'idle' || status === 'configured_not_tested') {
            return {
              phase: 'failed', message: '同步未执行（状态异常）', errorCode: 'SYNC_STATE_ERROR',
              operationId: result.diagnosticOperationId,
              failureStage: 'core_sync', errorCategory: 'protocol_error',
              totalDownloaded: syncResult.totalDownloaded, totalUploaded: syncResult.totalUploaded,
              totalConflicts: syncResult.totalConflicts, overallStatus: status
            }
          }

          // 未知值是协议错误
          hilog.error(DOMAIN, TAG,
            'performSync: unknown overallStatus=%{public}s failureStage=core_sync ' +
            'failureCategory=protocol_error diagnosticOperationId=%{public}s durationMs=%{public}s',
            status, result.diagnosticOperationId ?? '',
            String(Math.max(0, Date.now() - startedAtMs)))
          return {
            phase: 'failed', message: `未知的同步状态: ${status}`, errorCode: 'SCHEMA_MISMATCH',
            operationId: result.diagnosticOperationId,
            failureStage: 'core_sync', errorCategory: 'protocol_error',
            totalDownloaded: syncResult.totalDownloaded, totalUploaded: syncResult.totalUploaded,
            totalConflicts: syncResult.totalConflicts, overallStatus: status
          }
        }
      )
    } catch (e) {
      if (e instanceof SyncPreflightError) {
        // 保存屏障失败：正文没有可靠落盘，禁止进入 Core 同步（不制造"假成功"）。
        const durationMs: number = Math.max(0, Date.now() - startedAtMs)
        hilog.error(DOMAIN, TAG,
          'performSync: preflight save failed: errorCode=%{public}s failureStage=preflight_save ' +
          'failureCategory=preflight_save_failed diagnosticOperationId= durationMs=%{public}s',
          e.errorCode, String(durationMs))
        return {
          phase: 'failed',
          message: '同步前保存当前正文失败，已取消同步',
          errorCode: e.errorCode.length > 0 ? e.errorCode : SYNC_PREFLIGHT_SAVE_FAILED,
          failureStage: 'preflight_save',
          errorCategory: 'preflight_save_failed'
        }
      }
      // Issue #906：异常只记类型，不记 String(e)（可能含 URL/Token）。
      const errorType: string = e instanceof Error ? e.name : 'Unknown'
      hilog.error(DOMAIN, TAG,
        'performSync: exception: errorType=%{public}s failureStage=arkts_bridge ' +
        'failureCategory=native_bridge_exception diagnosticOperationId= durationMs=%{public}s',
        errorType, String(Math.max(0, Date.now() - startedAtMs)))
      return {
        phase: 'failed', message: '同步异常，请查看诊断信息。', errorCode: 'NATIVE_ERROR',
        failureStage: 'arkts_bridge', errorCategory: 'native_bridge_exception',
        rawError: String(e)
      }
    }
    // 同步屏障（含活跃会话回填）已释放，再通知页面刷新，避免刷新读到未回填的旧会话状态。
    if (contentChanged) {
      this.notifyContentChanged()
    }
    return outcome
  }

  private safeSyncFailureMessage(stage: string | undefined, category: string | undefined,
    errorCode: string | undefined): string {
    if (stage === 'config_load') {
      return '同步配置读取失败，请检查本地同步配置。'
    }
    if (stage === 'secrets_load') {
      return '同步凭据读取失败，请检查本地凭据设置。'
    }
    if (stage === 'core_enter') {
      return '同步核心流程失败，请查看诊断信息。'
    }
    if (stage === 'provider_init') {
      return '云端同步连接初始化失败，请检查同步设置。'
    }
    if (stage === 'preflight') {
      return '同步前检查失败，请查看诊断信息。'
    }
    if (stage === 'prepare' || stage === 'seed') {
      return '本地同步数据准备失败，请查看诊断信息。'
    }
    if (stage === 'transfer' || stage === 'remote_revision' || stage === 'commit') {
      return '同步传输或提交失败，请查看诊断信息。'
    }
    if (stage === 'starmap_store_flush') {
      return '星图数据保存失败，请查看诊断信息。'
    }
    if (stage === 'cleanup') {
      return '同步已完成，但收尾清理失败，请查看诊断信息。'
    }
    switch (category) {
      case 'repository_unconfigured':
        return '云端仓库未配置，请先完成同步设置。'
      case 'sync_core_error':
        return '同步核心流程失败，请查看诊断信息。'
      case 'native_bridge_exception':
        return '原生同步调用异常，请查看诊断信息。'
      case 'preflight_save_failed':
        return '同步前保存当前正文失败，已取消同步。'
      case 'network':
      case 'network_error':
      case 'network_failed':
      case 'retryable_network':
        return '网络连接失败，请检查网络后重试。'
      case 'authentication':
      case 'authentication_error':
      case 'auth_failed':
      case 'auth_error':
      case 'token_missing':
        return '云端认证失败，请检查账号或访问令牌。'
      case 'tls_ca_unavailable':
        return '系统证书库不可用，无法建立安全的 HTTPS 连接。'
      case 'tls_certificate_invalid':
        return '云端 HTTPS 证书校验失败，请检查系统时间或网络代理。'
      case 'git_runtime_init':
      case 'local_git_cache':
      case 'git_remote_config':
        return '本地 Git 同步缓存初始化失败，请重试或查看诊断信息。'
      case 'local_io':
      case 'local_io_error':
      case 'config_local_io':
      case 'config_file_read':
      case 'config_file_decode':
      case 'credentials_local_io':
      case 'credentials_file_read':
      case 'credentials_file_decode':
      case 'secure_storage_read':
      case 'secure_storage_decode':
      case 'credential_migration_io':
      case 'local_decode_error':
        return '读取本地同步数据失败，请检查配置和凭据文件。'
      default:
        return errorCode === undefined ? '同步失败，请查看诊断信息。' : `同步失败（${errorCode}），请查看诊断信息。`
    }
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
